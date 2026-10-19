//! One-shot, worker-only GitHub update checks and bounded package staging.
//! WinHTTP uses system TLS validation. Redirects are followed explicitly only
//! to GitHub's asset hosts: https://learn.microsoft.com/windows/win32/winhttp/option-flags
use sha2::{Digest, Sha256};
use std::{
    ffi::c_void,
    io::{Cursor, Read},
    path::{Path, PathBuf},
    ptr,
    time::{Duration, Instant},
};

const REPO: &str = "https://github.com/sanirudh17/Seek";
const API: &str = "https://api.github.com/repos/sanirudh17/Seek/releases/latest";
pub const CURRENT: &str = env!("CARGO_PKG_VERSION");
pub const FILES: &[&str] = &[
    "seek.exe",
    "seek-helper.exe",
    "seek.ico",
    "README.md",
    "LICENSE",
    "THIRD-PARTY-NOTICES.txt",
    "Install.ps1",
    "Install.cmd",
    "Uninstall.ps1",
    "Uninstall.cmd",
    "Update.ps1",
];

#[derive(Clone, Debug)]
pub struct Release {
    pub version: String,
    tag: String,
}
pub enum Check {
    Current,
    NoRelease,
    Available(Release),
}
pub struct State {
    pub busy: bool,
    pub available: Option<Release>,
    pub message: String,
}
impl Default for State {
    fn default() -> Self {
        Self {
            busy: false,
            available: None,
            message: "Check for a newer version on GitHub.".into(),
        }
    }
}
fn version(value: &str) -> Option<[u32; 3]> {
    let parts: Vec<_> = value
        .strip_prefix('v')
        .unwrap_or(value)
        .split('.')
        .collect();
    if parts.len() != 3
        || parts
            .iter()
            .any(|p| p.is_empty() || !p.bytes().all(|b| b.is_ascii_digit()))
    {
        return None;
    }
    Some([
        parts[0].parse().ok()?,
        parts[1].parse().ok()?,
        parts[2].parse().ok()?,
    ])
}
impl Release {
    fn filename(&self) -> String {
        format!("Seek-{}-windows-x64.zip", self.version)
    }
    fn url(&self, name: &str) -> String {
        format!("{REPO}/releases/download/{}/{name}", self.tag)
    }
}
fn release(bytes: &[u8]) -> Result<Check, String> {
    let value: serde_json::Value =
        serde_json::from_slice(bytes).map_err(|_| "GitHub returned an invalid release response")?;
    if value["draft"].as_bool() != Some(false) || value["prerelease"].as_bool() != Some(false) {
        return Err("No stable release was returned".into());
    }
    let tag = value["tag_name"]
        .as_str()
        .ok_or("Release version is missing")?;
    let number = version(tag).ok_or("Unsupported release version")?;
    if number <= version(CURRENT).unwrap() {
        return Ok(Check::Current);
    }
    let info = Release {
        version: tag.trim_start_matches('v').into(),
        tag: tag.into(),
    };
    let assets = value["assets"]
        .as_array()
        .ok_or("Release assets are missing")?;
    for name in [info.filename(), "SHA256SUMS.txt".into()] {
        if !assets.iter().any(|a| {
            a["name"].as_str() == Some(&name)
                && a["browser_download_url"].as_str() == Some(&info.url(&name))
        }) {
            return Err("The release is missing its Windows package or checksum".into());
        }
    }
    Ok(Check::Available(info))
}
pub fn check() -> Result<Check, String> {
    let (status, bytes) = get(API, 1024 * 1024)?;
    if status == 404 {
        return Ok(Check::NoRelease);
    }
    if status != 200 {
        return Err(format!(
            "GitHub update check returned HTTP {status}. Try again later."
        ));
    }
    release(&bytes)
}

fn checksum(bytes: &[u8], sums: &[u8], name: &str) -> Result<(), String> {
    let sums = std::str::from_utf8(sums).map_err(|_| "Invalid checksum file")?;
    let lines: Vec<_> = sums
        .lines()
        .filter_map(|line| {
            let mut p = line.split_whitespace();
            let hash = p.next()?;
            let file = p.next()?.trim_start_matches('*');
            (file == name
                && hash.len() == 64
                && hash.bytes().all(|b| b.is_ascii_hexdigit())
                && p.next().is_none())
            .then_some(hash)
        })
        .collect();
    if lines.len() != 1 || !format!("{:x}", Sha256::digest(bytes)).eq_ignore_ascii_case(lines[0]) {
        return Err("Update checksum did not match. Nothing was installed.".into());
    }
    Ok(())
}
fn extract(bytes: Vec<u8>, folder: &Path, release: &Release) -> Result<(), String> {
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).map_err(|e| e.to_string())?;
    let prefix = format!("Seek-{}-windows-x64/", release.version);
    let mut found = std::collections::HashSet::new();
    let mut total = 0u64;
    for i in 0..archive.len() {
        let entry = archive.by_index(i).map_err(|e| e.to_string())?;
        if entry.is_dir() && entry.name() == prefix {
            continue;
        }
        let name = entry
            .name()
            .strip_prefix(&prefix)
            .ok_or("Unexpected update folder")?
            .to_owned();
        if !FILES.contains(&name.as_str())
            || !found.insert(name.clone())
            || entry.unix_mode().is_some_and(|m| m & 0o170000 == 0o120000)
        {
            return Err("Unexpected or duplicate file in update package".into());
        }
        let declared = entry.size();
        total = total
            .checked_add(declared)
            .ok_or("Update package too large")?;
        if entry.size() > 48 * 1024 * 1024 || total > 128 * 1024 * 1024 {
            return Err("Update package too large".into());
        }
        let mut content = Vec::new();
        entry
            .take(48 * 1024 * 1024 + 1)
            .read_to_end(&mut content)
            .map_err(|e| e.to_string())?;
        if content.len() > 48 * 1024 * 1024 {
            return Err("Update entry exceeds its size limit".into());
        }
        total = total - declared + content.len() as u64;
        if total > 128 * 1024 * 1024 {
            return Err("Update package expands beyond its size limit".into());
        }
        std::fs::write(folder.join(name), content).map_err(|e| e.to_string())?;
    }
    if found.len() != FILES.len() {
        return Err("Incomplete update package".into());
    }
    Ok(())
}
pub fn prepare(release: Release) -> Result<PathBuf, String> {
    let name = release.filename();
    let (status, sums) = get(&release.url("SHA256SUMS.txt"), 64 * 1024)?;
    if status != 200 {
        return Err(format!("Cannot download checksum: HTTP {status}"));
    }
    let (status, bytes) = get(&release.url(&name), 64 * 1024 * 1024)?;
    if status != 200 {
        return Err(format!("Cannot download update: HTTP {status}"));
    }
    checksum(&bytes, &sums, &name)?;
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_nanos();
    let stage = std::env::temp_dir().join(format!("Seek-update-{}-{stamp}", std::process::id()));
    std::fs::create_dir(&stage).map_err(|e| e.to_string())?;
    let payload = stage.join("payload");
    let result = (|| {
        std::fs::create_dir(&payload).map_err(|e| e.to_string())?;
        extract(bytes, &payload, &release)?;
        let args: Vec<_> = std::env::args().collect();
        let option = |key: &str| args.windows(2).find(|a| a[0] == key).map(|a| a[1].clone());
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        let mut hashes = serde_json::Map::new();
        for name in FILES {
            let bytes = std::fs::read(payload.join(name)).map_err(|e| e.to_string())?;
            hashes.insert(
                (*name).into(),
                format!("{:x}", Sha256::digest(bytes)).into(),
            );
        }
        let request = serde_json::json!({
            "product":"Seek", "target":exe.parent().ok_or("No app directory")?, "stage":stage,
            "parent_pid":std::process::id(), "version":release.version,
            "data":option("--data").map(PathBuf::from).unwrap_or_else(||PathBuf::from(std::env::var_os("LOCALAPPDATA").unwrap_or_default()).join("Seek")),
            "root":option("--root"), "walker":args.iter().any(|a|a=="--walker"), "hashes":hashes
        });
        std::fs::write(
            stage.join("request.json"),
            serde_json::to_vec(&request).map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;
        Ok(stage.clone())
    })();
    if result.is_err() {
        let _ = std::fs::remove_dir_all(&stage);
    }
    result
}
pub fn launch(stage: &Path) -> Result<(), String> {
    use std::os::windows::process::CommandExt;
    let powershell =
        PathBuf::from(std::env::var_os("SystemRoot").ok_or("Windows path is missing")?)
            .join("System32/WindowsPowerShell/v1.0/powershell.exe");
    let mut child = std::process::Command::new(powershell)
        .args([
            "-NoLogo",
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-WindowStyle",
            "Hidden",
            "-File",
        ])
        .arg(stage.join("payload/Update.ps1"))
        .arg("-Request")
        .arg(stage.join("request.json"))
        .creation_flags(0x08000000)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|e| e.to_string())?;
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(10) {
        if stage.join("ready").is_file() {
            return Ok(());
        }
        if let Some(code) = child.try_wait().map_err(|e| e.to_string())? {
            return Err(format!(
                "Update preparation exited ({code}). Seek remains open. Details: {}",
                stage.join("update-result.json").display()
            ));
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    let _ = child.kill();
    let _ = child.wait();
    Err("Update preparation timed out. Seek remains open.".into())
}

// Keep this narrow system API binding independent of the pinned framework's
// Windows feature graph. Handles stay on the calling worker and close on exit.
#[link(name = "winhttp")]
unsafe extern "system" {
    fn WinHttpOpen(
        agent: *const u16,
        access: u32,
        proxy: *const u16,
        bypass: *const u16,
        flags: u32,
    ) -> *mut c_void;
    fn WinHttpCloseHandle(handle: *mut c_void) -> i32;
    fn WinHttpSetTimeouts(
        handle: *mut c_void,
        resolve: i32,
        connect: i32,
        send: i32,
        receive: i32,
    ) -> i32;
    fn WinHttpSetOption(handle: *mut c_void, option: u32, value: *const c_void, len: u32) -> i32;
    fn WinHttpConnect(
        session: *mut c_void,
        host: *const u16,
        port: u16,
        reserved: u32,
    ) -> *mut c_void;
    fn WinHttpOpenRequest(
        connection: *mut c_void,
        verb: *const u16,
        path: *const u16,
        version: *const u16,
        referer: *const u16,
        accept: *const *const u16,
        flags: u32,
    ) -> *mut c_void;
    fn WinHttpSendRequest(
        request: *mut c_void,
        headers: *const u16,
        headers_len: u32,
        optional: *const c_void,
        optional_len: u32,
        total: u32,
        context: usize,
    ) -> i32;
    fn WinHttpReceiveResponse(request: *mut c_void, reserved: *mut c_void) -> i32;
    fn WinHttpQueryHeaders(
        request: *mut c_void,
        info: u32,
        name: *const u16,
        buffer: *mut c_void,
        len: *mut u32,
        index: *mut u32,
    ) -> i32;
    fn WinHttpReadData(request: *mut c_void, buffer: *mut c_void, len: u32, read: *mut u32) -> i32;
}
struct Handle(*mut c_void);
impl Drop for Handle {
    fn drop(&mut self) {
        unsafe {
            WinHttpCloseHandle(self.0);
        }
    }
}
fn handle(raw: *mut c_void) -> Result<Handle, String> {
    if raw.is_null() {
        Err(std::io::Error::last_os_error().to_string())
    } else {
        Ok(Handle(raw))
    }
}
fn checked(ok: i32) -> Result<(), String> {
    if ok == 0 {
        Err(std::io::Error::last_os_error().to_string())
    } else {
        Ok(())
    }
}
fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}
fn url_parts(url: &str) -> Result<(&str, &str), String> {
    let tail = url
        .strip_prefix("https://")
        .ok_or("Updates require HTTPS")?;
    let (host, path) = tail.split_once('/').ok_or("Invalid update URL")?;
    if ![
        "api.github.com",
        "github.com",
        "release-assets.githubusercontent.com",
        "objects.githubusercontent.com",
        "github-releases.githubusercontent.com",
    ]
    .contains(&host)
        || path.contains(['\r', '\n'])
    {
        return Err("Update redirected outside the allowed GitHub hosts".into());
    }
    Ok((host, &tail[host.len()..]))
}
fn get(url: &str, limit: usize) -> Result<(u32, Vec<u8>), String> {
    let session = handle(unsafe {
        WinHttpOpen(
            wide(&format!("Seek/{CURRENT}")).as_ptr(),
            4,
            ptr::null(),
            ptr::null(),
            0,
        )
    })?;
    checked(unsafe { WinHttpSetTimeouts(session.0, 5000, 5000, 5000, 15000) })?;
    let redirect_policy = 0u32; // WINHTTP_OPTION_REDIRECT_POLICY_NEVER
    checked(unsafe { WinHttpSetOption(session.0, 88, ptr::from_ref(&redirect_policy).cast(), 4) })?;
    let start = Instant::now();
    let mut url = url.to_owned();
    for _ in 0..6 {
        if start.elapsed() > Duration::from_secs(60) {
            return Err("Update request timed out".into());
        }
        let (host, path) = url_parts(&url)?;
        let connection = handle(unsafe { WinHttpConnect(session.0, wide(host).as_ptr(), 443, 0) })?;
        let request = handle(unsafe {
            WinHttpOpenRequest(
                connection.0,
                wide("GET").as_ptr(),
                wide(path).as_ptr(),
                ptr::null(),
                ptr::null(),
                ptr::null(),
                0x00800000,
            )
        })?;
        checked(unsafe { WinHttpSendRequest(request.0, ptr::null(), 0, ptr::null(), 0, 0, 0) })?;
        checked(unsafe { WinHttpReceiveResponse(request.0, ptr::null_mut()) })?;
        let mut status = 0u32;
        let mut len = 4;
        checked(unsafe {
            WinHttpQueryHeaders(
                request.0,
                19 | 0x20000000,
                ptr::null(),
                ptr::from_mut(&mut status).cast(),
                &mut len,
                ptr::null_mut(),
            )
        })?;
        if [301, 302, 303, 307, 308].contains(&status) {
            let mut location = [0u16; 8192];
            let mut len = 16384;
            checked(unsafe {
                WinHttpQueryHeaders(
                    request.0,
                    33,
                    ptr::null(),
                    location.as_mut_ptr().cast(),
                    &mut len,
                    ptr::null_mut(),
                )
            })?;
            url = String::from_utf16(&location[..(len as usize / 2)])
                .map_err(|_| "Invalid redirect")?
                .trim_end_matches('\0')
                .to_owned();
            continue;
        }
        if status != 200 {
            return Ok((status, vec![]));
        }
        let mut bytes = Vec::new();
        let mut buffer = [0u8; 16384];
        loop {
            if start.elapsed() > Duration::from_secs(60) {
                return Err("Update request timed out".into());
            }
            let mut read = 0;
            checked(unsafe {
                WinHttpReadData(
                    request.0,
                    buffer.as_mut_ptr().cast(),
                    buffer.len() as u32,
                    &mut read,
                )
            })?;
            if read == 0 {
                break;
            }
            if bytes.len() + read as usize > limit {
                return Err("Update response exceeds its size limit".into());
            }
            bytes.extend_from_slice(&buffer[..read as usize]);
        }
        return Ok((status, bytes));
    }
    Err("Too many update redirects".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn versions_and_redirects_reject_untrusted_input() {
        assert!(version("v1.1.0") > version("1.0.0"));
        for v in ["1.2", "1.2.3.4", "1.2.3-beta", "1.2.3/evil", "-1.2.3"] {
            assert!(version(v).is_none());
        }
        for u in [
            "http://github.com/x",
            "https://github.com.evil/x",
            "https://user@github.com/x",
            "https://github.com:443/x",
            "https://evil.invalid/a",
        ] {
            assert!(url_parts(u).is_err());
        }
        assert!(url_parts("https://release-assets.githubusercontent.com/asset?token=test").is_ok());
    }
    #[test]
    fn checksums_require_one_exact_filename_and_valid_bytes() {
        let sums = format!("{:x}  package.zip\n", Sha256::digest(b"fixture"));
        assert!(checksum(b"fixture", sums.as_bytes(), "package.zip").is_ok());
        assert!(checksum(b"altered", sums.as_bytes(), "package.zip").is_err());
        assert!(checksum(b"fixture", sums.as_bytes(), "other.zip").is_err());
        assert!(checksum(b"fixture", (sums.clone() + &sums).as_bytes(), "package.zip").is_err());
    }
    #[test]
    fn release_selection_requires_correct_assets_and_stable_newer_version() {
        let r = Release {
            version: "1.1.0".into(),
            tag: "v1.1.0".into(),
        };
        let mut value = serde_json::json!({"tag_name":"v1.1.0","draft":false,"prerelease":false,"assets":[{"name":r.filename(),"browser_download_url":r.url(&r.filename())},{"name":"SHA256SUMS.txt","browser_download_url":r.url("SHA256SUMS.txt")}]});
        assert!(matches!(
            release(&serde_json::to_vec(&value).unwrap()),
            Ok(Check::Available(_))
        ));
        value["assets"][0]["browser_download_url"] = "https://evil.invalid/file".into();
        assert!(release(&serde_json::to_vec(&value).unwrap()).is_err());
        value["tag_name"] = CURRENT.into();
        assert!(matches!(
            release(&serde_json::to_vec(&value).unwrap()),
            Ok(Check::Current)
        ));
    }
    #[test]
    fn zip_rejects_traversal_and_incomplete_payload() {
        use std::io::Write;
        let temp = tempfile::tempdir().unwrap();
        let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
        zip.start_file(
            "Seek-1.1.0-windows-x64/../escape",
            zip::write::SimpleFileOptions::default(),
        )
        .unwrap();
        zip.write_all(b"bad").unwrap();
        let bytes = zip.finish().unwrap().into_inner();
        let r = Release {
            version: "1.1.0".into(),
            tag: "v1.1.0".into(),
        };
        assert!(extract(bytes, temp.path(), &r).is_err());
        assert_eq!(std::fs::read_dir(temp.path()).unwrap().count(), 0);
    }
    #[test]
    fn exact_payload_is_accepted_but_missing_files_are_rejected() {
        use std::io::Write;
        let r = Release {
            version: "1.1.0".into(),
            tag: "v1.1.0".into(),
        };
        for count in [FILES.len(), FILES.len() - 1] {
            let temp = tempfile::tempdir().unwrap();
            let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
            for name in FILES.iter().take(count) {
                zip.start_file(
                    format!("Seek-1.1.0-windows-x64/{name}"),
                    zip::write::SimpleFileOptions::default(),
                )
                .unwrap();
                zip.write_all(b"owned fixture").unwrap();
            }
            let result = extract(zip.finish().unwrap().into_inner(), temp.path(), &r);
            assert_eq!(result.is_ok(), count == FILES.len());
        }
    }
    #[test]
    #[ignore = "Run only after the real release assets are published"]
    fn live_published_package_downloads_and_stages() {
        let r = Release {
            version: CURRENT.into(),
            tag: format!("v{CURRENT}"),
        };
        let stage = prepare(r).expect("GitHub HTTPS package download, checksum and extraction");
        assert!(
            FILES
                .iter()
                .all(|f| stage.join("payload").join(f).is_file())
        );
        // prepare created this exact absolute temporary directory for this test.
        assert_eq!(stage.parent(), Some(std::env::temp_dir().as_path()));
        std::fs::remove_dir_all(stage).unwrap();
    }
}
