//! Cleanup suggestions (Phase 9.2). Suggestions only: nothing is removed
//! unless the person reviews a list and confirms. Removal goes to the
//! Recycle Bin through the journaled file operations (so Ctrl+Z works); the
//! one exception is emptying the Recycle Bin itself, which asks first.
//! Everything offered lives in the person's own profile (or Temp) and is
//! rebuilt or re-downloaded by the app that made it; nothing needs
//! administrator rights, and Windows system folders are never offered.
//! Sizes come from the index when it covers a folder, and otherwise from a
//! bounded walk that never follows links or reads cloud placeholders.
use seek_engine::{Engine, Query};
use std::{
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant, SystemTime},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    /// Move the ticked items to the Recycle Bin (the person's own files:
    /// installers and unfinished downloads, downloaded models).
    Recycle,
    /// Delete the ticked items in place: caches, logs and build output the
    /// apps that made them rebuild on their own. Recycling hundreds of
    /// thousands of cache files makes Windows record each one and takes
    /// many minutes; deleting them takes seconds.
    Delete,
    /// Empty the Recycle Bin, after a confirmation.
    EmptyRecycleBin,
}

#[derive(Clone, Debug)]
pub struct Suggestion {
    pub title: String,
    pub detail: String,
    /// Something to do first, such as closing a browser.
    pub note: Option<String>,
    pub action: Action,
    /// What would go to the Recycle Bin, with each item's size.
    pub items: Vec<(PathBuf, u64)>,
    /// Size when there are no items to list (the Recycle Bin).
    pub total: Option<u64>,
}
impl Suggestion {
    pub fn bytes(&self) -> u64 {
        self.total
            .unwrap_or_else(|| self.items.iter().map(|(_, b)| b).sum())
    }
}

/// Temp files left alone this long are offered.
const TEMP_AGE: Duration = Duration::from_secs(24 * 3600);
/// Installers and disk images in Downloads left alone this long are offered.
const DOWNLOAD_AGE: Duration = Duration::from_secs(30 * 24 * 3600);
/// A repository untouched this long has its build folders offered under "old projects".
pub const STALE_REPO: Duration = Duration::from_secs(90 * 24 * 3600);
/// A repository untouched for at least a week with a large generated build folder.
pub const RECENT_IDLE_REPO: Duration = Duration::from_secs(7 * 24 * 3600);
/// Minimum size (50 MiB) for a recent-project generated build folder to be suggested.
const LARGE_BUILD_MIN: u64 = 50 << 20;
const BUILD_FOLDERS: [&str; 21] = [
    "node_modules",
    "target",
    ".venv",
    "dist",
    "build",
    ".next",
    "__pycache__",
    ".nuxt",
    ".svelte-kit",
    ".turbo",
    ".parcel-cache",
    ".pytest_cache",
    ".mypy_cache",
    ".ruff_cache",
    ".dart_tool",
    "out",
    "bin",
    "obj",
    "coverage",
    ".angular",
    "storybook-static",
];
/// Items smaller than this aren't worth a line.
const MIN_ITEM: u64 = 4 << 10;
/// How long a whole look may take, and how many entries one folder walk reads.
const DEADLINE: Duration = Duration::from_secs(35);
const WALK_ENTRIES: usize = 500_000;
const INSTALLERS: [&str; 9] = ["exe", "msi", "msix", "msixbundle", "appx", "appxbundle", "iso", "img", "vhdx"];

#[derive(Clone, Copy)]
enum Base {
    Home,
    Local,
    LocalLow,
    Roaming,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Group {
    Developer,
    Models,
    Apps,
    Browsers,
    Shaders,
    Crashes,
    Logs,
    Leftovers,
}

/// A folder an app rebuilds or re-downloads on its own. `*` in `rel`
/// stands for any one folder (browser profiles).
struct Spot {
    group: Group,
    base: Base,
    rel: &'static str,
}

const fn spot(group: Group, base: Base, rel: &'static str) -> Spot {
    Spot { group, base, rel }
}

use Base::*;
use Group::*;
const SPOTS: &[Spot] = &[
    // Package managers and build tools: re-downloaded when needed.
    spot(Developer, Local, r"npm-cache"),
    spot(Developer, Home, r".npm\_cacache"),
    spot(Developer, Local, r"pnpm-cache"),
    spot(Developer, Local, r"pnpm\store"),
    spot(Developer, Local, r"Yarn\Cache"),
    spot(Developer, Home, r".yarn\berry\cache"),
    spot(Developer, Home, r".bun\install\cache"),
    spot(Developer, Home, r".cargo\registry\cache"),
    spot(Developer, Home, r".cargo\registry\src"),
    spot(Developer, Home, r".cargo\git\checkouts"),
    spot(Developer, Home, r".cargo\git\db"),
    spot(Developer, Local, r"pip\Cache"),
    spot(Developer, Local, r"pip\http"),
    spot(Developer, Local, r"pip\http-v2"),
    spot(Developer, Local, r"pip\wheels"),
    spot(Developer, Local, r"uv\cache"),
    spot(Developer, Local, r"pypoetry\Cache"),
    spot(Developer, Home, r".gradle\caches"),
    spot(Developer, Home, r".m2\wrapper\dists"),
    spot(Developer, Home, r"scoop\cache"),
    spot(Developer, Local, r"Chocolatey\cache"),
    spot(Developer, Local, r"vcpkg\downloads"),
    // Not Maven's .m2\repository: it also holds what `mvn install` put there
    // from the person's own projects, which can't be downloaded again.
    spot(Developer, Home, r".nuget\packages"),
    spot(Developer, Home, r".cache\selenium"),
    spot(Developer, Home, r".cache\pre-commit"),
    spot(Developer, Local, r"NuGet\v3-cache"),
    spot(Developer, Local, r"NuGet\http-cache"),
    spot(Developer, Local, r"go-build"),
    spot(Developer, Local, r"deno"),
    spot(Developer, Local, r"Composer"),
    spot(Developer, Local, r"ms-playwright"),
    spot(Developer, Home, r".cache\puppeteer"),
    spot(Developer, Local, r"electron\Cache"),
    spot(Developer, Local, r"electron-builder\Cache"),
    spot(Developer, Local, r"node-gyp\Cache"),
    spot(Developer, Local, r"Microsoft\vscode-cpptools\ipch"),
    spot(Developer, Home, r".gradle\wrapper\dists"),
    spot(Developer, Home, r".rustup\downloads"),
    spot(Developer, Home, r".rustup\tmp"),
    spot(Developer, Home, r".cargo\registry\index"),
    spot(Developer, Home, r"go\pkg\mod\cache\download"),
    spot(Developer, Home, r"anaconda3\pkgs"),
    spot(Developer, Home, r"miniconda3\pkgs"),
    spot(Developer, Home, r".conda\pkgs"),
    spot(Developer, Local, r"JetBrains\*\caches"),
    spot(Developer, Local, r"Google\*\caches"),
    spot(Developer, Local, r"Microsoft\TypeScript"),
    spot(Developer, Local, r"node\corepack"),
    spot(Developer, Home, r".dotnet\toolResolverCache"),
    spot(Developer, Local, r"npm-cache\_npx"),
    spot(Developer, Local, r"Yarn\Berry\cache"),
    spot(Developer, Local, r"Yarn\Data\cache"),
    spot(Developer, Local, r"Cypress\Cache"),
    spot(Developer, Local, r"Pub\Cache"),
    spot(Developer, Local, r"Vcpkg\archives"),
    spot(Developer, Local, r"sccache"),
    spot(Developer, Local, r"Mozilla\sccache"),
    spot(Developer, Local, r"Zig"),
    spot(Developer, Local, r"Android\Sdk\temp"),
    spot(Developer, Home, r".android\cache"),
    spot(Developer, Home, r".android\build-cache"),
    spot(Developer, Local, r"SymbolCache"),
    spot(Developer, Local, r"Microsoft\VisualStudio\*\ComponentModelCache"),
    spot(Developer, Local, r"Microsoft\VisualStudio\*\Roslyn\Cache"),
    spot(Developer, Local, r"Ruff\cache"),
    spot(Developer, Local, r"mypy"),
    spot(Developer, Local, r"hatch\cache"),
    spot(Developer, Local, r"pdm\Cache"),
    spot(Developer, Local, r"pipenv\Cache"),
    spot(Developer, Local, r"mamba\pkgs"),
    spot(Developer, Home, r".mamba\pkgs"),
    spot(Developer, Local, r"Coursier\cache"),
    spot(Developer, Home, r".ivy2\cache"),
    spot(Developer, Local, r"esbuild"),
    spot(Developer, Local, r"turbo"),
    spot(Developer, Local, r"swc"),
    spot(Developer, Local, r"tsx"),
    spot(Developer, Local, r"Biome"),
    spot(Developer, Local, r"vite"),
    spot(Developer, Home, r".cache\pip"),
    spot(Developer, Home, r".cache\uv"),
    spot(Developer, Home, r".cache\yarn"),
    spot(Developer, Home, r".cache\node"),
    spot(Developer, Home, r".cache\ms-playwright"),
    spot(Developer, Home, r".cache\turbo"),
    spot(Developer, Home, r".cache\zig"),
    spot(Developer, Home, r".cache\sccache"),
    spot(Developer, Home, r".cache\ccache"),
    spot(Developer, Home, r".cache\go-build"),
    spot(Developer, Home, r".cache\cypress"),
    spot(Developer, Home, r".cache\electron"),
    spot(Developer, Home, r".cache\electron-builder"),
    spot(Developer, Home, r".cache\prisma"),
    spot(Developer, Home, r".cache\bazel"),
    spot(Developer, Home, r".cache\pnpm"),
    spot(Developer, Home, r".cache\bun"),
    spot(Developer, Home, r".cache\gradle"),
    spot(Developer, Home, r".cache\helm"),
    spot(Developer, Home, r".cache\vcpkg"),
    spot(Developer, Home, r".cache\composer"),
    spot(Developer, Home, r".cache\coursier"),
    spot(Developer, Local, r"JetBrains\*\index"),
    spot(Developer, Local, r"Android\Sdk\.temp"),
    // Downloaded models: large, and slow to download again.
    // The downloaded models only: .cache\huggingface also keeps the sign-in
    // token, which must never go.
    spot(Models, Home, r".cache\huggingface\hub"),
    spot(Models, Home, r".cache\huggingface\xet"),
    spot(Models, Home, r".cache\huggingface\datasets"),
    spot(Models, Home, r".cache\torch"),
    spot(Models, Home, r".cache\whisper"),
    spot(Models, Home, r".cache\clip"),
    spot(Models, Home, r".cache\sentence_transformers"),
    // Editors and chat apps rebuild these on the next start.
    spot(Apps, Roaming, r"Code\Cache"),
    spot(Apps, Roaming, r"Code\CachedData"),
    spot(Apps, Roaming, r"Code\CachedExtensionVSIXs"),
    spot(Apps, Roaming, r"Code\Code Cache"),
    spot(Apps, Roaming, r"Code\GPUCache"),
    spot(Apps, Roaming, r"Code - Insiders\Cache"),
    spot(Apps, Roaming, r"Code - Insiders\CachedData"),
    spot(Apps, Roaming, r"Code - Insiders\CachedExtensionVSIXs"),
    spot(Apps, Roaming, r"Code - Insiders\Code Cache"),
    spot(Apps, Roaming, r"Code - Insiders\GPUCache"),
    spot(Apps, Roaming, r"Cursor\Cache"),
    spot(Apps, Roaming, r"Cursor\CachedData"),
    spot(Apps, Roaming, r"Cursor\CachedExtensionVSIXs"),
    spot(Apps, Roaming, r"Cursor\Code Cache"),
    spot(Apps, Roaming, r"Cursor\GPUCache"),
    spot(Apps, Roaming, r"Cursor\DawnCache"),
    spot(Apps, Roaming, r"Cursor\DawnGraphiteCache"),
    spot(Apps, Roaming, r"Cursor\DawnWebGPUCache"),
    spot(Apps, Roaming, r"Claude\Cache"),
    spot(Apps, Roaming, r"Claude\Code Cache"),
    spot(Apps, Roaming, r"Claude\GPUCache"),
    spot(Apps, Roaming, r"Claude\DawnCache"),
    spot(Apps, Roaming, r"Claude\DawnGraphiteCache"),
    spot(Apps, Roaming, r"Claude\DawnWebGPUCache"),
    spot(Apps, Roaming, r"Notion\Cache"),
    spot(Apps, Roaming, r"Notion\Code Cache"),
    spot(Apps, Roaming, r"Notion\GPUCache"),
    spot(Apps, Roaming, r"Figma\Cache"),
    spot(Apps, Roaming, r"Figma\Code Cache"),
    spot(Apps, Roaming, r"Figma\GPUCache"),
    spot(Apps, Roaming, r"Obsidian\Cache"),
    spot(Apps, Roaming, r"Obsidian\Code Cache"),
    spot(Apps, Roaming, r"Obsidian\GPUCache"),
    spot(Apps, Roaming, r"Postman\Cache"),
    spot(Apps, Roaming, r"Postman\Code Cache"),
    spot(Apps, Roaming, r"Postman\GPUCache"),
    spot(Apps, Roaming, r"Windsurf\Cache"),
    spot(Apps, Roaming, r"Windsurf\CachedData"),
    spot(Apps, Roaming, r"Windsurf\Code Cache"),
    spot(Apps, Roaming, r"Windsurf\GPUCache"),
    spot(Apps, Roaming, r"Antigravity\Cache"),
    spot(Apps, Roaming, r"Antigravity\CachedData"),
    spot(Apps, Roaming, r"Antigravity\Code Cache"),
    spot(Apps, Roaming, r"Antigravity\GPUCache"),
    spot(Apps, Roaming, r"discord\Cache"),
    spot(Apps, Roaming, r"discord\Code Cache"),
    spot(Apps, Roaming, r"Slack\Cache"),
    spot(Apps, Roaming, r"Slack\Code Cache"),
    spot(Apps, Roaming, r"Microsoft\Teams\Cache"),
    spot(Apps, Local, r"Spotify\Data"),
    spot(Apps, Local, r"Spotify\Storage"),
    spot(Apps, Roaming, r"Adobe\Common\Media Cache Files"),
    spot(Apps, Roaming, r"Adobe\Common\Media Cache"),
    spot(Apps, Roaming, r"Adobe\Common\Peak Files"),
    spot(Apps, Local, r"CapCut\User Data\Cache"),
    spot(Apps, Local, r"Steam\htmlcache\Cache"),
    spot(Apps, Local, r"Steam\htmlcache\Code Cache"),
    spot(Apps, Local, r"Steam\htmlcache\GPUCache"),
    spot(Apps, Local, r"EpicGamesLauncher\Saved\webcache"),
    spot(Apps, Local, r"EpicGamesLauncher\Saved\webcache_4430"),
    spot(Apps, Local, r"Battle.net\Cache"),
    spot(Apps, Local, r"Riot Games\Riot Client\Data\Cache"),
    spot(Apps, Local, r"UnrealEngine\Common\DerivedDataCache"),
    spot(Apps, Local, r"Unity\cache"),
    spot(Apps, Local, r"Blender Foundation\Blender\*\cache"),
    spot(Apps, Local, r"Packages\*\LocalCache\Microsoft\MSTeams\EBWebView\*\Cache"),
    spot(Apps, Local, r"Packages\*\LocalCache\Microsoft\MSTeams\EBWebView\*\Code Cache"),
    spot(Apps, Local, r"Packages\*\TempState"),
    spot(Apps, Local, r"Packages\*\AC\Temp"),
    spot(Apps, Local, r"Microsoft\EdgeWebView\User Data\*\Cache"),
    spot(Apps, Local, r"Microsoft\EdgeWebView\User Data\*\Code Cache"),
    spot(Apps, Local, r"Microsoft\EdgeWebView\User Data\*\GPUCache"),
    spot(Apps, Local, r"Microsoft\EdgeWebView\User Data\GrShaderCache"),
    spot(Apps, Local, r"Microsoft\EdgeWebView\User Data\ShaderCache"),
    // Store apps' web caches and Windows' own internet/WebCache caches.
    spot(Apps, Local, r"Packages\*\AC\INetCache"),
    spot(Apps, Local, r"Packages\*\LocalCache\Local\Microsoft\Windows\INetCache"),
    spot(Apps, Local, r"Microsoft\Windows\INetCache"),
    spot(Apps, Local, r"Microsoft\Windows\Caches"),
    spot(Apps, Local, r"Microsoft\Windows\WebCache"),
    spot(Apps, Local, r"Microsoft\Windows\SharedWebView\EBWebView\*\Cache"),
    spot(Apps, Local, r"Microsoft\Windows\SharedWebView\EBWebView\*\Code Cache"),
    spot(Apps, Local, r"NVIDIA Corporation\*\CefCache"),
    // Old update packages an app already installed.
    spot(Leftovers, Roaming, r"Telegram Desktop\tupdates"),
    spot(Leftovers, Local, r"Microsoft\EdgeUpdate\Download"),
    spot(Leftovers, Local, r"Google\Update\Download"),
    spot(Leftovers, Local, r"BraveSoftware\Update\Download"),
    // Browser caches, per profile.
    spot(Browsers, Local, r"Google\Chrome\User Data\*\Cache"),
    spot(Browsers, Local, r"Google\Chrome\User Data\*\Code Cache"),
    spot(Browsers, Local, r"Microsoft\Edge\User Data\*\Cache"),
    spot(Browsers, Local, r"Microsoft\Edge\User Data\*\Code Cache"),
    spot(Browsers, Local, r"BraveSoftware\Brave-Browser\User Data\*\Cache"),
    spot(Browsers, Local, r"BraveSoftware\Brave-Browser\User Data\*\Code Cache"),
    spot(Browsers, Local, r"Mozilla\Firefox\Profiles\*\cache2"),
    spot(Browsers, Local, r"Mozilla\Firefox\Profiles\*\startupCache"),
    spot(Browsers, Local, r"Mozilla\Firefox\Profiles\*\jumpListCache"),
    spot(Browsers, Local, r"Mozilla\Firefox\Profiles\*\thumbnails"),
    spot(Browsers, Local, r"Zen\Profiles\*\cache2"),
    spot(Browsers, Local, r"LibreWolf\Profiles\*\cache2"),
    spot(Browsers, Local, r"Waterfox\Profiles\*\cache2"),
    spot(Browsers, Local, r"Google\Chrome\User Data\*\Service Worker\CacheStorage"),
    spot(Browsers, Local, r"Google\Chrome\User Data\*\GPUCache"),
    spot(Browsers, Local, r"Google\Chrome\User Data\GrShaderCache"),
    spot(Browsers, Local, r"Google\Chrome\User Data\ShaderCache"),
    spot(Browsers, Local, r"Microsoft\Edge\User Data\*\Service Worker\CacheStorage"),
    spot(Browsers, Local, r"Microsoft\Edge\User Data\*\GPUCache"),
    spot(Browsers, Local, r"Microsoft\Edge\User Data\GrShaderCache"),
    spot(Browsers, Local, r"Microsoft\Edge\User Data\ShaderCache"),
    spot(Browsers, Local, r"BraveSoftware\Brave-Browser\User Data\*\Service Worker\CacheStorage"),
    spot(Browsers, Local, r"Vivaldi\User Data\*\Cache"),
    spot(Browsers, Local, r"Vivaldi\User Data\*\Code Cache"),
    spot(Browsers, Local, r"Opera Software\*\Cache"),
    spot(Browsers, Local, r"Opera Software\*\Default\Cache"),
    spot(Browsers, Roaming, r"Opera Software\*\Default\Code Cache"),
    spot(Browsers, Roaming, r"Opera Software\*\Default\GPUCache"),
    spot(Browsers, Local, r"Arc\User Data\*\Cache"),
    spot(Browsers, Local, r"Arc\User Data\*\Code Cache"),
    spot(Browsers, Local, r"Arc\User Data\*\GPUCache"),
    spot(Browsers, Local, r"Dia\User Data\*\Cache"),
    spot(Browsers, Local, r"Dia\User Data\*\Code Cache"),
    spot(Browsers, Local, r"Perplexity\Comet\User Data\*\Cache"),
    spot(Browsers, Local, r"Perplexity\Comet\User Data\*\Code Cache"),
    spot(Browsers, Local, r"Thorium\User Data\*\Cache"),
    spot(Browsers, Local, r"Thorium\User Data\*\Code Cache"),
    spot(Browsers, Local, r"Floorp\Profiles\*\cache2"),
    // Graphics drivers rebuild compiled shaders as games and apps run.
    spot(Shaders, Local, r"D3DSCache"),
    spot(Shaders, Local, r"NVIDIA\DXCache"),
    spot(Shaders, Local, r"NVIDIA\GLCache"),
    spot(Shaders, Local, r"AMD\DxCache"),
    spot(Shaders, Local, r"AMD\DxcCache"),
    spot(Shaders, Local, r"AMD\GLCache"),
    spot(Shaders, Local, r"Intel\ShaderCache"),
    spot(Shaders, LocalLow, r"AMD\DxCache"),
    spot(Shaders, LocalLow, r"AMD\DxcCache"),
    spot(Shaders, LocalLow, r"Intel\ShaderCache"),
    spot(Shaders, LocalLow, r"NVIDIA\PerDriverVersion\DXCache"),
    spot(Shaders, Roaming, r"NVIDIA\ComputeCache"),
    // Crash dumps and the person's own error reports.
    spot(Crashes, Local, r"CrashDumps"),
    spot(Crashes, Local, r"Microsoft\Windows\WER\ReportArchive"),
    spot(Crashes, Local, r"Microsoft\Windows\WER\ReportQueue"),
    spot(Crashes, Local, r"Microsoft\Windows\WER\Temp"),
    spot(Crashes, Local, r"npm-cache\_logs"),
    spot(Crashes, Local, r"EpicGamesLauncher\Saved\Crashes"),
    // Logs apps rotate on their own.
    spot(Logs, Local, r"Docker\log"),
    spot(Logs, Roaming, r"Docker\log"),
    spot(Logs, Local, r"JetBrains\*\log"),
    spot(Logs, Roaming, r"Claude\logs"),
    spot(Logs, Roaming, r"Cursor\logs"),
    spot(Logs, Roaming, r"Code\logs"),
    spot(Logs, Roaming, r"Antigravity\logs"),
    spot(Logs, Local, r"EpicGamesLauncher\Saved\Logs"),
    spot(Logs, Local, r"Battle.net\Logs"),
    spot(Logs, Local, r"Riot Games\Riot Client\Logs"),
    spot(Logs, Home, r".gradle\daemon"),
    // What installers and updaters leave behind.
    spot(Leftovers, Local, r"SquirrelTemp"),
    spot(Leftovers, Local, r"AnthropicClaude\packages"),
    spot(Leftovers, Local, r"Discord\packages"),
    spot(Leftovers, Local, r"Slack\packages"),
    spot(Leftovers, Local, r"Downloaded Installations"),
    spot(Leftovers, Local, r"Microsoft\Windows\Temporary Internet Files"),
];

/// Folders inside a Chromium browser profile, or an Electron or WebView2
/// app's data folder, that the app rebuilds: its web cache, compiled
/// scripts and shaders, and service workers' caches (not their data).
const PROFILE_CACHES: [&str; 8] = [
    "Cache",
    "Code Cache",
    "GPUCache",
    "DawnCache",
    "DawnGraphiteCache",
    "DawnWebGPUCache",
    r"Service Worker\CacheStorage",
    r"Service Worker\ScriptCache",
];
/// Beside a Chromium browser's `Local State`: downloaded components and
/// shader caches shared by its profiles.
const BROWSER_CACHES: [&str; 4] = ["component_crx_cache", "GrShaderCache", "ShaderCache", "GraphiteDawnCache"];
/// AppData folders the shape search never enters: installed programs and
/// packages (not caches), Store apps' data, and Temp (offered separately).
const PROFILE_SKIP: [&str; 8] = [
    "packages", "programs", "winget", "temp", "site-packages", "node_modules", "lib", "python",
];
/// Under Microsoft: Windows' own folders and Office, OneDrive and WinGet.
const MICROSOFT_SKIP: [&str; 7] = ["windows", "office", "winget", "windowsapps", "onedrive", "outlook", "credentials"];

/// Browser and app caches (plus Chromium/Electron `Crashpad\reports` dumps)
/// found by shape rather than by name, so any Chromium browser or
/// Electron/WebView2 app is covered: a folder holding `Code Cache` or
/// `GPUCache` is a profile, and a folder holding a `Local State` file is a
/// Chromium browser. A folder merely called "Cache" in some other app is never
/// offered.
fn profile_caches(home: &Path) -> Vec<(Group, PathBuf)> {
    let mut out = vec![];
    let mut stack: Vec<(PathBuf, usize)> = ["Local", "LocalLow", "Roaming"]
        .iter()
        .map(|b| (home.join("AppData").join(b), 0))
        .collect();
    let deadline = Instant::now() + Duration::from_secs(6);
    while let Some((dir, depth)) = stack.pop() {
        if Instant::now() >= deadline {
            break;
        }
        let browser = dir
            .ancestors()
            .any(|a| a.file_name().is_some_and(|n| n.eq_ignore_ascii_case("User Data")))
            || dir.to_string_lossy().contains("Opera Software");
        let crashpad = dir.join("Crashpad").join("reports");
        if depth > 0 && is_plain_dir(&crashpad) {
            out.push((Crashes, crashpad));
        }
        if dir.join("Local State").is_file() {
            out.extend(
                BROWSER_CACHES
                    .iter()
                    .map(|c| dir.join(c))
                    .filter(|p| is_plain_dir(p))
                    .map(|p| (Browsers, p)),
            );
        }
        if depth > 0
            && (is_plain_dir(&dir.join("Code Cache"))
                || is_plain_dir(&dir.join("GPUCache"))
                || is_plain_dir(&dir.join("DawnCache"))
                || is_plain_dir(&dir.join("DawnGraphiteCache")))
        {
            let group = if browser { Browsers } else { Apps };
            out.extend(
                PROFILE_CACHES
                    .iter()
                    .map(|c| dir.join(c))
                    .filter(|p| is_plain_dir(p))
                    .map(|p| (group, p)),
            );
            continue;
        }
        if depth >= 6 {
            continue;
        }
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        for entry in entries.flatten() {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().to_ascii_lowercase();
            // Windows' own folders stay out, except the shared WebView
            // spots listed by name.
            let under_microsoft = dir
                .file_name()
                .is_some_and(|n| n.eq_ignore_ascii_case("Microsoft"));
            if (depth == 0 && PROFILE_SKIP.contains(&name.as_str()))
                || (under_microsoft && MICROSOFT_SKIP.contains(&name.as_str()))
                || name == "node_modules"
                || name == "site-packages"
                || name.starts_with('.')
                || !is_plain_dir(&path)
            {
                continue;
            }
            stack.push((path, depth + 1));
        }
    }
    out
}

/// Log folders apps keep in AppData: apps start new logs, and old ones only
/// help when reporting a problem. Windows' own logs are not included.
fn app_logs(home: &Path) -> Vec<PathBuf> {
    let mut out = vec![];
    for base in ["Local", "Roaming"] {
        let base = home.join("AppData").join(base);
        let Ok(apps) = std::fs::read_dir(&base) else { continue };
        for app in apps.flatten() {
            let app = app.path();
            let name = app.file_name().map(|n| n.to_string_lossy().to_ascii_lowercase());
            if matches!(name.as_deref(), Some("microsoft" | "packages" | "temp")) || !is_plain_dir(&app) {
                continue;
            }
            // <app>\logs and <app>\<part>\logs.
            let mut candidates = vec![app.join("logs"), app.join("Logs")];
            if let Ok(parts) = std::fs::read_dir(&app) {
                for part in parts.flatten().take(64) {
                    candidates.push(part.path().join("logs"));
                }
            }
            for c in candidates {
                if is_plain_dir(&c) && !out.iter().any(|o: &PathBuf| o.as_os_str().eq_ignore_ascii_case(c.as_os_str())) {
                    out.push(c);
                }
            }
        }
    }
    // OneDrive's own logs sit under Microsoft, which is otherwise skipped.
    let onedrive = home.join(r"AppData\Local\Microsoft\OneDrive\logs");
    if is_plain_dir(&onedrive) {
        out.push(onedrive);
    }
    out
}

/// The folders a spot names on this PC (wildcards expanded one level).
fn expand(home: &Path, spot: &Spot) -> Vec<PathBuf> {
    let base = match spot.base {
        Home => home.to_path_buf(),
        Local => home.join(r"AppData\Local"),
        LocalLow => home.join(r"AppData\LocalLow"),
        Roaming => home.join(r"AppData\Roaming"),
    };
    let mut paths = vec![base];
    for part in spot.rel.split('\\') {
        paths = if part == "*" {
            paths
                .iter()
                .filter_map(|p| std::fs::read_dir(p).ok())
                .flat_map(|e| e.flatten().map(|e| e.path()))
                .filter(|p| is_plain_dir(p))
                .collect()
        } else {
            paths.into_iter().map(|p| p.join(part)).collect()
        };
    }
    paths.retain(|p| is_plain_dir(p));
    paths
}

/// A folder that is not a link or junction.
fn is_plain_dir(path: &Path) -> bool {
    use std::os::windows::fs::MetadataExt;
    std::fs::symlink_metadata(path).is_ok_and(|m| m.is_dir() && m.file_attributes() & 0x400 == 0)
}

fn older_than(path: &Path, age: Duration, now: SystemTime) -> bool {
    std::fs::symlink_metadata(path)
        .and_then(|m| m.modified())
        .is_ok_and(|t| now.duration_since(t).is_ok_and(|d| d >= age))
}

/// When a repository was last touched: its .git index or HEAD, or the folder.
fn repo_touched(repo: &Path) -> Option<SystemTime> {
    let git = repo.join(".git");
    [git.join("index"), git.join("HEAD"), git.clone(), repo.to_path_buf()]
        .iter()
        .filter_map(|p| std::fs::metadata(p).and_then(|m| m.modified()).ok())
        .max()
}

/// Folders that only ever hold what a tool generated. `target`, `dist`,
/// `build`, `out`, `bin`, `obj` and similar names may hold source in some
/// projects, so those count only when the repository's `.gitignore` ignores
/// them.
const ALWAYS_GENERATED: [&str; 12] = [
    "node_modules",
    ".venv",
    ".next",
    "__pycache__",
    ".nuxt",
    ".svelte-kit",
    ".turbo",
    ".parcel-cache",
    ".pytest_cache",
    ".mypy_cache",
    ".ruff_cache",
    ".dart_tool",
];

/// Whether the repository's top-level .gitignore ignores `name` there.
fn ignored_by_git(repo: &Path, name: &str) -> bool {
    let Ok(text) = std::fs::read_to_string(repo.join(".gitignore")) else {
        return false;
    };
    let name = name.to_ascii_lowercase();
    text.lines().map(str::trim).filter(|l| !l.starts_with('#')).any(|line| {
        let line = line.to_ascii_lowercase();
        let bare = line.trim_start_matches("**/").trim_start_matches('/').trim_end_matches("/**").trim_end_matches('/');
        bare == name
    })
}

/// A build folder belongs to a repository at its parent that has been left
/// alone for at least `min_age`, is generated (see ALWAYS_GENERATED) or
/// ignored by `.gitignore`, and is not itself inside another build folder.
fn generated_build_folder_older_than(folder: &Path, now: SystemTime, min_age: Duration) -> bool {
    let Some(repo) = folder.parent() else {
        return false;
    };
    let Some(name) = folder.file_name().map(|n| n.to_string_lossy().to_string()) else {
        return false;
    };
    let generated =
        ALWAYS_GENERATED.iter().any(|g| name.eq_ignore_ascii_case(g)) || ignored_by_git(repo, &name);
    if !generated {
        return false;
    }
    let nested = folder.ancestors().skip(1).any(|a| {
        a.file_name()
            .is_some_and(|n| BUILD_FOLDERS.iter().any(|b| n.eq_ignore_ascii_case(b)))
    });
    !nested
        && std::fs::symlink_metadata(repo.join(".git")).is_ok()
        && repo_touched(repo).is_some_and(|t| now.duration_since(t).is_ok_and(|d| d >= min_age))
}

/// A build folder belongs to a repository at its parent that has been left
/// alone long enough (90 days), is generated (see ALWAYS_GENERATED), and is not
/// itself inside another build folder.
pub fn stale_build_folder(folder: &Path, now: SystemTime) -> bool {
    generated_build_folder_older_than(folder, now, STALE_REPO)
}

/// Bytes under `path` by walking it: links and junctions aren't followed,
/// cloud placeholders count as nothing (their bytes aren't on this PC).
pub fn disk_size(path: &Path, deadline: Instant, cancel: &AtomicBool) -> u64 {
    use std::os::windows::fs::MetadataExt;
    const CLOUD: u32 = 0x400000 | 0x40000 | 0x1000;
    let Ok(meta) = std::fs::symlink_metadata(path) else {
        return 0;
    };
    if !meta.is_dir() {
        return if meta.file_attributes() & CLOUD == 0 { meta.len() } else { 0 };
    }
    let mut total = 0u64;
    let mut seen = 0usize;
    let mut stack = vec![path.to_path_buf()];
    while let Some(dir) = stack.pop() {
        if cancel.load(Ordering::Relaxed) || Instant::now() >= deadline || seen >= WALK_ENTRIES {
            break;
        }
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        for entry in entries.flatten() {
            seen += 1;
            let Ok(meta) = entry.metadata() else { continue };
            let attributes = meta.file_attributes();
            if attributes & 0x400 != 0 {
                continue;
            }
            if meta.is_dir() {
                stack.push(entry.path());
            } else if attributes & CLOUD == 0 {
                total += meta.len();
            }
        }
    }
    total
}

/// The index's size for a folder it covers, otherwise a walk of the disk.
fn size(engine: Option<&Engine>, path: &Path, deadline: Instant, cancel: &AtomicBool) -> u64 {
    let indexed = engine.map_or(0, |e| e.subtree_bytes(path, deadline, cancel));
    if indexed > 0 {
        indexed
    } else {
        disk_size(path, deadline, cancel)
    }
}

/// Drops items inside another listed item, so nothing is counted twice.
fn without_nested(mut items: Vec<(PathBuf, u64)>) -> Vec<(PathBuf, u64)> {
    items.sort_by_key(|(p, _)| p.as_os_str().len());
    let mut kept: Vec<(PathBuf, u64)> = vec![];
    for (path, bytes) in items {
        if !kept.iter().any(|(k, _)| path.starts_with(k)) {
            kept.push((path, bytes));
        }
    }
    kept.sort_by_key(|(_, b)| std::cmp::Reverse(*b));
    kept
}

fn group_text(group: Group) -> (&'static str, &'static str, Option<&'static str>) {
    match group {
        Developer => (
            "Developer caches",
            "npm, pnpm, Yarn, Cargo, pip, uv, Gradle, Maven, NuGet, Go, Playwright and others download these again when needed",
            None,
        ),
        Apps => (
            "App caches",
            "Desktop apps (VS Code, Discord, Slack, Teams, Notion and others) and Store apps rebuild these on their next start",
            Some("Close these apps first; files they have open are skipped"),
        ),
        Browsers => (
            "Browser caches",
            "Saved copies of web pages; history, passwords and sign-ins are not touched",
            Some("Close the browser first; files it has open are skipped"),
        ),
        Shaders => (
            "Graphics shader caches",
            "Your graphics driver rebuilds these as games and apps run",
            None,
        ),
        Crashes => (
            "Crash dumps, error reports and logs",
            "Left behind by apps that crashed; only useful when reporting the crash",
            None,
        ),
        Models => (
            "Downloaded AI models",
            "Hugging Face and PyTorch download these again when a project needs them",
            Some("Large to download again; keep the ones you still use"),
        ),
        Logs => (
            "App logs",
            "Apps start new logs; old ones only help when reporting a problem",
            Some("Logs an app has open are skipped"),
        ),
        Leftovers => (
            "Leftover installer files",
            "Unpacked setup files and updates that installers didn't clean up",
            None,
        ),
    }
}

/// What a look found, and how far it got.
pub struct Report {
    pub suggestions: Vec<Suggestion>,
    pub elapsed: Duration,
    pub stopped: Option<String>,
}

/// Looks for everything worth suggesting. `progress` names what is being
/// checked; `repositories` are folders holding a `.git` (from the index or a
/// bounded walk of the profile).
pub fn suggest(
    engine: Option<&Engine>,
    home: &Path,
    repositories: &[PathBuf],
    cancel: &AtomicBool,
    progress: impl Fn(&str),
) -> Report {
    let started = Instant::now();
    let deadline = started + DEADLINE;
    let now = SystemTime::now();
    let mut out = vec![];
    // Caches apps rebuild, by group. Folders are sized in parallel; each
    // walk is independent and bounded.
    progress("Caches");
    let mut spots: Vec<(Group, PathBuf)> = SPOTS
        .iter()
        .flat_map(|spot| expand(home, spot).into_iter().map(move |p| (spot.group, p)))
        .collect();
    spots.extend(profile_caches(home));
    spots.extend(app_logs(home).into_iter().map(|p| (Logs, p)));
    let next = std::sync::atomic::AtomicUsize::new(0);
    let sized = std::sync::Mutex::new(Vec::new());
    let workers = std::thread::available_parallelism().map_or(4, |n| n.get().clamp(2, 8));
    std::thread::scope(|scope| {
        for _ in 0..workers {
            scope.spawn(|| {
                loop {
                    let i = next.fetch_add(1, Ordering::Relaxed);
                    let Some((group, path)) = spots.get(i) else { break };
                    if cancel.load(Ordering::Relaxed) {
                        break;
                    }
                    let bytes = size(engine, path, deadline, cancel);
                    if let Ok(mut sized) = sized.lock() {
                        sized.push((*group, path.clone(), bytes));
                    }
                }
            });
        }
    });
    let sized = sized.into_inner().unwrap_or_default();
    for group in [Developer, Models, Apps, Browsers, Shaders, Crashes, Logs, Leftovers] {
        let (title, detail, note) = group_text(group);
        let items = without_nested(
            sized
                .iter()
                .filter(|(g, _, b)| *g == group && *b >= MIN_ITEM)
                .map(|(_, p, b)| (p.clone(), *b))
                .collect(),
        );
        if !items.is_empty() {
            out.push(Suggestion {
                title: title.into(),
                detail: detail.into(),
                note: note.map(Into::into),
                action: if group == Models { Action::Recycle } else { Action::Delete },
                items,
                total: None,
            });
        }
    }
    // Old temporary files.
    progress("Temporary files");
    let mut temp_items = vec![];
    if let Ok(entries) = std::fs::read_dir(std::env::temp_dir()) {
        for entry in entries.flatten().take(20_000) {
            let path = entry.path();
            if !older_than(&path, TEMP_AGE, now) {
                continue;
            }
            let bytes = match std::fs::symlink_metadata(&path) {
                Ok(m) if !m.is_dir() => m.len(),
                Ok(_) => size(engine, &path, deadline, cancel),
                Err(_) => 0,
            };
            if bytes > 0 {
                temp_items.push((path, bytes));
            }
        }
    }
    if temp_items.iter().map(|(_, b)| b).sum::<u64>() > 0 {
        temp_items.sort_by_key(|(_, b)| std::cmp::Reverse(*b));
        out.push(Suggestion {
            title: "Old temporary files".into(),
            detail: "In your Temp folder and untouched for a day".into(),
            note: Some("Files still in use are skipped".into()),
            action: Action::Delete,
            items: temp_items,
            total: None,
        });
    }
    // Installers and disk images in Downloads.
    progress("Downloads");
    let mut installers = vec![];
    if let Ok(entries) = std::fs::read_dir(home.join("Downloads")) {
        for entry in entries.flatten() {
            let path = entry.path();
            let installer = path
                .extension()
                .and_then(|e| e.to_str())
                .is_some_and(|e| INSTALLERS.iter().any(|i| e.eq_ignore_ascii_case(i)));
            if installer
                && older_than(&path, DOWNLOAD_AGE, now)
                && let Ok(meta) = entry.metadata()
                && meta.is_file()
                && meta.len() >= MIN_ITEM
            {
                installers.push((path, meta.len()));
            }
        }
    }
    // Downloads that never finished, at any age.
    let mut unfinished = vec![];
    if let Ok(entries) = std::fs::read_dir(home.join("Downloads")) {
        for entry in entries.flatten() {
            let path = entry.path();
            let partial = path
                .extension()
                .and_then(|e| e.to_str())
                .is_some_and(|e| {
                    ["crdownload", "part", "partial", "download", "opdownload"]
                        .iter()
                        .any(|x| e.eq_ignore_ascii_case(x))
                });
            if partial
                && older_than(&path, Duration::from_secs(3600), now)
                && let Ok(meta) = entry.metadata()
                && meta.is_file()
            {
                unfinished.push((path, meta.len()));
            }
        }
    }
    if !unfinished.is_empty() {
        out.push(Suggestion {
            title: "Unfinished downloads".into(),
            detail: "Partial files from downloads that stopped or failed".into(),
            note: None,
            action: Action::Recycle,
            items: unfinished,
            total: None,
        });
    }
    if !installers.is_empty() {
        installers.sort_by_key(|(_, b)| std::cmp::Reverse(*b));
        out.push(Suggestion {
            title: "Old installers in Downloads".into(),
            detail: "Setup files and disk images downloaded over a month ago".into(),
            note: None,
            action: Action::Recycle,
            items: installers,
            total: None,
        });
    }
    // Build folders in repositories left alone for months, plus large
    // generated build caches in repos untouched for at least a week.
    progress("Project build folders");
    let mut builds = vec![];
    let mut recent_large_builds = vec![];
    let mut repos: Vec<PathBuf> = repositories.to_vec();
    if let Some(engine) = engine {
        for name in BUILD_FOLDERS {
            let Ok(mut query) = Query::parse(&format!("'{name} kind:folder"), "") else {
                continue;
            };
            query.limit = 2_000;
            repos.extend(
                engine
                    .search(&query)
                    .unwrap_or_default()
                    .into_iter()
                    .filter(|f| f.path.file_name().is_some_and(|n| n.eq_ignore_ascii_case(name)))
                    .filter_map(|f| f.path.parent().map(Path::to_path_buf)),
            );
        }
    }
    repos.sort();
    repos.dedup();
    for repo in repos {
        if cancel.load(Ordering::Relaxed) {
            break;
        }
        for name in BUILD_FOLDERS {
            let folder = repo.join(name);
            if !is_plain_dir(&folder) {
                continue;
            }
            if stale_build_folder(&folder, now) {
                let bytes = size(engine, &folder, deadline, cancel);
                if bytes >= MIN_ITEM {
                    builds.push((folder, bytes));
                }
            } else if generated_build_folder_older_than(&folder, now, RECENT_IDLE_REPO) {
                let bytes = size(engine, &folder, deadline, cancel);
                if bytes >= LARGE_BUILD_MIN {
                    recent_large_builds.push((folder, bytes));
                }
            }
        }
    }
    if !builds.is_empty() {
        out.push(Suggestion {
            title: "Build folders in old projects".into(),
            detail: "Generated build, dependency and cache folders (node_modules, target, .venv, dist, .next, __pycache__ and others) in repositories untouched for 90 days".into(),
            note: Some("Rebuilt the next time you build the project".into()),
            action: Action::Delete,
            items: without_nested(builds),
            total: None,
        });
    }
    if !recent_large_builds.is_empty() {
        out.push(Suggestion {
            title: "Large build folders in recent projects".into(),
            detail: "Generated build folders of 50 MB or more (target, node_modules, .venv, dist and others) in repositories untouched for at least 7 days".into(),
            note: Some("Rebuilt next time you compile; uncheck any active project you want to keep warm".into()),
            action: Action::Delete,
            items: without_nested(recent_large_builds),
            total: None,
        });
    }
    // The Recycle Bin itself.
    if let Some(bin) = recycle_bin_bytes().filter(|b| *b > 0) {
        out.push(Suggestion {
            title: "Recycle Bin".into(),
            detail: "Everything already moved to the Recycle Bin on all drives".into(),
            note: None,
            action: Action::EmptyRecycleBin,
            items: vec![],
            total: Some(bin),
        });
    }
    out.sort_by_key(|s| std::cmp::Reverse(s.bytes()));
    let stopped = if cancel.load(Ordering::Relaxed) {
        Some("Stopped".into())
    } else if Instant::now() >= deadline {
        Some("Took too long; sizes may be partial".into())
    } else {
        None
    };
    Report {
        suggestions: out,
        elapsed: started.elapsed(),
        stopped,
    }
}

/// How far a deletion has got, read by the page while it runs.
#[derive(Default)]
pub struct Progress {
    /// Files found so far, and whether the listing is still going.
    pub listed: std::sync::atomic::AtomicU64,
    pub listing: AtomicBool,
    /// Files deleted, their bytes, and files left because they're in use or
    /// can't be removed.
    pub files: std::sync::atomic::AtomicU64,
    pub bytes: std::sync::atomic::AtomicU64,
    pub skipped: std::sync::atomic::AtomicU64,
}

/// Deletes each path in place: files at once, folders by deleting what they
/// hold (several at a time) and then the emptied folders, deepest first.
/// Links and junctions are removed themselves, never followed; a file in use
/// or protected is skipped and counted; `cancel` stops it between files.
pub fn delete(paths: &[PathBuf], cancel: &AtomicBool, progress: &Progress) {
    use rayon::prelude::*;
    use std::os::windows::fs::MetadataExt;
    use std::sync::atomic::Ordering::Relaxed;
    const REPARSE: u32 = 0x400;
    progress.listing.store(true, Relaxed);
    let mut files: Vec<(PathBuf, u64)> = vec![];
    let mut dirs: Vec<(PathBuf, usize)> = vec![];
    let mut links: Vec<PathBuf> = vec![];
    for root in paths {
        let Ok(meta) = std::fs::symlink_metadata(root) else { continue };
        if meta.file_attributes() & REPARSE != 0 {
            links.push(root.clone());
            continue;
        }
        if !meta.is_dir() {
            files.push((root.clone(), meta.len()));
            continue;
        }
        let mut stack = vec![(root.clone(), 0usize)];
        while let Some((dir, depth)) = stack.pop() {
            if cancel.load(Relaxed) {
                progress.listing.store(false, Relaxed);
                return;
            }
            dirs.push((dir.clone(), depth));
            let Ok(entries) = std::fs::read_dir(&dir) else { continue };
            for entry in entries.flatten() {
                let Ok(meta) = entry.metadata() else { continue };
                let path = entry.path();
                if meta.file_attributes() & REPARSE != 0 {
                    links.push(path);
                } else if meta.is_dir() {
                    stack.push((path, depth + 1));
                } else {
                    files.push((path, meta.len()));
                    progress.listed.fetch_add(1, Relaxed);
                }
            }
        }
    }
    progress.listed.store(files.len() as u64, Relaxed);
    progress.listing.store(false, Relaxed);
    let remove_file = |path: &Path| -> bool {
        if std::fs::remove_file(path).is_ok() {
            return true;
        }
        // A read-only file: clear the flag and try once more.
        std::fs::metadata(path)
            .map(|m| {
                let mut permissions = m.permissions();
                #[allow(clippy::permissions_set_readonly_false)]
                permissions.set_readonly(false);
                std::fs::set_permissions(path, permissions).is_ok() && std::fs::remove_file(path).is_ok()
            })
            .unwrap_or(false)
    };
    let work = || {
        files.par_iter().for_each(|(path, bytes)| {
            if cancel.load(Relaxed) {
                return;
            }
            if remove_file(path) {
                progress.files.fetch_add(1, Relaxed);
                progress.bytes.fetch_add(*bytes, Relaxed);
            } else {
                progress.skipped.fetch_add(1, Relaxed);
            }
        });
    };
    // Bound concurrency and use low background CPU/I/O QoS so deleting
    // hundreds of thousands of cache files stays fast without lagging the PC.
    let workers = std::thread::available_parallelism()
        .map_or(2, |n| (n.get() / 2).clamp(2, 4));
    match rayon::ThreadPoolBuilder::new()
        .num_threads(workers)
        .start_handler(|_| seek_engine::qos::utility())
        .build()
    {
        Ok(pool) => pool.install(work),
        Err(_) => work(),
    }
    // A link or junction is removed as itself; what it points to is never
    // touched.
    for link in &links {
        if cancel.load(Relaxed) {
            break;
        }
        let _ = std::fs::remove_dir(link).or_else(|_| std::fs::remove_file(link));
    }
    // Emptied folders, deepest first; one still holding a skipped file stays.
    dirs.sort_by_key(|(_, depth)| std::cmp::Reverse(*depth));
    for (dir, _) in dirs {
        if cancel.load(Relaxed) {
            break;
        }
        let _ = std::fs::remove_dir(&dir);
    }
}

/// Ensures two Recycle Bin empty calls can never collide and spawn Windows
/// "Item Not Found" error dialogs.
static RECYCLE_EMPTYING: AtomicBool = AtomicBool::new(false);

/// Sums the exact original byte sizes recorded in `$I*` Recycle Bin metadata
/// files across fixed/removable drives so newly recycled items show up
/// immediately even before `SHQueryRecycleBinW` refreshes its shell cache.
fn recycle_bin_metadata_bytes() -> u64 {
    use windows_sys::Win32::Storage::FileSystem::{GetDriveTypeW, GetLogicalDrives};
    let Ok(sid) = seek_engine::helper::user_sid() else {
        return 0;
    };
    let mask = unsafe { GetLogicalDrives() };
    let mut total = 0u64;
    for i in 0..26u8 {
        if mask & (1 << i) == 0 {
            continue;
        }
        let root = format!("{}:\\", (b'A' + i) as char);
        let wide: Vec<u16> = root.encode_utf16().chain(Some(0)).collect();
        if !matches!(unsafe { GetDriveTypeW(wide.as_ptr()) }, 2 | 3) {
            continue;
        }
        let bin = PathBuf::from(&root).join("$Recycle.Bin").join(&sid);
        let Ok(entries) = std::fs::read_dir(&bin) else {
            continue;
        };
        for entry in entries.flatten() {
            let name = entry.file_name();
            let lossy = name.to_string_lossy();
            if !lossy.starts_with("$I") && !lossy.starts_with("$i") {
                continue;
            }
            if let Ok(mut file) = std::fs::File::open(entry.path()) {
                let mut hdr = [0u8; 16];
                if std::io::Read::read_exact(&mut file, &mut hdr).is_ok() {
                    let version = u64::from_le_bytes(hdr[0..8].try_into().unwrap());
                    if version == 1 || version == 2 {
                        let size = u64::from_le_bytes(hdr[8..16].try_into().unwrap());
                        total = total.saturating_add(size);
                    }
                }
            }
        }
    }
    total
}

/// Bytes in the Recycle Bin on all drives.
pub fn recycle_bin_bytes() -> Option<u64> {
    use windows::Win32::UI::Shell::{SHQUERYRBINFO, SHQueryRecycleBinW};
    use windows_sys::Win32::Storage::FileSystem::{GetDriveTypeW, GetLogicalDrives};
    let mask = unsafe { GetLogicalDrives() };
    let mut drive_total = 0u64;
    let mut any_ok = false;
    for i in 0..26u8 {
        if mask & (1 << i) == 0 {
            continue;
        }
        let root = format!("{}:\\", (b'A' + i) as char);
        let wide: Vec<u16> = root.encode_utf16().chain(Some(0)).collect();
        if !matches!(unsafe { GetDriveTypeW(wide.as_ptr()) }, 2 | 3) {
            continue;
        }
        let mut info = SHQUERYRBINFO {
            cbSize: std::mem::size_of::<SHQUERYRBINFO>() as u32,
            ..Default::default()
        };
        if unsafe { SHQueryRecycleBinW(windows::core::PCWSTR(wide.as_ptr()), &mut info) }.is_ok() {
            any_ok = true;
            drive_total = drive_total.saturating_add(info.i64Size.max(0) as u64);
        }
    }
    let meta_total = recycle_bin_metadata_bytes();
    if any_ok {
        return Some(drive_total.max(meta_total));
    }
    let mut info = SHQUERYRBINFO {
        cbSize: std::mem::size_of::<SHQUERYRBINFO>() as u32,
        ..Default::default()
    };
    if unsafe { SHQueryRecycleBinW(windows::core::PCWSTR::null(), &mut info) }.is_ok() {
        Some((info.i64Size.max(0) as u64).max(meta_total))
    } else if meta_total > 0 {
        Some(meta_total)
    } else {
        None
    }
}

/// Top-level items inside the current user's `$Recycle.Bin\<SID>` folders across
/// all fixed and removable drives, never including `desktop.ini`.
fn user_recycle_bin_items() -> Vec<PathBuf> {
    use windows_sys::Win32::Storage::FileSystem::{GetDriveTypeW, GetLogicalDrives};
    let Ok(sid) = seek_engine::helper::user_sid() else {
        return vec![];
    };
    let mask = unsafe { GetLogicalDrives() };
    let mut items = vec![];
    for i in 0..26u8 {
        if mask & (1 << i) == 0 {
            continue;
        }
        let root = format!("{}:\\", (b'A' + i) as char);
        let wide: Vec<u16> = root.encode_utf16().chain(Some(0)).collect();
        if !matches!(unsafe { GetDriveTypeW(wide.as_ptr()) }, 2 | 3) {
            continue;
        }
        let bin = PathBuf::from(&root).join("$Recycle.Bin").join(&sid);
        let Ok(entries) = std::fs::read_dir(&bin) else {
            continue;
        };
        for entry in entries.flatten() {
            let name = entry.file_name();
            if name.eq_ignore_ascii_case("desktop.ini") {
                continue;
            }
            items.push(entry.path());
        }
    }
    items
}

/// Notifies Windows Shell that the Recycle Bin changed so the Desktop icon
/// and Explorer views refresh immediately without any Shell dialogs.
fn notify_recycle_bin_changed() {
    use windows::Win32::{
        System::Com::CoTaskMemFree,
        UI::Shell::{
            FOLDERID_RecycleBinFolder, KF_FLAG_DEFAULT, SHCNF_IDLIST, SHCNE_UPDATEDIR,
            SHCNE_UPDATEITEM, SHChangeNotify, SHGetKnownFolderIDList,
        },
    };
    unsafe {
        if let Ok(pidl) =
            SHGetKnownFolderIDList(&FOLDERID_RecycleBinFolder, KF_FLAG_DEFAULT.0 as u32, None)
        {
            SHChangeNotify(SHCNE_UPDATEDIR, SHCNF_IDLIST, Some(pidl.cast()), None);
            SHChangeNotify(SHCNE_UPDATEITEM, SHCNF_IDLIST, Some(pidl.cast()), None);
            CoTaskMemFree(Some(pidl.cast()));
        }
    }
}

/// Empties the Recycle Bin on all drives with live progress, low background
/// I/O priority, single-flight protection, and no Windows Shell popup dialogs.
/// Permanent: only call after explicit confirmation.
pub fn empty_recycle_bin_with_progress(
    cancel: &AtomicBool,
    progress: &Progress,
) -> Result<(), String> {
    use windows::Win32::UI::Shell::{
        SHERB_NOCONFIRMATION, SHERB_NOPROGRESSUI, SHERB_NOSOUND, SHEmptyRecycleBinW,
    };
    if RECYCLE_EMPTYING.swap(true, Ordering::AcqRel) {
        return Ok(());
    }
    struct Guard;
    impl Drop for Guard {
        fn drop(&mut self) {
            RECYCLE_EMPTYING.store(false, Ordering::Release);
        }
    }
    let _guard = Guard;

    // Fast, silent, non-blocking parallel removal of the current user's own
    // `$Recycle.Bin\<SID>` entries (skipping `desktop.ini`, never following
    // reparse points, and never showing Windows "Item Not Found" popups).
    let items = user_recycle_bin_items();
    if !items.is_empty() {
        delete(&items, cancel, progress);
    }
    notify_recycle_bin_changed();
    if cancel.load(Ordering::Relaxed) {
        return Ok(());
    }
    // Only fall back to Shell if something remains on a drive we couldn't
    // address directly (e.g. a redirected/mounted volume), and ignore the
    // benign E_UNEXPECTED Shell returns when the bin is already empty.
    if recycle_bin_bytes().unwrap_or(0) > 0 {
        let res = unsafe {
            SHEmptyRecycleBinW(
                None,
                windows::core::PCWSTR::null(),
                SHERB_NOCONFIRMATION | SHERB_NOPROGRESSUI | SHERB_NOSOUND,
            )
        };
        notify_recycle_bin_changed();
        if let Err(err) = res
            && recycle_bin_bytes().unwrap_or(0) > 0
        {
            return Err(err.message().to_string());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set_mtime(path: &Path, age: Duration) {
        let file = std::fs::OpenOptions::new()
            .write(true)
            .open(path)
            .or_else(|_| {
                use std::os::windows::fs::OpenOptionsExt;
                std::fs::OpenOptions::new()
                    .write(true)
                    .custom_flags(0x0200_0000) // FILE_FLAG_BACKUP_SEMANTICS for folders
                    .open(path)
            })
            .unwrap();
        file.set_modified(SystemTime::now() - age).unwrap();
    }

    #[test]
    fn only_build_folders_of_long_untouched_repositories_are_offered() {
        let dir = tempfile::tempdir().unwrap();
        let old = dir.path().join("old-app");
        let fresh = dir.path().join("fresh-app");
        let plain = dir.path().join("not-a-repo");
        for repo in [&old, &fresh] {
            std::fs::create_dir_all(repo.join(".git")).unwrap();
            std::fs::write(repo.join(".git").join("HEAD"), "ref").unwrap();
            std::fs::create_dir_all(repo.join("node_modules").join("x").join("node_modules"))
                .unwrap();
        }
        std::fs::create_dir_all(plain.join("target")).unwrap();
        // A `build` folder that holds source (not in .gitignore) stays; one
        // the project ignores is offered.
        std::fs::create_dir_all(old.join("build")).unwrap();
        std::fs::create_dir_all(old.join("dist")).unwrap();
        std::fs::write(old.join(".gitignore"), "# output\n/dist/\n").unwrap();
        let ancient = Duration::from_secs(200 * 24 * 3600);
        for path in [
            old.join(".git").join("HEAD"),
            old.join(".git"),
            old.clone(),
        ] {
            set_mtime(&path, ancient);
        }
        let now = SystemTime::now();
        assert!(stale_build_folder(&old.join("node_modules"), now));
        assert!(stale_build_folder(&old.join("dist"), now), "ignored by .gitignore");
        assert!(!stale_build_folder(&old.join("build"), now), "tracked: may hold source");
        assert!(!stale_build_folder(&fresh.join("node_modules"), now), "touched recently");
        assert!(!stale_build_folder(&plain.join("target"), now), "not a repository");
        assert!(
            !stale_build_folder(&old.join("node_modules").join("x").join("node_modules"), now),
            "nested inside another build folder"
        );
    }

    #[test]
    fn spots_stay_inside_the_profile() {
        for spot in SPOTS {
            let path = Path::new(spot.rel);
            assert!(path.is_relative(), "{}", spot.rel);
            assert!(!spot.rel.contains(".."), "{}", spot.rel);
            // Only the person's own error reports, never Windows itself.
            let lower = spot.rel.to_ascii_lowercase();
            assert!(!lower.starts_with("windows"), "{}", spot.rel);
        }
        assert!(recycle_bin_bytes().is_some());
    }

    #[test]
    fn a_look_finds_caches_in_a_profile_and_lists_profiles_separately() {
        let home = tempfile::tempdir().unwrap();
        let home = home.path();
        let write = |rel: &str, bytes: usize| {
            let path = home.join(rel);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, vec![7u8; bytes]).unwrap();
        };
        write(r"AppData\Local\npm-cache\_cacache\a.bin", 2 << 20);
        write(r"AppData\Local\Google\Chrome\User Data\Default\Cache\Cache_Data\f", 2 << 20);
        write(r"AppData\Local\Google\Chrome\User Data\Profile 1\Cache\Cache_Data\f", 2 << 20);
        // Too small to bother with.
        write(r"AppData\Local\D3DSCache\x", 10);
        // Downloaded models go; the sign-in token beside them never does.
        write(r".cache\huggingface\hub\models--x\blob", 64 << 10);
        write(r".cache\huggingface\token", 40);
        let report = suggest(None, home, &[], &AtomicBool::new(false), |_| {});
        let titles: Vec<_> = report.suggestions.iter().map(|s| s.title.as_str()).collect();
        assert!(titles.contains(&"Developer caches"), "{titles:?}");
        assert!(!titles.contains(&"Graphics shader caches"), "{titles:?}");
        let browsers = report
            .suggestions
            .iter()
            .find(|s| s.title == "Browser caches")
            .unwrap();
        assert_eq!(browsers.items.len(), 2, "{:?}", browsers.items);
        assert!(browsers.note.is_some());
        assert!(browsers.bytes() >= 4 << 20);
        let all: Vec<&PathBuf> = report.suggestions.iter().flat_map(|s| s.items.iter().map(|(p, _)| p)).collect();
        assert!(all.iter().any(|p| p.ends_with(r".cache\huggingface\hub")), "{all:?}");
        assert!(
            !all.iter().any(|p| home.join(r".cache\huggingface\token").starts_with(p)),
            "the token must never be offered: {all:?}"
        );
    }

    /// Read-only: prints what a look finds on this PC.
    #[test]
    #[ignore]
    fn look_at_this_pc() {
        let home = PathBuf::from(std::env::var_os("USERPROFILE").unwrap());
        let repos: Vec<PathBuf> = crate::worktrees::find_on_disk(&home, &AtomicBool::new(false))
            .into_iter()
            .filter_map(|g| g.parent().map(Path::to_path_buf))
            .collect();
        let report = suggest(None, &home, &repos, &AtomicBool::new(false), |_| {});
        println!("{} repositories; {:.1} s; stopped {:?}", repos.len(), report.elapsed.as_secs_f32(), report.stopped);
        for s in &report.suggestions {
            println!("{:>10} MB  {} ({} items)", s.bytes() >> 20, s.title, s.items.len());
            if std::env::var_os("SEEK_LIST").is_some() {
                for (p, b) in &s.items {
                    println!("              {:>6} MB  {}", b >> 20, p.strip_prefix(&home).unwrap_or(p).display());
                }
            }
        }
    }

    #[test]
    fn delete_removes_folders_and_never_follows_links() {
        let dir = tempfile::tempdir().unwrap();
        let cache = dir.path().join("cache");
        let outside = dir.path().join("outside");
        std::fs::create_dir_all(cache.join("a").join("b")).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::write(outside.join("keep.txt"), b"keep").unwrap();
        for i in 0..50 {
            std::fs::write(cache.join("a").join("b").join(format!("{i}.bin")), vec![1u8; 100]).unwrap();
        }
        std::fs::write(cache.join("ro.bin"), b"x").unwrap();
        let mut permissions = std::fs::metadata(cache.join("ro.bin")).unwrap().permissions();
        permissions.set_readonly(true);
        std::fs::set_permissions(cache.join("ro.bin"), permissions).unwrap();
        // A junction inside the cache to a folder outside it.
        let junction = cache.join("link");
        let made = std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(&junction)
            .arg(&outside)
            .output()
            .is_ok_and(|o| o.status.success());
        let progress = Progress::default();
        delete(std::slice::from_ref(&cache), &AtomicBool::new(false), &progress);
        assert!(!cache.exists(), "the cache folder is gone");
        assert!(outside.join("keep.txt").exists(), "a junction's target is never touched (junction made: {made})");
        assert_eq!(progress.files.load(Ordering::Relaxed), 51);
        assert_eq!(progress.bytes.load(Ordering::Relaxed), 5001);
        assert_eq!(progress.skipped.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn nested_items_count_once() {
        let items = vec![
            (PathBuf::from(r"C:\a\b"), 5),
            (PathBuf::from(r"C:\a"), 9),
            (PathBuf::from(r"C:\c"), 1),
        ];
        let kept = without_nested(items);
        assert_eq!(kept, vec![(PathBuf::from(r"C:\a"), 9), (PathBuf::from(r"C:\c"), 1)]);
    }
}
