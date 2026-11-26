//! Find Duplicates (Phase 9.3), after Monodrive's docs/storage.md and the
//! practice of fclones, czkawka and dupeGuru. Candidates share a size; a
//! BLAKE3 hash of the first, middle and last 64 KB splits them; survivors
//! are hashed in full, and a group is confirmed only when the full hashes
//! match and each file's length and last change were the same before and
//! after it was read. Links and junctions are never followed, cloud
//! placeholders are skipped (never downloaded), hard links to one file are
//! one file, and system files and folder clutter (desktop.ini, Thumbs.db)
//! are left out.
//!
//! Identical folders are reported only when two folders hold exactly the
//! same files and subfolders: every relative name, every size and every
//! file's contents. A folder is never paired with one inside it, and a
//! match is widened to the highest pair of parents that still match, so a
//! copied project shows once.
use std::{
    collections::HashMap,
    io::{Read, Seek, SeekFrom},
    os::windows::fs::MetadataExt,
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
    time::SystemTime,
};

const SAMPLE: u64 = 64 * 1024;
/// Most files one search looks at.
pub const MAX_FILES: usize = 500_000;
const CLOUD: u32 = 0x400000 | 0x40000 | 0x1000;
const REPARSE: u32 = 0x400;

/// Files of one size: path and last change.
type Listed = Vec<(PathBuf, Option<SystemTime>)>;

#[derive(Clone, Debug)]
pub struct Copy {
    pub path: PathBuf,
    pub modified: Option<SystemTime>,
}

#[derive(Clone, Debug)]
pub struct Group {
    pub size: u64,
    pub copies: Vec<Copy>,
}
impl Group {
    /// Bytes freed by keeping one copy.
    pub fn reclaimable(&self) -> u64 {
        self.size * (self.copies.len() as u64 - 1)
    }
}

/// One entry of a folder's listing: relative path (lower case), size,
/// last change; folders have no size.
pub type Entry = (String, Option<u64>, Option<SystemTime>);

/// Two folders with exactly the same contents.
#[derive(Clone, Debug)]
pub struct FolderCopy {
    pub a: PathBuf,
    pub b: PathBuf,
    pub files: usize,
    pub bytes: u64,
    /// The listings as compared, to check nothing changed before moving.
    pub listing_a: Vec<Entry>,
    pub listing_b: Vec<Entry>,
}

#[derive(Clone, Debug, Default)]
pub struct Report {
    pub groups: Vec<Group>,
    /// Folders whose whole contents are identical.
    pub folders: Vec<FolderCopy>,
    pub files: usize,
    pub skipped: usize,
    pub stopped: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Keep {
    Oldest,
    Newest,
    ShortestPath,
}

/// Which copy the rule keeps.
pub fn keeper(group: &Group, rule: Keep) -> usize {
    let indexed = group.copies.iter().enumerate();
    match rule {
        Keep::Oldest => indexed.min_by_key(|(_, c)| c.modified).map(|(i, _)| i),
        Keep::Newest => indexed.max_by_key(|(_, c)| c.modified).map(|(i, _)| i),
        Keep::ShortestPath => indexed
            .min_by_key(|(_, c)| c.path.as_os_str().len())
            .map(|(i, _)| i),
    }
    .unwrap_or(0)
}

pub(crate) fn file_id(path: &Path) -> Option<(u32, u64)> {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Storage::FileSystem::{
        BY_HANDLE_FILE_INFORMATION, GetFileInformationByHandle,
    };
    let file = std::fs::File::open(path).ok()?;
    let mut info: BY_HANDLE_FILE_INFORMATION = unsafe { std::mem::zeroed() };
    (unsafe { GetFileInformationByHandle(file.as_raw_handle(), &mut info) } != 0).then_some((
        info.dwVolumeSerialNumber,
        ((info.nFileIndexHigh as u64) << 32) | info.nFileIndexLow as u64,
    ))
}

fn dir_id(path: &Path) -> Option<(u32, u64)> {
    use std::os::windows::{fs::OpenOptionsExt, io::AsRawHandle};
    use windows_sys::Win32::Storage::FileSystem::{
        BY_HANDLE_FILE_INFORMATION, GetFileInformationByHandle,
    };
    let dir = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(0x0200_0000) // FILE_FLAG_BACKUP_SEMANTICS
        .open(path)
        .ok()?;
    let mut info: BY_HANDLE_FILE_INFORMATION = unsafe { std::mem::zeroed() };
    (unsafe { GetFileInformationByHandle(dir.as_raw_handle(), &mut info) } != 0).then_some((
        info.dwVolumeSerialNumber,
        ((info.nFileIndexHigh as u64) << 32) | info.nFileIndexLow as u64,
    ))
}

fn sample_hash(path: &Path, size: u64) -> std::io::Result<[u8; 32]> {
    let mut file = std::fs::File::open(path)?;
    let mut hasher = blake3::Hasher::new();
    let mut buffer = vec![0u8; SAMPLE as usize];
    let offsets = if size <= SAMPLE * 3 {
        vec![0]
    } else {
        vec![0, size / 2 - SAMPLE / 2, size - SAMPLE]
    };
    for offset in offsets {
        file.seek(SeekFrom::Start(offset))?;
        // `read` may return less than asked; keep going to fill the sample.
        let mut filled = 0;
        while filled < buffer.len() {
            match file.read(&mut buffer[filled..])? {
                0 => break,
                n => filled += n,
            }
        }
        hasher.update(&buffer[..filled]);
    }
    Ok(*hasher.finalize().as_bytes())
}

/// The whole file's hash. A file whose length doesn't match `expected_size` or
/// whose length or last change differ after reading from before (it was being
/// written) is an error, so it is never grouped on a stale or half-read state.
fn full_hash(
    path: &Path,
    expected_size: u64,
    cancel: &AtomicBool,
) -> std::io::Result<Option<[u8; 32]>> {
    let mut file = std::fs::File::open(path)?;
    let before = file.metadata()?;
    if before.len() != expected_size {
        return Err(std::io::Error::other("size changed before hashing"));
    }
    let mut hasher = blake3::Hasher::new();
    let mut buffer = vec![0u8; 1 << 20];
    let mut read = 0u64;
    loop {
        if cancel.load(Ordering::Relaxed) {
            return Ok(None);
        }
        let n = file.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        read += n as u64;
        hasher.update(&buffer[..n]);
    }
    let after = std::fs::metadata(path)?;
    if read != expected_size
        || after.len() != expected_size
        || after.modified().ok() != before.modified().ok()
    {
        return Err(std::io::Error::other("changed while it was read"));
    }
    Ok(Some(*hasher.finalize().as_bytes()))
}

/// System files and the clutter Windows and macOS leave in folders: never
/// offered, since removing them changes how a folder looks, not its space.
fn clutter(path: &Path, attributes: u32) -> bool {
    const SYSTEM: u32 = 0x4;
    attributes & SYSTEM != 0
        || path.file_name().and_then(|n| n.to_str()).is_some_and(|n| {
            let n = n.to_ascii_lowercase();
            matches!(n.as_str(), "desktop.ini" | "thumbs.db" | "ehthumbs.db" | ".ds_store")
                || n.starts_with("._")
        })
}

/// Finds duplicate files under `folder`. `progress(done, total, stage)`
/// reports as it goes.
pub fn find(
    folder: &Path,
    cancel: &AtomicBool,
    progress: impl Fn(usize, usize, &'static str) + Sync,
) -> Report {
    let mut report = Report::default();
    // 1. Every regular file, by size. Links and junctions are not entered.
    let mut by_size: HashMap<u64, Listed> = HashMap::new();
    let mut stack = vec![folder.to_path_buf()];
    while let Some(dir) = stack.pop() {
        if cancel.load(Ordering::Relaxed) {
            report.stopped = Some("Stopped".into());
            return report;
        }
        let Ok(entries) = std::fs::read_dir(&dir) else {
            report.skipped += 1;
            continue;
        };
        for entry in entries.flatten() {
            let Ok(meta) = entry.metadata() else {
                report.skipped += 1;
                continue;
            };
            let attributes = meta.file_attributes();
            if attributes & REPARSE != 0 || meta.file_type().is_symlink() {
                continue;
            }
            if meta.is_dir() {
                stack.push(entry.path());
                continue;
            }
            if attributes & CLOUD != 0 {
                report.skipped += 1;
                continue;
            }
            if meta.len() == 0 {
                continue;
            }
            report.files += 1;
            if report.files > MAX_FILES {
                report.stopped = Some(format!(
                    "More than {MAX_FILES} files; pick a smaller folder"
                ));
                return report;
            }
            if report.files % 2_000 == 0 {
                progress(report.files, 0, "Listing files");
            }
            by_size
                .entry(meta.len())
                .or_default()
                .push((entry.path(), meta.modified().ok()));
        }
    }
    confirm(by_size, report, cancel, &progress)
}

/// Splits same-size files by sampled, then full, hashes into confirmed
/// groups. Cloud placeholders and links found here (the index doesn't know
/// about them) are skipped before anything is opened.
fn confirm(
    by_size: HashMap<u64, Listed>,
    mut report: Report,
    cancel: &AtomicBool,
    progress: &(impl Fn(usize, usize, &'static str) + Sync),
) -> Report {
    use std::sync::{Mutex, atomic::AtomicUsize};
    let mut candidates: Vec<(u64, Listed)> =
        by_size.into_iter().filter(|(_, v)| v.len() > 1).collect();
    // Biggest first, so a stopped search has already found what matters most.
    candidates.sort_by_key(|(size, _)| std::cmp::Reverse(*size));
    let total: usize = candidates.iter().map(|(_, v)| v.len()).sum();
    let done = AtomicUsize::new(0);
    let next = AtomicUsize::new(0);
    let skipped = AtomicUsize::new(0);
    let groups = Mutex::new(Vec::new());
    // Size groups are independent; several readers keep an SSD busy.
    let workers = std::thread::available_parallelism().map_or(4, |n| n.get().clamp(2, 6));
    std::thread::scope(|scope| {
        for _ in 0..workers {
            scope.spawn(|| {
                loop {
                    let i = next.fetch_add(1, Ordering::Relaxed);
                    let Some((size, files)) = candidates.get(i) else { break };
                    if cancel.load(Ordering::Relaxed) {
                        break;
                    }
                    let size = *size;
                    // Hard links to one file are the same file, not copies;
                    // so is one path listed twice (overlapping locations),
                    // even where the file's identity can't be read.
                    let mut seen_ids = std::collections::HashSet::new();
                    let mut seen_paths = std::collections::HashSet::new();
                    let files: Vec<_> = files
                        .iter()
                        .filter(|(p, _)| {
                            // Opening a placeholder would download it.
                            std::fs::symlink_metadata(p).is_ok_and(|m| {
                                m.is_file()
                                    && m.file_attributes() & (CLOUD | REPARSE) == 0
                                    && !clutter(p, m.file_attributes())
                            })
                        })
                        .filter(|(p, _)| seen_paths.insert(p.to_string_lossy().to_lowercase()))
                        .filter(|(p, _)| file_id(p).is_none_or(|id| seen_ids.insert(id)))
                        .cloned()
                        .collect();
                    // Sample hash.
                    let mut by_sample: HashMap<[u8; 32], Listed> = HashMap::new();
                    for (path, modified) in files {
                        let n = done.fetch_add(1, Ordering::Relaxed) + 1;
                        if n.is_multiple_of(200) {
                            progress(n, total, "Comparing");
                        }
                        match sample_hash(&path, size) {
                            Ok(hash) => by_sample.entry(hash).or_default().push((path, modified)),
                            Err(_) => {
                                skipped.fetch_add(1, Ordering::Relaxed);
                            }
                        }
                    }
                    // Full hash of the survivors confirms a group.
                    for (_, files) in by_sample.into_iter().filter(|(_, v)| v.len() > 1) {
                        let mut by_full: HashMap<[u8; 32], Vec<Copy>> = HashMap::new();
                        for (path, modified) in files {
                            match full_hash(&path, size, cancel) {
                                Ok(Some(hash)) => {
                                    by_full.entry(hash).or_default().push(Copy { path, modified })
                                }
                                Ok(None) => return,
                                Err(_) => {
                                    skipped.fetch_add(1, Ordering::Relaxed);
                                }
                            }
                        }
                        let found: Vec<Group> = by_full
                            .into_values()
                            .filter(|v| v.len() > 1)
                            .map(|copies| Group { size, copies })
                            .collect();
                        if let Ok(mut groups) = groups.lock() {
                            groups.extend(found);
                        }
                    }
                }
            });
        }
    });
    report.groups = groups.into_inner().unwrap_or_default();
    report.skipped += skipped.into_inner();
    if cancel.load(Ordering::Relaxed) {
        report.stopped = Some("Stopped".into());
    }
    report
        .groups
        .sort_by_key(|g| std::cmp::Reverse(g.reclaimable()));
    if !cancel.load(Ordering::Relaxed) {
        progress(0, 0, "Comparing folders");
        report.folders = identical_folders(&report.groups, cancel);
    }
    progress(total, total, "Done");
    report
}

/// Most entries a folder may hold to be compared, and new bytes hashed per
/// folder pair and per search (files outside the groups, usually small).
const FOLDER_ENTRIES: usize = 20_000;
const PAIR_BYTES: u64 = 512 << 20;
const SEARCH_BYTES: u64 = 4 << 30;

/// A folder's full listing, relative and in lower case, with each file's
/// path. `None` when it can't be compared safely: unreadable, too large, or
/// holding links, junctions or cloud placeholders.
fn listing(dir: &Path) -> Option<(Vec<Entry>, HashMap<String, PathBuf>)> {
    let mut entries = vec![];
    let mut files = HashMap::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(folder) = stack.pop() {
        for entry in std::fs::read_dir(&folder).ok()? {
            let entry = entry.ok()?;
            let meta = entry.metadata().ok()?;
            let attributes = meta.file_attributes();
            if attributes & (REPARSE | CLOUD) != 0 {
                return None;
            }
            let path = entry.path();
            if !meta.is_dir() && clutter(&path, attributes) {
                continue;
            }
            let rel = path
                .strip_prefix(dir)
                .ok()?
                .to_string_lossy()
                .to_lowercase();
            if meta.is_dir() {
                entries.push((rel, None, None));
                stack.push(path);
            } else {
                entries.push((rel.clone(), Some(meta.len()), meta.modified().ok()));
                files.insert(rel, path);
            }
            if entries.len() > FOLDER_ENTRIES {
                return None;
            }
        }
    }
    entries.sort_by(|x, y| x.0.cmp(&y.0));
    Some((entries, files))
}

/// Whether folder `a`'s listing still matches what was compared.
pub fn listing_unchanged(dir: &Path, compared: &[Entry]) -> bool {
    listing(dir).is_some_and(|(now, _)| now == compared)
}

fn key(path: &Path) -> String {
    path.to_string_lossy().to_lowercase()
}

/// Whether one path is inside (or is) the other.
fn nested(a: &Path, b: &Path) -> bool {
    let (a, b) = (key(a), key(b));
    a == b || a.starts_with(&format!("{b}\\")) || b.starts_with(&format!("{a}\\"))
}

/// Compares two folders completely. Files both in one verified group are
/// known to match; others are hashed, within `budget` bytes.
fn same_folders(
    a: &Path,
    b: &Path,
    group_of: &HashMap<String, usize>,
    budget: &mut u64,
    cancel: &AtomicBool,
) -> Option<FolderCopy> {
    if let (Some(ia), Some(ib)) = (dir_id(a), dir_id(b))
        && ia == ib
    {
        return None;
    }
    let (listing_a, files_a) = listing(a)?;
    let (listing_b, files_b) = listing(b)?;
    // Same names, same kinds, same sizes, or not the same folder.
    if listing_a.len() != listing_b.len()
        || listing_a
            .iter()
            .zip(&listing_b)
            .any(|(x, y)| x.0 != y.0 || x.1 != y.1)
        || files_a.is_empty()
    {
        return None;
    }
    let mut spent = 0u64;
    let mut bytes = 0u64;
    let mut distinct_files = 0usize;
    for (rel, size, _) in &listing_a {
        let Some(size) = size else { continue };
        if *size == 0 {
            continue;
        }
        let (pa, pb) = (&files_a[rel], &files_b[rel]);
        // Hard links to the same underlying file take no duplicate space and
        // must not count as duplicated bytes.
        if let (Some(ia), Some(ib)) = (file_id(pa), file_id(pb))
            && ia == ib
        {
            continue;
        }
        distinct_files += 1;
        bytes += size;
        let known = match (group_of.get(&key(pa)), group_of.get(&key(pb))) {
            (Some(x), Some(y)) => Some(x == y),
            _ => None,
        };
        match known {
            Some(true) => continue,
            Some(false) => return None,
            None => {
                spent += 2 * size;
                if spent > PAIR_BYTES || spent > *budget || cancel.load(Ordering::Relaxed) {
                    return None;
                }
                let ha = full_hash(pa, *size, cancel).ok()??;
                let hb = full_hash(pb, *size, cancel).ok()??;
                if ha != hb {
                    return None;
                }
            }
        }
    }
    if distinct_files == 0 {
        return None;
    }
    *budget = budget.saturating_sub(spent);
    Some(FolderCopy {
        a: a.to_path_buf(),
        b: b.to_path_buf(),
        files: files_a.len(),
        bytes,
        listing_a,
        listing_b,
    })
}

/// Pairs of folders with identical contents, among the folders that hold
/// confirmed duplicates. Each match is widened to the highest parents that
/// still match (stopping at a drive, the profile, or where the two meet),
/// and matches inside a wider one are dropped.
pub fn identical_folders(groups: &[Group], cancel: &AtomicBool) -> Vec<FolderCopy> {
    let mut group_of = HashMap::new();
    for (i, g) in groups.iter().enumerate() {
        for c in &g.copies {
            group_of.insert(key(&c.path), i);
        }
    }
    let mut candidates: Vec<(PathBuf, PathBuf)> = vec![];
    let mut seen = std::collections::HashSet::new();
    for g in groups {
        let parents: Vec<&Path> = g.copies.iter().take(8).filter_map(|c| c.path.parent()).collect();
        for (i, a) in parents.iter().enumerate() {
            for b in &parents[i + 1..] {
                if nested(a, b) {
                    continue;
                }
                let (x, y) = if key(a) < key(b) { (a, b) } else { (b, a) };
                if seen.insert((key(x), key(y))) {
                    candidates.push((x.to_path_buf(), y.to_path_buf()));
                }
            }
        }
        if candidates.len() >= 500 {
            break;
        }
    }
    let home = std::env::var_os("USERPROFILE").map(PathBuf::from);
    let stop = |p: &Path| p.parent().is_none() || home.as_deref().is_some_and(|h| key(h) == key(p));
    let mut budget = SEARCH_BYTES;
    let mut found: Vec<FolderCopy> = vec![];
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    for (a, b) in candidates {
        if cancel.load(Ordering::Relaxed) || std::time::Instant::now() >= deadline {
            break;
        }
        // Already inside a wider match.
        if found.iter().any(|f| {
            (key(&a).starts_with(&key(&f.a)) && key(&b).starts_with(&key(&f.b)))
                || (key(&a).starts_with(&key(&f.b)) && key(&b).starts_with(&key(&f.a)))
        }) {
            continue;
        }
        let Some(mut best) = same_folders(&a, &b, &group_of, &mut budget, cancel) else {
            continue;
        };
        // Widen while the parents still match.
        for _ in 0..6 {
            let (Some(pa), Some(pb)) = (best.a.parent(), best.b.parent()) else { break };
            if stop(pa) || stop(pb) || nested(pa, pb) {
                break;
            }
            match same_folders(pa, pb, &group_of, &mut budget, cancel) {
                Some(wider) => best = wider,
                None => break,
            }
        }
        found.retain(|f| {
            !(key(&f.a).starts_with(&key(&best.a)) && key(&f.b).starts_with(&key(&best.b))
                || key(&f.a).starts_with(&key(&best.b)) && key(&f.b).starts_with(&key(&best.a)))
        });
        found.push(best);
    }
    found.sort_by_key(|f| std::cmp::Reverse(f.bytes));
    found
}

/// Smallest file the everywhere search compares; smaller copies rarely
/// matter for space and would swamp the list.
pub const EVERYWHERE_MIN: u64 = 1 << 20;

/// A folder that is really one document an app keeps as a folder (its name
/// ends in an extension, such as "Clip.glint-studio" or "Library.photoslibrary"):
/// the files inside belong to that document.
fn bundle(name: &str) -> bool {
    name.rsplit_once('.').is_some_and(|(stem, ext)| {
        !stem.is_empty()
            && ext.len() >= 3
            && ext.len() <= 24
            && ext.chars().all(|c| c.is_ascii_alphabetic() || c == '-')
    })
}

/// Folders whose copies aren't the person's to tidy: Windows, installed
/// programs and games, app data (AppData and the profile's hidden dot-folders
/// such as .codex or .local), package caches, repositories' internals, and
/// files inside document bundles.
pub(crate) fn system_path(path: &Path) -> bool {
    let parts: Vec<_> = path.components().collect();
    let last = parts.len().saturating_sub(1);
    parts.iter().enumerate().any(|(i, c)| {
        let c = c.as_os_str().to_string_lossy().to_ascii_lowercase();
        // Folders only (the file's own name may start with a dot or have an
        // extension).
        if i < last && (c.starts_with('.') || bundle(&c)) {
            return true;
        }
        matches!(
            c.as_str(),
            "windows"
                | "program files"
                | "program files (x86)"
                | "programdata"
                | "appdata"
                | "$recycle.bin"
                | "system volume information"
                | "recovery"
                | "perflogs"
                | "msocache"
                | "xboxgames"
                | "steamapps"
                | "node_modules"
                | ".git"
                | ".cargo"
                | ".rustup"
                | ".gradle"
                | ".m2"
                | ".nuget"
                | "windowsapps"
                // Build output: rebuilt copies, which Cleanup handles.
                | "target"
                | ".venv"
                | "__pycache__"
                | ".next"
        )
    })
}

/// Whether `path` sits inside a project: a folder in a Git, Mercurial or
/// Subversion repository. Copies there are usually each project's own
/// (vendored binaries, assets), and removing one would break that project.
/// Answers are cached by folder.
fn in_project(path: &Path, cache: &mut HashMap<PathBuf, bool>) -> bool {
    let mut walked = vec![];
    let mut answer = false;
    for dir in path.ancestors().skip(1) {
        if let Some(known) = cache.get(dir) {
            answer = *known;
            break;
        }
        walked.push(dir.to_path_buf());
        if std::fs::symlink_metadata(dir.join(".git")).is_ok()
            || std::fs::symlink_metadata(dir.join(".hg")).is_ok()
            || std::fs::symlink_metadata(dir.join(".svn")).is_ok()
        {
            answer = true;
            break;
        }
    }
    for dir in walked {
        cache.insert(dir, answer);
    }
    answer
}

/// Whether a copy still looks as it did when compared: same size and the
/// same last change. Checked again just before anything moves.
pub fn unchanged(copy: &Copy, size: u64) -> bool {
    std::fs::metadata(&copy.path)
        .is_ok_and(|m| m.is_file() && m.len() == size && m.modified().ok() == copy.modified)
}

/// Duplicates of 1 MB or more across everything indexed, outside system
/// and app folders and outside projects. When the index has nothing (it doesn't cover the
/// person's folders, or it is still being built), `home` is walked instead.
pub fn find_everywhere(
    engine: Option<&seek_engine::Engine>,
    home: &Path,
    cancel: &AtomicBool,
    progress: impl Fn(usize, usize, &'static str) + Sync,
) -> Report {
    progress(0, 0, "Asking the index");
    let mut by_size: HashMap<u64, Listed> = HashMap::new();
    let mut report = Report::default();
    let mut projects = HashMap::new();
    if let Some(engine) = engine
        && let Ok(mut query) = seek_engine::Query::parse(&format!("size:>={EVERYWHERE_MIN} kind:file"), "")
    {
        query.limit = MAX_FILES;
        for found in engine.search(&query).unwrap_or_default() {
            if system_path(&found.path) || in_project(&found.path, &mut projects) {
                continue;
            }
            report.files += 1;
            // The index's time is to the second; read the real one so the
            // check before moving compares like with like.
            let modified = std::fs::metadata(&found.path).and_then(|m| m.modified()).ok();
            by_size.entry(found.size).or_default().push((found.path, modified));
        }
    }
    // The index may not cover the profile (only some locations are indexed,
    // or it is still being built): then walk the profile too, counting each
    // file once.
    let covered = engine.is_some_and(|e| e.has_directory(home));
    if report.files == 0 || !covered {
        let known: std::collections::HashSet<String> = by_size
            .values()
            .flatten()
            .map(|(p, _)| p.to_string_lossy().to_lowercase())
            .collect();
        progress(0, 0, "Listing your folders");
        let mut stack = vec![home.to_path_buf()];
        while let Some(dir) = stack.pop() {
            if cancel.load(Ordering::Relaxed) {
                report.stopped = Some("Stopped".into());
                return report;
            }
            let Ok(entries) = std::fs::read_dir(&dir) else { continue };
            for entry in entries.flatten() {
                let Ok(meta) = entry.metadata() else { continue };
                let attributes = meta.file_attributes();
                let path = entry.path();
                // Only below the walked folder: the profile itself may sit
                // under a folder with a system-looking name.
                let rel = path.strip_prefix(home).unwrap_or(&path);
                // A folder is checked by its own name too, so the walk never
                // enters app data, dot-folders or bundles.
                let skip = if meta.is_dir() { system_path(&rel.join("_")) } else { system_path(rel) };
                if attributes & REPARSE != 0 || skip {
                    continue;
                }
                if meta.is_dir() {
                    // A repository is a project: its copies stay out.
                    if std::fs::symlink_metadata(path.join(".git")).is_err() {
                        stack.push(path);
                    }
                } else if attributes & CLOUD == 0
                    && meta.len() >= EVERYWHERE_MIN
                    && !known.contains(&path.to_string_lossy().to_lowercase())
                {
                    report.files += 1;
                    if report.files > MAX_FILES {
                        report.stopped = Some(format!("More than {MAX_FILES} large files"));
                        break;
                    }
                    by_size.entry(meta.len()).or_default().push((path, meta.modified().ok()));
                }
            }
        }
    }
    confirm(by_size, report, cancel, &progress)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_identical_contents_group_and_hard_links_do_not_count() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join("a")).unwrap();
        // Two real copies, one same-size impostor that differs only in the
        // middle (past the sample's reach for a small file it is all read).
        let big: Vec<u8> = (0..400_000u32).map(|i| (i % 251) as u8).collect();
        let mut middle = big.clone();
        middle[200_000] ^= 1;
        std::fs::write(root.join("one.bin"), &big).unwrap();
        std::fs::write(root.join("a").join("two.bin"), &big).unwrap();
        std::fs::write(root.join("impostor.bin"), &middle).unwrap();
        let mut tail = big.clone();
        *tail.last_mut().unwrap() ^= 1;
        std::fs::write(root.join("tail.bin"), &tail).unwrap();
        // A hard link to one.bin is the same file, not a copy.
        std::fs::hard_link(root.join("one.bin"), root.join("link.bin")).unwrap();
        std::fs::write(root.join("empty1"), b"").unwrap();
        std::fs::write(root.join("empty2"), b"").unwrap();
        let report = find(root, &AtomicBool::new(false), |_, _, _| {});
        assert_eq!(report.stopped, None);
        assert_eq!(report.groups.len(), 1, "{:?}", report.groups);
        let group = &report.groups[0];
        assert_eq!(group.copies.len(), 2);
        assert_eq!(group.reclaimable(), 400_000);
        let names: Vec<_> = group
            .copies
            .iter()
            .map(|c| c.path.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        assert!(names.contains(&"two.bin".to_string()));
        assert!(!names.iter().any(|n| n == "impostor.bin" || n == "tail.bin"));
    }

    #[test]
    fn everywhere_walks_the_home_and_skips_app_data_and_small_files() {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path();
        let big: Vec<u8> = (0..(EVERYWHERE_MIN as u32 + 10)).map(|i| (i % 253) as u8).collect();
        for rel in [r"Documents\a.bin", r"Pictures\copy of a.bin", r"AppData\Local\c.bin"] {
            let path = home.join(rel);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, &big).unwrap();
        }
        std::fs::write(home.join("s1"), b"same").unwrap();
        std::fs::write(home.join("s2"), b"same").unwrap();
        let report = find_everywhere(None, home, &AtomicBool::new(false), |_, _, _| {});
        assert_eq!(report.groups.len(), 1, "{:?}", report.groups);
        assert_eq!(report.groups[0].copies.len(), 2);
        assert!(report.groups[0]
            .copies
            .iter()
            .all(|c| !c.path.ends_with("c.bin")));
        assert!(system_path(Path::new(r"C:\Program Files\x\y.dll")));
        assert!(!system_path(Path::new(r"C:\Users\me\Documents\y.pdf")));
    }

    /// Read-only: prints what an everywhere search finds in this profile.
    #[test]
    #[ignore]
    fn everywhere_on_this_pc() {
        let home = PathBuf::from(std::env::var_os("USERPROFILE").unwrap());
        let started = std::time::Instant::now();
        let report = find_everywhere(None, &home, &AtomicBool::new(false), |_, _, _| {});
        let bytes: u64 = report.groups.iter().map(Group::reclaimable).sum();
        println!(
            "{} large files, {} groups, {} MB to free, {} skipped, {:.1} s, stopped {:?}",
            report.files,
            report.groups.len(),
            bytes >> 20,
            report.skipped,
            started.elapsed().as_secs_f32(),
            report.stopped
        );
        for f in &report.folders {
            println!("  folders {} files {} MB
    {}
    {}", f.files, f.bytes >> 20, f.a.display(), f.b.display());
        }
        for g in report.groups.iter().take(8) {
            println!("  group {} x {} KB: {}", g.copies.len(), g.size >> 10, g.copies.iter().map(|c| c.path.display().to_string()).collect::<Vec<_>>().join(" | "));
        }
    }

    #[test]
    fn everywhere_leaves_projects_out() {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path();
        let big: Vec<u8> = (0..(EVERYWHERE_MIN as u32 + 10)).map(|i| (i % 251) as u8).collect();
        std::fs::create_dir_all(home.join(r"code\carbon\.git")).unwrap();
        std::fs::create_dir_all(home.join(r"code\blend\.git")).unwrap();
        for rel in [r"code\carbon\bin\ocr.exe", r"code\blend\bin\ocr.exe"] {
            let path = home.join(rel);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, &big).unwrap();
        }
        let report = find_everywhere(None, home, &AtomicBool::new(false), |_, _, _| {});
        assert!(report.groups.is_empty(), "{:?}", report.groups);
        let mut cache = HashMap::new();
        assert!(in_project(&home.join(r"code\carbon\bin\ocr.exe"), &mut cache));
        assert!(!in_project(&home.join(r"Documents\a.pdf"), &mut cache));
    }

    fn tree(root: &Path, files: &[(&str, &[u8])]) {
        for (rel, bytes) in files {
            let path = root.join(rel);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, bytes).unwrap();
        }
    }

    #[test]
    fn identical_folders_are_whole_matches_widened_to_the_top() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let big: Vec<u8> = (0..300_000u32).map(|i| (i % 249) as u8).collect();
        let files: &[(&str, &[u8])] = &[
            (r"shots\a.png", &big),
            (r"shots\b.png", &big[..200_000]),
            (r"shots\notes.txt", b"small"),
            (r"readme.md", b"hello"),
        ];
        tree(&root.join(r"one\project"), files);
        tree(&root.join(r"two\project copy"), files);
        // A folder with the same pictures but one different small file is
        // not a copy; nor is a folder and its own subfolder.
        tree(&root.join(r"three\shots"), &[(r"a.png", &big), (r"b.png", &big[..200_000]), (r"notes.txt", b"SMALL")]);
        std::fs::write(root.join(r"one\project\shots\inner.png"), &big[..250_000]).unwrap();
        std::fs::write(root.join(r"two\project copy\shots\inner.png"), &big[..250_000]).unwrap();
        tree(&root.join(r"one\project\shots\sub"), &[(r"a.png", &big)]);
        tree(&root.join(r"two\project copy\shots\sub"), &[(r"a.png", &big)]);
        let report = find(root, &AtomicBool::new(false), |_, _, _| {});
        assert_eq!(report.folders.len(), 1, "{:#?}", report.folders.iter().map(|f| (&f.a, &f.b)).collect::<Vec<_>>());
        let f = &report.folders[0];
        let names = [f.a.file_name().unwrap(), f.b.file_name().unwrap()];
        assert!(names.contains(&std::ffi::OsStr::new("project")), "{names:?}");
        assert!(names.contains(&std::ffi::OsStr::new("project copy")), "{names:?}");
        assert!(listing_unchanged(&f.a, &f.listing_a));
        std::fs::write(f.a.join("readme.md"), b"changed!").unwrap();
        assert!(!listing_unchanged(&f.a, &f.listing_a));
    }

    #[test]
    fn app_data_dot_folders_and_bundles_are_left_out() {
        assert!(system_path(Path::new(r"C:\Users\me\.codex\packages\a.exe")));
        assert!(system_path(Path::new(r"C:\Users\me\Videos\Clip.glint-studio\video.mp4")));
        assert!(!system_path(Path::new(r"C:\Users\me\Videos\Clip.mp4")));
        assert!(!system_path(Path::new(r"C:\Users\me\Downloads\.hidden-name.pdf")));
        assert!(!system_path(Path::new(r"C:\Users\me\Documents\Project v1.2\a.pdf")));
        assert!(bundle("library.photoslibrary"));
        assert!(!bundle("v1.2"));
    }

    #[test]
    fn clutter_is_left_out() {
        assert!(clutter(Path::new(r"C:\x\desktop.ini"), 0));
        assert!(clutter(Path::new(r"C:\x\Thumbs.db"), 0));
        assert!(clutter(Path::new(r"C:\x\any.bin"), 0x4));
        assert!(!clutter(Path::new(r"C:\x\photo.jpg"), 0x20));
    }

    #[test]
    fn keep_rules_pick_one_copy() {
        let old = SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(10);
        let new = SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(20);
        let group = Group {
            size: 1,
            copies: vec![
                Copy {
                    path: PathBuf::from(r"C:\long\path\x.txt"),
                    modified: Some(new),
                },
                Copy {
                    path: PathBuf::from(r"C:\x.txt"),
                    modified: Some(old),
                },
            ],
        };
        assert_eq!(keeper(&group, Keep::Oldest), 1);
        assert_eq!(keeper(&group, Keep::Newest), 0);
        assert_eq!(keeper(&group, Keep::ShortestPath), 1);
    }

    #[test]
    fn a_cancelled_search_says_so() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a"), b"x").unwrap();
        let report = find(dir.path(), &AtomicBool::new(true), |_, _, _| {});
        assert_eq!(report.stopped.as_deref(), Some("Stopped"));
    }
}
