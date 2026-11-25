//! ZIP archives (Phase 8.6): browse read-only like a folder, extract, and
//! compress. Archive contents are untrusted: names that escape the
//! destination (traversal, absolute paths, drive prefixes) are skipped, links
//! are never written or followed, sizes and counts are capped, and output is
//! created new, so nothing existing is ever replaced.
use seek_engine::{Found, walk};
use std::{
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
};

/// Most entries an archive may list or extract.
pub const MAX_ENTRIES: usize = 100_000;
/// Most bytes one extraction may write.
pub const MAX_BYTES: u64 = 8 << 30;

fn is_zip(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("zip"))
}

/// `C:\a\b.zip\c\d` → (`C:\a\b.zip`, `c/d`). `None` when no ancestor is a
/// ZIP file. The archive itself gives an empty inner path.
pub fn split(path: &Path) -> Option<(PathBuf, String)> {
    let mut archive = PathBuf::new();
    let mut inner: Vec<String> = vec![];
    let mut found = false;
    for component in path.components() {
        if found {
            inner.push(component.as_os_str().to_string_lossy().into_owned());
            continue;
        }
        archive.push(component);
        if is_zip(&archive) && std::fs::metadata(&archive).is_ok_and(|m| m.is_file()) {
            found = true;
        }
    }
    found.then(|| (archive, inner.join("/")))
}

/// Inside an archive (not the archive file itself): read-only.
pub fn inside(path: &Path) -> bool {
    split(path).is_some_and(|(_, inner)| !inner.is_empty())
}

fn open(zip: &Path) -> Result<zip::ZipArchive<std::fs::File>, String> {
    let file = std::fs::File::open(zip).map_err(|e| e.to_string())?;
    let archive = zip::ZipArchive::new(file).map_err(|_| "This ZIP file can't be read".to_string())?;
    if archive.len() > MAX_ENTRIES {
        return Err(format!(
            "This ZIP file lists more than {MAX_ENTRIES} items"
        ));
    }
    Ok(archive)
}

fn unix_time(date: Option<zip::DateTime>) -> u32 {
    date.and_then(|d| {
        chrono::NaiveDate::from_ymd_opt(d.year() as i32, d.month() as u32, d.day() as u32)?
            .and_hms_opt(d.hour() as u32, d.minute() as u32, d.second() as u32)
    })
    .map_or(0, |t| t.and_utc().timestamp().clamp(0, u32::MAX as i64) as u32)
}

/// A safe relative name, with `/` separators, or `None` for names that would
/// escape (`..`, absolute, drive letters) or are empty.
fn safe_name(raw: &str) -> Option<String> {
    let raw = raw.replace('\\', "/");
    if raw.starts_with('/') || raw.contains(':') {
        return None;
    }
    let mut parts = vec![];
    for part in raw.split('/') {
        match part {
            "" | "." => {}
            ".." => return None,
            part => {
                if !crate::file_ops::valid_name(part) {
                    return None;
                }
                parts.push(part)
            }
        }
    }
    (!parts.is_empty()).then(|| parts.join("/"))
}

fn is_link(entry: &zip::read::ZipFile<'_>) -> bool {
    entry.unix_mode().is_some_and(|mode| mode & 0o170000 == 0o120000)
}

/// The children of `inner` inside the archive, as rows for a folder view.
pub fn list(zip: &Path, inner: &str) -> Result<Vec<Found>, String> {
    let mut archive = open(zip)?;
    let prefix = if inner.is_empty() {
        String::new()
    } else {
        format!("{inner}/")
    };
    let mut seen = std::collections::HashMap::<String, Found>::new();
    for index in 0..archive.len() {
        let entry = archive.by_index_raw(index).map_err(|e| e.to_string())?;
        let Some(name) = safe_name(entry.name()) else {
            continue;
        };
        let Some(rest) = name.strip_prefix(&prefix) else {
            continue;
        };
        let (child, deeper) = match rest.split_once('/') {
            Some((child, _)) => (child, true),
            None => (rest, entry.is_dir()),
        };
        if child.is_empty() || (!deeper && is_link(&entry)) {
            continue;
        }
        let key = child.to_lowercase();
        let path = zip.join(inner.replace('/', "\\")).join(child);
        let row = Found {
            path,
            kind: if deeper { walk::KIND_DIR } else { walk::KIND_FILE },
            size: if deeper { 0 } else { entry.size() },
            mtime: unix_time(entry.last_modified()),
            score: 0,
        };
        seen.entry(key).or_insert(row);
    }
    if !inner.is_empty() && seen.is_empty() {
        return Err("This folder isn't in the ZIP file".into());
    }
    Ok(seen.into_values().collect())
}

/// `name`, `name 2`, `name 3`… in `folder`, whichever is free.
fn free(folder: &Path, stem: &str, extension: &str) -> PathBuf {
    (1..10_000)
        .map(|n| {
            let name = if n == 1 {
                format!("{stem}{extension}")
            } else {
                format!("{stem} {n}{extension}")
            };
            folder.join(name)
        })
        .find(|path| std::fs::symlink_metadata(path).is_err())
        .unwrap_or_else(|| folder.join(format!("{stem} copy{extension}")))
}

fn copy_limited(
    from: &mut impl Read,
    to: &mut impl Write,
    limit: u64,
    cancel: &AtomicBool,
) -> Result<u64, String> {
    let mut buffer = vec![0u8; 256 * 1024];
    let mut total = 0u64;
    loop {
        if cancel.load(Ordering::Relaxed) {
            return Err("Stopped".into());
        }
        let n = from.read(&mut buffer).map_err(|_| "The ZIP file is damaged".to_string())?;
        if n == 0 {
            return Ok(total);
        }
        total += n as u64;
        if total > limit {
            return Err("The ZIP file unpacks to more than the size limit".into());
        }
        to.write_all(&buffer[..n]).map_err(|e| e.to_string())?;
    }
}

#[derive(Debug, Default)]
pub struct Extracted {
    pub folder: PathBuf,
    pub files: usize,
    /// Entries left out: unsafe names or links.
    pub skipped: usize,
}

/// Extracts the whole archive into a new folder beside it (named after the
/// archive, numbered if taken). On failure the partial folder stays, named,
/// so nothing is lost silently; the caller reports it.
pub fn extract(zip: &Path, cancel: &AtomicBool) -> Result<Extracted, String> {
    let mut archive = open(zip)?;
    let declared: u64 = (0..archive.len())
        .filter_map(|i| archive.by_index_raw(i).ok().map(|e| e.size()))
        .sum();
    if declared > MAX_BYTES {
        return Err("This ZIP file unpacks to more than 8 GB".into());
    }
    let parent = zip.parent().ok_or("No folder for the extracted files")?;
    let stem = zip
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "Archive".into());
    let folder = free(parent, &stem, "");
    std::fs::create_dir(&folder).map_err(|e| e.to_string())?;
    let mut done = Extracted {
        folder: folder.clone(),
        ..Default::default()
    };
    let mut budget = MAX_BYTES;
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).map_err(|_| "The ZIP file is damaged".to_string())?;
        let Some(name) = safe_name(entry.name()) else {
            done.skipped += 1;
            continue;
        };
        if is_link(&entry) {
            done.skipped += 1;
            continue;
        }
        let target = folder.join(name.replace('/', "\\"));
        if entry.is_dir() {
            std::fs::create_dir_all(&target).map_err(|e| e.to_string())?;
            continue;
        }
        if let Some(dir) = target.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        // Every directory on the way was made by this extraction, inside the
        // new folder; create_new refuses anything already there.
        let mut file = match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&target)
        {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                done.skipped += 1;
                continue;
            }
            Err(error) => return Err(error.to_string()),
        };
        let written = copy_limited(&mut entry, &mut file, budget, cancel);
        match written {
            Ok(n) => budget -= n,
            Err(error) => {
                drop(file);
                let _ = std::fs::remove_file(&target);
                return Err(error);
            }
        }
        done.files += 1;
    }
    Ok(done)
}

/// Extracts one file to a private temporary folder, for opening it.
pub fn extract_one(path: &Path) -> Result<PathBuf, String> {
    let (zip, inner) = split(path).ok_or("Not inside a ZIP file")?;
    let mut archive = open(&zip)?;
    let index = (0..archive.len())
        .find(|i| {
            archive
                .by_index_raw(*i)
                .ok()
                .and_then(|e| safe_name(e.name()))
                .is_some_and(|name| name.eq_ignore_ascii_case(&inner))
        })
        .ok_or("This file isn't in the ZIP file")?;
    let mut entry = archive.by_index(index).map_err(|_| "The ZIP file is damaged".to_string())?;
    if is_link(&entry) {
        return Err("Links inside ZIP files are not opened".into());
    }
    use std::hash::{Hash, Hasher};
    let mut hash = std::collections::hash_map::DefaultHasher::new();
    zip.hash(&mut hash);
    let folder = std::env::temp_dir()
        .join("Seek")
        .join("archive-open")
        .join(format!("{:016x}", hash.finish()));
    std::fs::create_dir_all(&folder).map_err(|e| e.to_string())?;
    let name = inner.rsplit('/').next().unwrap_or(&inner);
    let target = folder.join(name);
    let mut file = std::fs::File::create(&target).map_err(|e| e.to_string())?;
    copy_limited(&mut entry, &mut file, MAX_BYTES, &AtomicBool::new(false))?;
    Ok(target)
}

fn is_reparse(metadata: &std::fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    metadata.file_attributes() & 0x400 != 0 || metadata.file_type().is_symlink()
}

/// Compresses `paths` (from one folder) into a new ZIP beside them: the one
/// item's name, or "Archive" for several. Links and junctions are skipped,
/// never followed. A failed write removes the partial archive.
pub fn compress(paths: &[PathBuf], cancel: &AtomicBool) -> Result<(PathBuf, usize), String> {
    let first = paths.first().ok_or("Nothing to compress")?;
    let folder = first.parent().ok_or("No folder for the archive")?;
    let stem = if paths.len() == 1 {
        first
            .file_stem()
            .filter(|_| first.is_file())
            .or(first.file_name())
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "Archive".into())
    } else {
        "Archive".into()
    };
    let target = free(folder, &stem, ".zip");
    let file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&target)
        .map_err(|e| e.to_string())?;
    let result = (|| {
        let mut writer = zip::ZipWriter::new(std::io::BufWriter::new(file));
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated)
            .large_file(true);
        let mut stack: Vec<(PathBuf, String)> = paths
            .iter()
            .filter_map(|p| Some((p.clone(), p.file_name()?.to_string_lossy().into_owned())))
            .collect();
        let mut count = 0usize;
        while let Some((path, name)) = stack.pop() {
            if cancel.load(Ordering::Relaxed) {
                return Err("Stopped".to_string());
            }
            let Ok(metadata) = std::fs::symlink_metadata(&path) else {
                continue;
            };
            if is_reparse(&metadata) || path == target {
                continue;
            }
            count += 1;
            if count > MAX_ENTRIES {
                return Err(format!("More than {MAX_ENTRIES} items to compress"));
            }
            if metadata.is_dir() {
                writer
                    .add_directory(format!("{name}/"), options)
                    .map_err(|e| e.to_string())?;
                for child in std::fs::read_dir(&path).map_err(|e| e.to_string())?.flatten() {
                    let child_name = child.file_name().to_string_lossy().into_owned();
                    stack.push((child.path(), format!("{name}/{child_name}")));
                }
            } else {
                writer.start_file(&name, options).map_err(|e| e.to_string())?;
                let mut source = std::fs::File::open(&path).map_err(|e| e.to_string())?;
                copy_limited(&mut source, &mut writer, u64::MAX, cancel)?;
            }
        }
        writer
            .finish()
            .map_err(|e| e.to_string())?
            .flush()
            .map_err(|e| e.to_string())?;
        Ok(count)
    })();
    match result {
        Ok(count) => Ok((target, count)),
        Err(error) => {
            let _ = std::fs::remove_file(&target);
            Err(error)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_zip(path: &Path, entries: &[(&str, &[u8])]) {
        let file = std::fs::File::create(path).unwrap();
        let mut writer = zip::ZipWriter::new(file);
        for (name, bytes) in entries {
            if name.ends_with('/') {
                writer
                    .add_directory(*name, zip::write::SimpleFileOptions::default())
                    .unwrap();
            } else {
                writer
                    .start_file(*name, zip::write::SimpleFileOptions::default())
                    .unwrap();
                writer.write_all(bytes).unwrap();
            }
        }
        writer.finish().unwrap();
    }

    #[test]
    fn archives_browse_like_folders() {
        let dir = tempfile::tempdir().unwrap();
        let zip = dir.path().join("photos.zip");
        make_zip(
            &zip,
            &[("a.txt", b"one"), ("sub/b.txt", b"two"), ("sub/deep/c.txt", b"3"), ("empty/", b"")],
        );
        let (archive, inner) = split(&zip.join("sub").join("deep")).unwrap();
        assert_eq!((archive.as_path(), inner.as_str()), (zip.as_path(), "sub/deep"));
        assert!(inside(&zip.join("sub")));
        assert!(!inside(&zip));
        assert!(split(&dir.path().join("plain")).is_none());
        let mut root = list(&zip, "").unwrap();
        root.sort_by(|a, b| a.path.cmp(&b.path));
        let names: Vec<_> = root
            .iter()
            .map(|r| (r.path.file_name().unwrap().to_string_lossy().into_owned(), r.kind))
            .collect();
        assert_eq!(
            names,
            [
                ("a.txt".into(), walk::KIND_FILE),
                ("empty".into(), walk::KIND_DIR),
                ("sub".into(), walk::KIND_DIR)
            ]
        );
        let sub = list(&zip, "sub").unwrap();
        assert_eq!(sub.len(), 2);
        assert!(list(&zip, "missing").is_err());
        let opened = extract_one(&zip.join("sub").join("b.txt")).unwrap();
        assert_eq!(std::fs::read(opened).unwrap(), b"two");
    }

    #[test]
    fn file_operations_refuse_paths_inside_an_archive() {
        use crate::file_ops::{Operation, touches_archive};
        let dir = tempfile::tempdir().unwrap();
        let zip = dir.path().join("a.zip");
        make_zip(&zip, &[("x.txt", b"x")]);
        let inner = zip.join("x.txt");
        assert!(touches_archive(&Operation::Rename { path: inner.clone(), name: "y.txt".into() }));
        assert!(touches_archive(&Operation::Recycle { paths: vec![inner.clone()] }));
        assert!(touches_archive(&Operation::Copy { paths: vec![dir.path().join("b")], folder: zip.join("sub") }));
        assert!(touches_archive(&Operation::NewFolder { folder: zip.join("sub"), name: "n".into() }));
        // The ZIP file itself is an ordinary file.
        assert!(!touches_archive(&Operation::Recycle { paths: vec![zip.clone()] }));
        assert!(!touches_archive(&Operation::Rename { path: zip, name: "b.zip".into() }));
    }

    #[test]
    fn extraction_rejects_escapes_and_never_overwrites() {
        let dir = tempfile::tempdir().unwrap();
        let zip = dir.path().join("pack.zip");
        make_zip(
            &zip,
            &[
                ("ok.txt", b"fine"),
                ("../escape.txt", b"bad"),
                ("C:/abs.txt", b"bad"),
                ("dir/../../up.txt", b"bad"),
                ("CON", b"bad"),
            ],
        );
        // The obvious folder name is taken: extraction picks a new one.
        std::fs::create_dir(dir.path().join("pack")).unwrap();
        std::fs::write(dir.path().join("pack").join("ok.txt"), b"keep").unwrap();
        let done = extract(&zip, &AtomicBool::new(false)).unwrap();
        assert_eq!(done.folder, dir.path().join("pack 2"));
        assert_eq!(done.files, 1);
        assert_eq!(done.skipped, 4);
        assert_eq!(std::fs::read(done.folder.join("ok.txt")).unwrap(), b"fine");
        assert_eq!(std::fs::read(dir.path().join("pack").join("ok.txt")).unwrap(), b"keep");
        assert!(!dir.path().join("escape.txt").exists());
        assert!(!dir.path().join("up.txt").exists());
        // Listing never shows unsafe names either.
        assert_eq!(list(&zip, "").unwrap().len(), 1);
    }

    #[test]
    fn compress_round_trips_and_skips_nothing_it_should_keep() {
        let dir = tempfile::tempdir().unwrap();
        let folder = dir.path().join("notes");
        std::fs::create_dir_all(folder.join("inner")).unwrap();
        std::fs::write(folder.join("a.md"), b"# A").unwrap();
        std::fs::write(folder.join("inner").join("b.txt"), b"B").unwrap();
        std::fs::write(dir.path().join("notes.zip"), b"existing").unwrap();
        let (zip, count) = compress(std::slice::from_ref(&folder), &AtomicBool::new(false)).unwrap();
        assert_eq!(zip, dir.path().join("notes 2.zip"), "never replaces a file");
        assert_eq!(std::fs::read(dir.path().join("notes.zip")).unwrap(), b"existing");
        assert_eq!(count, 4);
        let entries = list(&zip, "notes").unwrap();
        assert_eq!(entries.len(), 2);
        // A stopped compression leaves no partial archive.
        let stopped = compress(&[folder], &AtomicBool::new(true));
        assert!(stopped.is_err());
        assert!(!dir.path().join("notes 3.zip").exists());
    }
}
