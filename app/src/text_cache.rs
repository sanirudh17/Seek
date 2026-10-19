//! Account-private, opportunistic document extraction cache. Never a content index.
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    cell::RefCell,
    collections::BTreeMap,
    fs,
    io::{Read, Write},
    os::windows::{
        ffi::{OsStrExt, OsStringExt},
        fs::MetadataExt,
    },
    path::{Path, PathBuf},
};
pub const LIMIT: u64 = 128 * 1024 * 1024;
// Two encrypted manifests (old + atomic replacement), DPAPI/cluster overhead.
const RESERVED: u64 = 3 * 1024 * 1024;
// Bump when Rust extraction or normalization changes. PDF DLL changes are also keyed.
const VERSION: u32 = 1;
const MAX_TEXT: u64 = 8 * 1024 * 1024;
const MAX_ENTRIES: usize = 4096;
#[derive(Clone)]
pub struct Config {
    pub dir: PathBuf,
    pub enabled: bool,
}
impl Config {
    pub fn new(data: &Path, enabled: bool) -> Self {
        Self {
            dir: data.join("document-text-v1"),
            enabled,
        }
    }
}
#[derive(Clone, Default, Serialize)]
pub struct Stats {
    pub hits: usize,
    pub misses: usize,
    pub writes: usize,
    pub invalidated: usize,
    pub error: Option<String>,
}
#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
struct Key {
    path: PathBuf,
    size: u64,
    modified: u64,
    version: u32,
    digest: String,
    extractor: String,
}
#[derive(Serialize, Deserialize)]
struct Record {
    key: Key,
    used: u64,
    bytes: u64,
}
#[derive(Default, Serialize, Deserialize)]
struct Index {
    records: BTreeMap<String, Record>,
}
struct Session {
    dir: PathBuf,
    index: Index,
    stats: Stats,
    pdf_version: String,
    cluster: u64,
}
thread_local! {
 static POLICY:RefCell<Option<Config>>=const {RefCell::new(None)};
 static ACTIVE:RefCell<Option<Session>>=const {RefCell::new(None)};
 static OPEN_ERROR:RefCell<Option<String>>=const {RefCell::new(None)};
}
pub fn current() -> Option<Config> {
    POLICY.with(|p| p.borrow().clone())
}
pub fn policy<T>(config: Option<Config>, run: impl FnOnce() -> T) -> T {
    struct Reset(Option<Config>);
    impl Drop for Reset {
        fn drop(&mut self) {
            POLICY.with(|p| *p.borrow_mut() = self.0.take());
        }
    }
    let _reset = Reset(POLICY.with(|p| p.replace(config)));
    run()
}
pub struct Guard;
impl Drop for Guard {
    fn drop(&mut self) {
        let _ = finish();
    }
}
pub fn begin(roots: &[PathBuf], excludes: &[PathBuf]) -> Guard {
    OPEN_ERROR.with(|e| *e.borrow_mut() = None);
    if let Some(config) = current().filter(|c| c.enabled) {
        match Session::open(&config, roots, excludes) {
            Ok(cache) => ACTIVE.with(|s| *s.borrow_mut() = Some(cache)),
            Err(e) => OPEN_ERROR.with(|error| *error.borrow_mut() = Some(e)),
        }
    }
    Guard
}
pub fn finish() -> Stats {
    ACTIVE.with(|s| {
        let Some(mut cache) = s.borrow_mut().take() else {
            return Stats {
                error: OPEN_ERROR.with(|e| e.borrow_mut().take()),
                ..Default::default()
            };
        };
        if let Err(error) = cache.save() {
            cache.stats.error = Some(error);
        }
        cache.stats
    })
}
pub fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn key(path: &Path, meta: &fs::Metadata, bytes: &[u8], pdf_version: &str) -> Key {
    Key {
        path: path.to_owned(),
        size: meta.len(),
        modified: meta.last_write_time(),
        version: VERSION,
        digest: digest(bytes),
        extractor: if path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("pdf"))
        {
            pdf_version.into()
        } else {
            "docx-1".into()
        },
    }
}
fn id(key: &Key) -> String {
    let value = (
        normalized(&key.path),
        key.size,
        key.modified,
        key.version,
        &key.extractor,
    );
    digest(&serde_json::to_vec(&value).unwrap())
}
fn normalized(p: &Path) -> String {
    p.to_string_lossy()
        .trim_start_matches(r"\\?\")
        .replace('/', "\\")
        .to_lowercase()
}
fn within(path: &Path, root: &Path) -> bool {
    let p = normalized(path);
    let r = normalized(root);
    p == r
        || p.strip_prefix(&r)
            .is_some_and(|tail| tail.starts_with('\\'))
}
fn permitted(path: &Path, roots: &[PathBuf], excludes: &[PathBuf]) -> bool {
    !crate::content::secret(path)
        && roots.iter().any(|r| within(path, r))
        && !excludes.iter().any(|r| within(path, r))
}
pub fn lookup(path: &Path, meta: &fs::Metadata, bytes: &[u8]) -> Option<String> {
    ACTIVE.with(|s| {
        let mut state = s.borrow_mut();
        let cache = state.as_mut()?;
        if path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("pdf"))
            && cache.pdf_version == "unavailable"
        {
            cache.stats.misses += 1;
            return None;
        }
        let key = key(path, meta, bytes, &cache.pdf_version);
        let id = id(&key);
        let result = (|| -> Result<String, String> {
            let entry = cache.index.records.get(&id).ok_or("Not cached")?;
            if entry.key != key {
                return Err("Changed source".into());
            }
            let encrypted =
                read_bounded(&cache.dir.join(format!("{id}.bin")), MAX_TEXT + 1024 * 1024)?;
            let payload = protect(&encrypted, false)?;
            let split = payload
                .iter()
                .position(|b| *b == b'\n')
                .ok_or("Invalid cache header")?;
            let stored: Key =
                serde_json::from_slice(&payload[..split]).map_err(|e| e.to_string())?;
            if stored != key {
                return Err("Cache key mismatch".into());
            }
            let mut zip = zip::ZipArchive::new(std::io::Cursor::new(&payload[split + 1..]))
                .map_err(|e| e.to_string())?;
            if zip.len() != 1 {
                return Err("Invalid cache archive".into());
            }
            let file = zip.by_name("text").map_err(|e| e.to_string())?;
            if file.size() > MAX_TEXT {
                return Err("Cached text too large".into());
            }
            let mut text = String::new();
            file.take(MAX_TEXT + 1)
                .read_to_string(&mut text)
                .map_err(|e| e.to_string())?;
            if text.len() as u64 > MAX_TEXT {
                return Err("Cached text too large".into());
            }
            Ok(text)
        })();
        match result {
            Ok(text) => {
                cache.stats.hits += 1;
                if let Some(r) = cache.index.records.get_mut(&id) {
                    r.used = now();
                }
                Some(text)
            }
            Err(_) => {
                cache.stats.misses += 1;
                cache.remove(&id);
                None
            }
        }
    })
}
pub fn store(path: &Path, meta: &fs::Metadata, bytes: &[u8], text: &str) {
    ACTIVE.with(|s| {
        let mut state = s.borrow_mut();
        let Some(cache) = state.as_mut() else { return };
        if path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("pdf"))
            && cache.pdf_version == "unavailable"
        {
            return;
        }
        let result = (|| -> Result<(), String> {
            if text.len() as u64 > MAX_TEXT {
                return Ok(());
            }
            let key = key(path, meta, bytes, &cache.pdf_version);
            let id = id(&key);
            let stale: Vec<_> = cache
                .index
                .records
                .iter()
                .filter(|(other, r)| normalized(&r.key.path) == normalized(path) && **other != id)
                .map(|(id, _)| id.clone())
                .collect();
            for id in stale {
                cache.remove(&id);
            }
            let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
            zip.start_file(
                "text",
                zip::write::SimpleFileOptions::default()
                    .compression_method(zip::CompressionMethod::Deflated),
            )
            .map_err(|e| e.to_string())?;
            zip.write_all(text.as_bytes()).map_err(|e| e.to_string())?;
            let mut payload = serde_json::to_vec(&key).map_err(|e| e.to_string())?;
            payload.push(b'\n');
            payload.extend(zip.finish().map_err(|e| e.to_string())?.into_inner());
            let encrypted = protect(&payload, true)?;
            let allocated = (encrypted.len() as u64).div_ceil(cache.cluster) * cache.cluster;
            while cache.index.records.len() >= MAX_ENTRIES
                || cache.total() + allocated > LIMIT - RESERVED
            {
                let Some(old) = cache
                    .index
                    .records
                    .iter()
                    .min_by_key(|(_, r)| r.used)
                    .map(|(id, _)| id.clone())
                else {
                    break;
                };
                cache.remove(&old);
            }
            atomic(&cache.dir, &format!("{id}.bin"), &encrypted)?;
            cache.index.records.insert(
                id,
                Record {
                    key,
                    used: now(),
                    bytes: allocated,
                },
            );
            cache.stats.writes += 1;
            Ok(())
        })();
        if let Err(error) = result {
            cache.stats.error = Some(error);
        }
    });
}
fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
impl Session {
    fn open(config: &Config, roots: &[PathBuf], excludes: &[PathBuf]) -> Result<Self, String> {
        secure_dir(&config.dir)?;
        let roots: Vec<_> = roots.iter().filter_map(|r| r.canonicalize().ok()).collect();
        let excludes: Vec<_> = excludes
            .iter()
            .map(|r| r.canonicalize().unwrap_or_else(|_| r.clone()))
            .collect();
        let index = read_bounded(&config.dir.join("index.bin"), 2 * 1024 * 1024)
            .and_then(|b| protect(&b, false))
            .and_then(|b| serde_json::from_slice::<Index>(&b).map_err(|e| e.to_string()))
            .unwrap_or_default();
        let mut cache = Self {
            dir: config.dir.clone(),
            index,
            stats: Stats::default(),
            pdf_version: crate::pdf_text::fingerprint().unwrap_or_else(|_| "unavailable".into()),
            cluster: allocation_unit(&config.dir),
        };
        let stale: Vec<_> = cache
            .index
            .records
            .iter()
            .filter(|(id, r)| {
                !valid_id(id)
                    || r.key.version != VERSION
                    || r.key
                        .path
                        .extension()
                        .is_some_and(|e| e.eq_ignore_ascii_case("pdf"))
                        && r.key.extractor != cache.pdf_version
                    || !permitted(&r.key.path, &roots, &excludes)
                    || fs::symlink_metadata(&r.key.path).map_or(true, |m| {
                        !m.is_file()
                            || m.len() != r.key.size
                            || m.last_write_time() != r.key.modified
                            || crate::content::unavailable(&r.key.path, &m)
                    })
            })
            .map(|(id, _)| id.clone())
            .collect();
        for id in stale {
            cache.remove(&id);
        }
        // Orphans from interrupted writes and corrupt/obsolete manifests are derivatives only.
        for file in fs::read_dir(&cache.dir)
            .map_err(|e| e.to_string())?
            .flatten()
        {
            let name = file.file_name().to_string_lossy().to_string();
            if name.starts_with("cache-write-") && name.ends_with(".tmp") {
                let _ = fs::remove_file(file.path());
                continue;
            }
            if let Some(id) = name.strip_suffix(".bin").filter(|id| valid_id(id))
                && !cache.index.records.contains_key(id)
            {
                let _ = fs::remove_file(file.path());
            }
        }
        while cache.total() > LIMIT - RESERVED || cache.index.records.len() > MAX_ENTRIES {
            let Some(old) = cache
                .index
                .records
                .iter()
                .min_by_key(|(_, r)| r.used)
                .map(|(id, _)| id.clone())
            else {
                break;
            };
            cache.remove(&old);
        }
        Ok(cache)
    }
    fn total(&self) -> u64 {
        self.index.records.values().map(|r| r.bytes).sum()
    }
    fn remove(&mut self, id: &str) {
        if valid_id(id) {
            let _ = fs::remove_file(self.dir.join(format!("{id}.bin")));
        }
        if self.index.records.remove(id).is_some() {
            self.stats.invalidated += 1;
        }
    }
    fn save(&mut self) -> Result<(), String> {
        loop {
            let bytes = serde_json::to_vec(&self.index).map_err(|e| e.to_string())?;
            if bytes.len() <= 1024 * 1024 {
                return atomic(&self.dir, "index.bin", &protect(&bytes, true)?);
            }
            // Long paths must reduce capacity rather than prevent all cache reuse.
            let old = self
                .index
                .records
                .iter()
                .min_by_key(|(_, r)| r.used)
                .map(|(id, _)| id.clone())
                .ok_or("Cache manifest limit exceeded")?;
            self.remove(&old);
        }
    }
}
pub fn maintain(config: &Config, roots: &[PathBuf], excludes: &[PathBuf]) -> Result<(), String> {
    if !config.enabled {
        return clear(config);
    }
    if !config.dir.exists() {
        return Ok(());
    }
    let mut cache = Session::open(config, roots, excludes)?;
    cache.save()
}
pub fn clear(config: &Config) -> Result<(), String> {
    if !config.dir.exists() {
        return Ok(());
    }
    secure_dir(&config.dir)?;
    for file in fs::read_dir(&config.dir).map_err(|e| e.to_string())? {
        let file = file.map_err(|e| e.to_string())?;
        let name = file.file_name().to_string_lossy().to_string();
        if name == "index.bin"
            || name.strip_suffix(".bin").is_some_and(valid_id)
            || name.starts_with("cache-write-") && name.ends_with(".tmp")
        {
            fs::remove_file(file.path()).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}
fn valid_id(id: &str) -> bool {
    id.len() == 64 && id.bytes().all(|c| c.is_ascii_hexdigit())
}
fn allocation_unit(path: &Path) -> u64 {
    use windows_sys::Win32::Storage::FileSystem::GetDiskFreeSpaceW;
    let root = match path.components().next() {
        Some(std::path::Component::Prefix(p)) => match p.kind() {
            std::path::Prefix::Disk(d) | std::path::Prefix::VerbatimDisk(d) => {
                PathBuf::from(format!("{}:\\", char::from(d)))
            }
            _ => return 65536,
        },
        _ => return 65536,
    };
    let (mut sectors, mut bytes, mut free, mut total) = (0, 0, 0, 0);
    if unsafe {
        GetDiskFreeSpaceW(
            wide(&root).as_ptr(),
            &mut sectors,
            &mut bytes,
            &mut free,
            &mut total,
        )
    } == 0
    {
        65536
    } else {
        (u64::from(sectors) * u64::from(bytes)).max(4096)
    }
}
fn read_bounded(path: &Path, limit: u64) -> Result<Vec<u8>, String> {
    let metadata = fs::symlink_metadata(path).map_err(|e| e.to_string())?;
    if !metadata.is_file() || metadata.file_attributes() & 0x400 != 0 || metadata.len() > limit {
        return Err("Invalid cache file".into());
    }
    let mut out = Vec::new();
    fs::File::open(path)
        .map_err(|e| e.to_string())?
        .take(limit + 1)
        .read_to_end(&mut out)
        .map_err(|e| e.to_string())?;
    if out.len() as u64 > limit {
        return Err("Cache file too large".into());
    }
    Ok(out)
}
fn wide(path: &Path) -> Vec<u16> {
    path.as_os_str().encode_wide().chain(Some(0)).collect()
}
fn atomic(dir: &Path, name: &str, bytes: &[u8]) -> Result<(), String> {
    use windows_sys::Win32::Storage::FileSystem::{
        MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW,
    };
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let temp = dir.join(format!(
        "cache-write-{}-{}.tmp",
        std::process::id(),
        NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    let result = (|| {
        let mut f = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp)
            .map_err(|e| e.to_string())?;
        f.write_all(bytes)
            .and_then(|_| f.sync_all())
            .map_err(|e| e.to_string())?;
        drop(f);
        if unsafe {
            MoveFileExW(
                wide(&temp).as_ptr(),
                wide(&dir.join(name)).as_ptr(),
                MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
            )
        } == 0
        {
            return Err(std::io::Error::last_os_error().to_string());
        }
        Ok(())
    })();
    let _ = fs::remove_file(temp);
    result
}
#[repr(C)]
struct Blob {
    len: u32,
    bytes: *mut u8,
}
#[link(name = "crypt32")]
unsafe extern "system" {
    fn CryptProtectData(
        input: *const Blob,
        description: *const u16,
        entropy: *const Blob,
        reserved: *const std::ffi::c_void,
        prompt: *const std::ffi::c_void,
        flags: u32,
        out: *mut Blob,
    ) -> i32;
    fn CryptUnprotectData(
        input: *const Blob,
        description: *mut *mut u16,
        entropy: *const Blob,
        reserved: *const std::ffi::c_void,
        prompt: *const std::ffi::c_void,
        flags: u32,
        out: *mut Blob,
    ) -> i32;
}
#[link(name = "advapi32")]
unsafe extern "system" {
    fn ConvertStringSecurityDescriptorToSecurityDescriptorW(
        sddl: *const u16,
        revision: u32,
        out: *mut *mut std::ffi::c_void,
        size: *mut u32,
    ) -> i32;
    fn SetFileSecurityW(
        path: *const u16,
        information: u32,
        descriptor: *const std::ffi::c_void,
    ) -> i32;
}
fn protect(bytes: &[u8], encrypt: bool) -> Result<Vec<u8>, String> {
    let input = Blob {
        len: bytes.len() as u32,
        bytes: bytes.as_ptr() as *mut u8,
    };
    let mut out = Blob {
        len: 0,
        bytes: std::ptr::null_mut(),
    };
    let result = unsafe {
        if encrypt {
            CryptProtectData(
                &input,
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
                1,
                &mut out,
            )
        } else {
            CryptUnprotectData(
                &input,
                std::ptr::null_mut(),
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
                1,
                &mut out,
            )
        }
    };
    if result == 0 {
        return Err(std::io::Error::last_os_error().to_string());
    }
    let value = if out.len as u64 > MAX_TEXT + 1024 * 1024 {
        Err("Decrypted cache exceeds limit".into())
    } else {
        Ok(unsafe { std::slice::from_raw_parts(out.bytes, out.len as usize) }.to_vec())
    };
    unsafe {
        windows_sys::Win32::Foundation::LocalFree(out.bytes.cast());
    }
    value
}
fn secure_dir(dir: &Path) -> Result<(), String> {
    use windows_sys::Win32::{
        Security::SECURITY_ATTRIBUTES,
        Storage::FileSystem::CreateDirectoryW,
        System::Com::CoTaskMemFree,
        UI::Shell::{FOLDERID_Profile, SHGetKnownFolderPath},
    };
    let mut ptr = std::ptr::null_mut();
    if unsafe { SHGetKnownFolderPath(&FOLDERID_Profile, 0, std::ptr::null_mut(), &mut ptr) } < 0
        || ptr.is_null()
    {
        return Err("Cannot resolve user profile".into());
    }
    let profile = unsafe {
        let mut len = 0;
        while *ptr.add(len) != 0 {
            len += 1;
        }
        let p = PathBuf::from(std::ffi::OsString::from_wide(std::slice::from_raw_parts(
            ptr, len,
        )));
        CoTaskMemFree(ptr.cast());
        p
    };
    let parent = dir
        .parent()
        .ok_or("Cache needs a parent")?
        .canonicalize()
        .map_err(|e| e.to_string())?;
    let profile = profile.canonicalize().map_err(|e| e.to_string())?;
    if !within(&parent, &profile) {
        return Err("Document cache must stay inside the user profile".into());
    }
    if let Ok(m) = fs::symlink_metadata(dir)
        && (!m.is_dir() || m.file_attributes() & 0x400 != 0)
    {
        return Err("Cache directory cannot be a link".into());
    }
    let sid = seek_engine::helper::user_sid().map_err(|e| e.to_string())?;
    let descriptor_text = format!("D:P(A;OICI;FA;;;{sid})");
    let sddl: Vec<u16> = descriptor_text.encode_utf16().chain(Some(0)).collect();
    let mut descriptor = std::ptr::null_mut();
    if unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            sddl.as_ptr(),
            1,
            &mut descriptor,
            std::ptr::null_mut(),
        )
    } == 0
    {
        return Err(std::io::Error::last_os_error().to_string());
    }
    let security = SECURITY_ATTRIBUTES {
        nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: descriptor,
        bInheritHandle: 0,
    };
    let result = (|| {
        if !dir.exists() && unsafe { CreateDirectoryW(wide(dir).as_ptr(), &security) } == 0 {
            return Err(std::io::Error::last_os_error().to_string());
        }
        if !within(&dir.canonicalize().map_err(|e| e.to_string())?, &profile) {
            return Err("Cache escaped the user profile".into());
        }
        if unsafe { SetFileSecurityW(wide(dir).as_ptr(), 0x80000004, descriptor) } == 0 {
            return Err(std::io::Error::last_os_error().to_string());
        }
        Ok(())
    })();
    unsafe {
        windows_sys::Win32::Foundation::LocalFree(descriptor);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, atomic::AtomicU64};
    fn document(path: &Path, text: &str) {
        let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        zip.start_file(
            "word/document.xml",
            zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Stored),
        )
        .unwrap();
        write!(
            zip,
            "<w:document xmlns:w=\"w\"><w:p><w:r><w:t>{text}</w:t></w:r></w:p></w:document>"
        )
        .unwrap();
        fs::write(path, zip.finish().unwrap().into_inner()).unwrap();
    }
    fn scan(config: &Config, roots: &[PathBuf], query: &str) -> crate::content::Report {
        policy(Some(config.clone()), || {
            crate::content::search(
                query,
                roots,
                None,
                &[],
                true,
                Arc::new(AtomicU64::new(1)),
                1,
            )
        })
    }
    fn fixture() -> (tempfile::TempDir, PathBuf, Config) {
        let base = tempfile::tempdir().unwrap();
        let files = base.path().join("files");
        let data = base.path().join("data");
        fs::create_dir_all(&files).unwrap();
        fs::create_dir_all(&data).unwrap();
        let config = Config::new(&data, true);
        (base, files, config)
    }
    #[test]
    fn persistent_hits_and_same_metadata_edit_cannot_return_old_text() {
        let (_base, files, config) = fixture();
        let path = files.join("A.docx");
        document(&path, "marble");
        let roots = vec![files];
        let first = scan(&config, &roots, "marble");
        assert_eq!(
            (first.rows.len(), first.cache.hits, first.cache.writes),
            (1, 0, 1)
        );
        assert!(first.cache.error.is_none(), "{:?}", first.cache.error);
        let second = scan(&config, &roots, "marble");
        assert_eq!((second.rows.len(), second.cache.hits), (1, 1));
        let before = fs::metadata(&path).unwrap();
        document(&path, "quartz");
        fs::File::options()
            .write(true)
            .open(&path)
            .unwrap()
            .set_modified(before.modified().unwrap())
            .unwrap();
        assert_eq!(fs::metadata(&path).unwrap().len(), before.len());
        let changed = scan(&config, &roots, "marble");
        assert!(changed.rows.is_empty());
        assert_eq!((changed.cache.hits, changed.cache.writes), (0, 1));
        assert_eq!(scan(&config, &roots, "quartz").cache.hits, 1);
    }
    #[test]
    fn rename_delete_removed_scope_and_disable_purge_derivatives() {
        let (_base, files, config) = fixture();
        let path = files.join("A.docx");
        document(&path, "quartz");
        let roots = vec![files.clone()];
        assert_eq!(scan(&config, &roots, "quartz").rows.len(), 1);
        let renamed = files.join("B.docx");
        fs::rename(path, &renamed).unwrap();
        let report = scan(&config, &roots, "quartz");
        assert_eq!(report.cache.hits, 0);
        assert_eq!(report.rows[0].name, "B.docx");
        assert!(report.cache.invalidated >= 1);
        maintain(&config, &[], &[]).unwrap();
        assert_eq!(
            Session::open(&config, &roots, &[])
                .unwrap()
                .index
                .records
                .len(),
            0
        );
        assert_eq!(scan(&config, &roots, "quartz").cache.writes, 1);
        fs::remove_file(renamed).unwrap();
        assert!(scan(&config, &roots, "quartz").rows.is_empty());
        assert_eq!(
            Session::open(&config, &roots, &[])
                .unwrap()
                .index
                .records
                .len(),
            0
        );
        document(&files.join("C.docx"), "quartz");
        scan(&config, &roots, "quartz");
        maintain(
            &Config {
                enabled: false,
                ..config.clone()
            },
            &roots,
            &[],
        )
        .unwrap();
        assert_eq!(fs::read_dir(&config.dir).unwrap().count(), 0);
    }
    #[test]
    fn corruption_and_extractor_version_fail_open_to_fresh_extraction() {
        let (_base, files, config) = fixture();
        document(&files.join("A.docx"), "quartz");
        let roots = vec![files];
        scan(&config, &roots, "quartz");
        let mut session = Session::open(&config, &roots, &[]).unwrap();
        let id = session.index.records.keys().next().unwrap().clone();
        let bytes = fs::read(config.dir.join(format!("{id}.bin"))).unwrap();
        assert!(!bytes.windows(6).any(|s| s == b"quartz"));
        fs::write(config.dir.join(format!("{id}.bin")), b"broken ciphertext").unwrap();
        let report = scan(&config, &roots, "quartz");
        assert_eq!(
            (report.rows.len(), report.cache.hits, report.cache.writes),
            (1, 0, 1)
        );
        session = Session::open(&config, &roots, &[]).unwrap();
        for record in session.index.records.values_mut() {
            record.key.version += 1;
        }
        session.save().unwrap();
        let report = scan(&config, &roots, "quartz");
        assert_eq!(
            (report.rows.len(), report.cache.hits, report.cache.writes),
            (1, 0, 1)
        );
    }
    #[test]
    fn old_usage_timestamp_does_not_expire_unchanged_document_text() {
        let (_base, files, config) = fixture();
        document(&files.join("A.docx"), "quartz");
        let roots = vec![files];
        assert_eq!(scan(&config, &roots, "quartz").cache.writes, 1);
        let mut session = Session::open(&config, &roots, &[]).unwrap();
        for record in session.index.records.values_mut() {
            record.used = 1;
        }
        session.save().unwrap();
        drop(session);
        let report = scan(&config, &roots, "quartz");
        assert_eq!(
            (report.rows.len(), report.cache.hits, report.cache.writes),
            (1, 1, 0)
        );
        assert_eq!(report.cache.invalidated, 0);
    }
    #[test]
    fn cap_evicts_old_entries_and_profile_boundary_is_enforced() {
        let (_base, files, config) = fixture();
        document(&files.join("A.docx"), "quartz");
        let roots = vec![files.clone()];
        scan(&config, &roots, "quartz");
        policy(Some(config.clone()), || {
            let _guard = begin(&roots, &[]);
            ACTIVE.with(|s| {
                let mut s = s.borrow_mut();
                for r in s.as_mut().unwrap().index.records.values_mut() {
                    r.bytes = LIMIT;
                    r.used = 0;
                }
            });
            let path = files.join("B.docx");
            document(&path, "marble");
            let bytes = fs::read(&path).unwrap();
            store(&path, &fs::metadata(&path).unwrap(), &bytes, "marble");
            ACTIVE.with(|s| {
                let s = s.borrow();
                let s = s.as_ref().unwrap();
                assert!(s.total() < LIMIT);
                assert_eq!(s.index.records.len(), 1);
            });
        });
        let system = PathBuf::from(std::env::var_os("SystemRoot").unwrap());
        assert!(secure_dir(&system.join("Seek-no-create-cache-test")).is_err());
    }
}
