//! Read-only, cancellable selection totals. No content reads or link traversal.
use std::{
    collections::BTreeMap,
    os::windows::fs::{MetadataExt, OpenOptionsExt},
    path::PathBuf,
    sync::atomic::{AtomicBool, Ordering},
};
use windows_sys::Win32::Storage::FileSystem::*;

#[derive(Clone, Default, Debug)]
pub struct Summary {
    pub selected: usize,
    pub selected_files: usize,
    pub selected_folders: usize,
    pub files: usize,
    pub folders: usize,
    pub bytes: u64,
    pub disk: u64,
    pub unknown_disk: usize,
    pub skipped: usize,
    pub errors: usize,
    pub cancelled: bool,
    pub kinds: BTreeMap<String, usize>,
}
impl Summary {
    pub fn complete(&self) -> bool {
        !self.cancelled && self.errors == 0 && self.skipped == 0
    }
}
fn blocked(flags: u32) -> bool {
    flags & (FILE_ATTRIBUTE_REPARSE_POINT | FILE_ATTRIBUTE_OFFLINE | 0x40000 | 0x400000) != 0
}
fn add(total: &mut u64, value: u64, errors: &mut usize) {
    match total.checked_add(value) {
        Some(next) => *total = next,
        None => *errors += 1,
    }
}
pub fn scan(paths: &[PathBuf], cancel: &AtomicBool) -> Summary {
    let mut out = Summary {
        selected: paths.len(),
        ..Default::default()
    };
    // Collapse nested selected paths so a parent plus one of its descendants counts once.
    let mut roots: Vec<PathBuf> = Vec::new();
    for path in paths {
        if cancel.load(Ordering::Relaxed) {
            out.cancelled = true;
            return out;
        }
        match std::fs::symlink_metadata(path) {
            Ok(meta) if meta.is_dir() => out.selected_folders += 1,
            Ok(meta) if meta.is_file() => {
                out.selected_files += 1;
                let kind = path
                    .extension()
                    .map(|e| e.to_string_lossy().to_ascii_uppercase())
                    .filter(|e| !e.is_empty())
                    .unwrap_or_else(|| "File".into());
                *out.kinds.entry(kind).or_default() += 1;
            }
            _ => {}
        }
        if roots
            .iter()
            .any(|root| crate::file_changes::under_any(path, std::slice::from_ref(root)))
        {
            continue;
        }
        roots.retain(|root| !crate::file_changes::under_any(root, std::slice::from_ref(path)));
        roots.push(path.clone());
    }
    // Retain each ancestor's non-delete/non-write-shared handle while visiting children.
    // This prevents a rename/reparse replacement between validation and read_dir.
    // Enumerators also bound memory by depth rather than materializing a whole tree.
    let mut frames: Vec<(std::fs::ReadDir, std::fs::File)> = Vec::new();
    loop {
        if cancel.load(Ordering::Relaxed) {
            out.cancelled = true;
            break;
        }
        let path = if let Some((entries, _guard)) = frames.last_mut() {
            match entries.next() {
                Some(Ok(entry)) => entry.path(),
                Some(Err(_)) => {
                    out.errors += 1;
                    continue;
                }
                None => {
                    frames.pop();
                    continue;
                }
            }
        } else if let Some(root) = roots.pop() {
            root
        } else {
            break;
        };
        let Ok(meta) = std::fs::symlink_metadata(&path) else {
            out.errors += 1;
            continue;
        };
        if blocked(meta.file_attributes()) {
            out.skipped += 1;
            continue;
        }
        if meta.is_file() {
            out.files += 1;
            add(&mut out.bytes, meta.len(), &mut out.errors);
            if let Some(bytes) = crate::file_info::allocation_size(&path, meta.len()) {
                add(&mut out.disk, bytes, &mut out.errors);
            } else {
                out.unknown_disk += 1;
            }
        } else if meta.is_dir() {
            let guard = match std::fs::OpenOptions::new()
                .read(true)
                .share_mode(FILE_SHARE_READ)
                .custom_flags(
                    FILE_FLAG_OPEN_REPARSE_POINT
                        | FILE_FLAG_BACKUP_SEMANTICS
                        | FILE_FLAG_OPEN_NO_RECALL,
                )
                .open(&path)
            {
                Ok(guard) => guard,
                Err(_) => {
                    out.errors += 1;
                    continue;
                }
            };
            if !guard
                .metadata()
                .is_ok_and(|m| m.is_dir() && !blocked(m.file_attributes()))
            {
                out.skipped += 1;
                continue;
            }
            let entries = match std::fs::read_dir(&path) {
                Ok(entries) => entries,
                Err(_) => {
                    out.errors += 1;
                    continue;
                }
            };
            out.folders += 1;
            frames.push((entries, guard));
        } else {
            out.skipped += 1;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn totals_include_nested_files_once_and_keep_unknowns_explicit() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("root");
        let sub = root.join("sub");
        std::fs::create_dir_all(&sub).unwrap();
        let a = root.join("a.txt");
        let b = sub.join("b.txt");
        std::fs::write(&a, b"abc").unwrap();
        std::fs::write(&b, b"12345").unwrap();
        let out = scan(&[a.clone(), root.clone(), sub, b], &AtomicBool::new(false));
        assert!(out.complete());
        assert_eq!(out.files, 2);
        assert_eq!(out.folders, 2);
        assert_eq!(out.bytes, 8);
        assert_eq!(out.selected, 4);
        assert_eq!(out.selected_files, 2);
        assert_eq!(out.selected_folders, 2);
        assert_eq!(std::fs::read(a).unwrap(), b"abc");
        let out = scan(&[root.join("missing")], &AtomicBool::new(false));
        assert!(!out.complete());
        assert_eq!(out.errors, 1);
    }
    #[test]
    fn junction_cycles_are_skipped_without_reading_their_target() {
        use std::os::windows::process::CommandExt;
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("owned");
        std::fs::create_dir(&root).unwrap();
        std::fs::write(root.join("a.txt"), b"abc").unwrap();
        let link = root.join("loop");
        let output = std::process::Command::new("cmd.exe")
            .args(["/d", "/c", "mklink", "/J"])
            .arg(&link)
            .arg(&root)
            .creation_flags(0x08000000)
            .output()
            .unwrap();
        if !output.status.success() {
            eprintln!(
                "Junction acceptance unverified: {}",
                String::from_utf8_lossy(&output.stderr)
            );
            return;
        }
        let summary = scan(std::slice::from_ref(&root), &AtomicBool::new(false));
        assert_eq!(summary.files, 1);
        assert_eq!(summary.bytes, 3);
        assert_eq!(summary.skipped, 1);
        assert!(!summary.complete());
        std::fs::remove_dir(link).unwrap();
    }
    #[test]
    fn cancelled_requests_do_no_traversal() {
        let temp = tempfile::tempdir().unwrap();
        let out = scan(&[temp.path().to_path_buf()], &AtomicBool::new(true));
        assert!(out.cancelled);
        assert_eq!(out.files + out.folders, 0);
    }
    #[test]
    fn reparse_and_cloud_flags_are_never_scanned() {
        for flag in [
            FILE_ATTRIBUTE_REPARSE_POINT,
            FILE_ATTRIBUTE_OFFLINE,
            0x40000,
            0x400000,
        ] {
            assert!(blocked(flag));
        }
        assert!(!blocked(FILE_ATTRIBUTE_ARCHIVE));
    }
}
