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
    /// Move the ticked items to the Recycle Bin.
    Recycle,
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
/// A repository untouched this long has its build folders offered.
pub const STALE_REPO: Duration = Duration::from_secs(90 * 24 * 3600);
const BUILD_FOLDERS: [&str; 6] = ["node_modules", "target", ".venv", "dist", "build", ".next"];
/// Items smaller than this aren't worth a line.
const MIN_ITEM: u64 = 4 << 10;
/// How long a whole look may take, and how many entries one folder walk reads.
const DEADLINE: Duration = Duration::from_secs(25);
const WALK_ENTRIES: usize = 400_000;
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
    spot(Developer, Local, r"pip\Cache"),
    spot(Developer, Local, r"uv\cache"),
    spot(Developer, Local, r"pypoetry\Cache"),
    spot(Developer, Home, r".gradle\caches"),
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
    // Downloaded models: large, and slow to download again.
    // The downloaded models only: .cache\huggingface also keeps the sign-in
    // token, which must never go.
    spot(Models, Home, r".cache\huggingface\hub"),
    spot(Models, Home, r".cache\huggingface\xet"),
    spot(Models, Home, r".cache\torch"),
    // Editors and chat apps rebuild these on the next start.
    spot(Apps, Roaming, r"Code\Cache"),
    spot(Apps, Roaming, r"Code\CachedData"),
    spot(Apps, Roaming, r"Code\CachedExtensionVSIXs"),
    spot(Apps, Roaming, r"Code\Code Cache"),
    spot(Apps, Roaming, r"Code\GPUCache"),
    spot(Apps, Roaming, r"Cursor\Cache"),
    spot(Apps, Roaming, r"Cursor\CachedData"),
    spot(Apps, Roaming, r"Cursor\Code Cache"),
    spot(Apps, Roaming, r"discord\Cache"),
    spot(Apps, Roaming, r"discord\Code Cache"),
    spot(Apps, Roaming, r"Slack\Cache"),
    spot(Apps, Roaming, r"Slack\Code Cache"),
    spot(Apps, Roaming, r"Microsoft\Teams\Cache"),
    spot(Apps, Local, r"Spotify\Data"),
    spot(Apps, Local, r"Packages\*\LocalCache\Microsoft\MSTeams\EBWebView\*\Cache"),
    spot(Apps, Local, r"Packages\*\LocalCache\Microsoft\MSTeams\EBWebView\*\Code Cache"),
    // Store apps' web caches and Windows' own internet cache.
    spot(Apps, Local, r"Packages\*\AC\INetCache"),
    spot(Apps, Local, r"Microsoft\Windows\INetCache"),
    spot(Apps, Local, r"Microsoft\Windows\SharedWebView\EBWebView\*\Cache"),
    spot(Apps, Local, r"Microsoft\Windows\SharedWebView\EBWebView\*\Code Cache"),
    spot(Apps, Local, r"NVIDIA Corporation\*\CefCache"),
    // Old update packages an app already installed.
    spot(Leftovers, Roaming, r"Telegram Desktop\tupdates"),
    // Browser caches, per profile.
    spot(Browsers, Local, r"Google\Chrome\User Data\*\Cache"),
    spot(Browsers, Local, r"Google\Chrome\User Data\*\Code Cache"),
    spot(Browsers, Local, r"Microsoft\Edge\User Data\*\Cache"),
    spot(Browsers, Local, r"Microsoft\Edge\User Data\*\Code Cache"),
    spot(Browsers, Local, r"BraveSoftware\Brave-Browser\User Data\*\Cache"),
    spot(Browsers, Local, r"BraveSoftware\Brave-Browser\User Data\*\Code Cache"),
    spot(Browsers, Local, r"Mozilla\Firefox\Profiles\*\cache2"),
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
    // What installers and updaters leave behind.
    spot(Leftovers, Local, r"SquirrelTemp"),
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

/// Browser and app caches found by shape rather than by name, so any
/// Chromium browser or Electron/WebView2 app is covered: a folder holding
/// `Code Cache` or `GPUCache` is a profile, and a folder holding a
/// `Local State` file is a Chromium browser. A folder merely called
/// "Cache" in some other app is never offered. Each path comes with
/// whether it belongs to a browser.
fn profile_caches(home: &Path) -> Vec<(PathBuf, bool)> {
    let mut out = vec![];
    let mut stack: Vec<(PathBuf, usize)> = ["Local", "LocalLow", "Roaming"]
        .iter()
        .map(|b| (home.join("AppData").join(b), 0))
        .collect();
    let deadline = Instant::now() + Duration::from_secs(4);
    while let Some((dir, depth)) = stack.pop() {
        if Instant::now() >= deadline {
            break;
        }
        let browser = dir
            .ancestors()
            .any(|a| a.file_name().is_some_and(|n| n.eq_ignore_ascii_case("User Data")))
            || dir.to_string_lossy().contains("Opera Software");
        if dir.join("Local State").is_file() {
            out.extend(
                BROWSER_CACHES
                    .iter()
                    .map(|c| dir.join(c))
                    .filter(|p| is_plain_dir(p))
                    .map(|p| (p, true)),
            );
        }
        if depth > 0 && (is_plain_dir(&dir.join("Code Cache")) || is_plain_dir(&dir.join("GPUCache"))) {
            out.extend(
                PROFILE_CACHES
                    .iter()
                    .map(|c| dir.join(c))
                    .filter(|p| is_plain_dir(p))
                    .map(|p| (p, browser)),
            );
            continue;
        }
        if depth >= 5 {
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

/// Folders that only ever hold what a tool generated. `target`, `dist` and
/// `build` are also ordinary folder names some projects keep source in, so
/// those count only when the repository's .gitignore ignores them.
const ALWAYS_GENERATED: [&str; 3] = ["node_modules", ".venv", ".next"];

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
/// alone long enough, is generated (see ALWAYS_GENERATED), and is not
/// itself inside another build folder.
pub fn stale_build_folder(folder: &Path, now: SystemTime) -> bool {
    let Some(repo) = folder.parent() else {
        return false;
    };
    let Some(name) = folder.file_name().map(|n| n.to_string_lossy().to_string()) else {
        return false;
    };
    let generated = ALWAYS_GENERATED.iter().any(|g| name.eq_ignore_ascii_case(g)) || ignored_by_git(repo, &name);
    if !generated {
        return false;
    }
    let nested = folder.ancestors().skip(1).any(|a| {
        a.file_name()
            .is_some_and(|n| BUILD_FOLDERS.iter().any(|b| n.eq_ignore_ascii_case(b)))
    });
    !nested
        && std::fs::symlink_metadata(repo.join(".git")).is_ok()
        && repo_touched(repo).is_some_and(|t| now.duration_since(t).is_ok_and(|d| d >= STALE_REPO))
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
    let indexed = engine.map_or(0, |e| e.storage(path, 4096, deadline, cancel).bytes);
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
    spots.extend(
        profile_caches(home)
            .into_iter()
            .map(|(p, browser)| (if browser { Browsers } else { Apps }, p)),
    );
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
                action: Action::Recycle,
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
            let bytes = size(engine, &path, deadline, cancel);
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
            action: Action::Recycle,
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
    // Build folders in repositories left alone for months.
    progress("Old projects");
    let mut builds = vec![];
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
            if is_plain_dir(&folder) && stale_build_folder(&folder, now) {
                let bytes = size(engine, &folder, deadline, cancel);
                if bytes >= MIN_ITEM {
                    builds.push((folder, bytes));
                }
            }
        }
    }
    if !builds.is_empty() {
        out.push(Suggestion {
            title: "Build folders in old projects".into(),
            detail: "node_modules, target, .venv, dist, build and .next in repositories untouched for 90 days".into(),
            note: Some("Rebuilt the next time you build the project".into()),
            action: Action::Recycle,
            items: without_nested(builds),
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

/// Bytes in the Recycle Bin on all drives.
pub fn recycle_bin_bytes() -> Option<u64> {
    use windows::Win32::UI::Shell::{SHQUERYRBINFO, SHQueryRecycleBinW};
    let mut info = SHQUERYRBINFO {
        cbSize: std::mem::size_of::<SHQUERYRBINFO>() as u32,
        ..Default::default()
    };
    unsafe { SHQueryRecycleBinW(windows::core::PCWSTR::null(), &mut info) }.ok()?;
    Some(info.i64Size.max(0) as u64)
}

/// Empties the Recycle Bin on all drives. Permanent: only after the person
/// confirmed it in Seek's own dialog.
pub fn empty_recycle_bin() -> Result<(), String> {
    use windows::Win32::UI::Shell::{
        SHERB_NOCONFIRMATION, SHERB_NOPROGRESSUI, SHERB_NOSOUND, SHEmptyRecycleBinW,
    };
    unsafe {
        SHEmptyRecycleBinW(
            None,
            windows::core::PCWSTR::null(),
            SHERB_NOCONFIRMATION | SHERB_NOPROGRESSUI | SHERB_NOSOUND,
        )
    }
    .map_err(|e| e.message().to_string())
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
