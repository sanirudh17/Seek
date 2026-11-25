//! Rename the opened file object; never replace an existing destination.
use std::{
    fs::{File, OpenOptions},
    os::windows::{
        ffi::OsStrExt,
        fs::{MetadataExt, OpenOptionsExt},
        io::AsRawHandle,
    },
    path::Path,
};
use windows_sys::Win32::Storage::FileSystem::*;
fn error(message: impl Into<String>) -> windows::core::Error {
    windows::core::Error::new(windows::Win32::Foundation::E_FAIL, message.into())
}
pub(crate) fn stable_parent(path: &Path) -> Result<Vec<File>, windows::core::Error> {
    let mut held = vec![];
    for parent in path
        .ancestors()
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .filter(|path| !path.as_os_str().is_empty())
    {
        let file = OpenOptions::new()
            .read(true)
            .access_mode(FILE_READ_ATTRIBUTES)
            .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
            .custom_flags(
                FILE_FLAG_BACKUP_SEMANTICS
                    | FILE_FLAG_OPEN_REPARSE_POINT
                    | FILE_FLAG_OPEN_NO_RECALL,
            )
            .open(parent)
            .map_err(|e| {
                error(format!(
                    "Cannot protect rename parent {}: {e}",
                    parent.display()
                ))
            })?;
        let meta = file.metadata().map_err(|e| error(e.to_string()))?;
        if !meta.is_dir()
            || meta.file_attributes()
                & (FILE_ATTRIBUTE_REPARSE_POINT | FILE_ATTRIBUTE_OFFLINE | 0x40000 | 0x400000)
                != 0
        {
            return Err(error(
                "Rename through linked or offline folders is unavailable.",
            ));
        }
        held.push(file);
    }
    Ok(held)
}
pub fn rename(
    path: &Path,
    name: &str,
    journal: Option<&crate::operation_journal::Context>,
) -> windows::core::Result<()> {
    rename_inner(path, name, journal, None, None)
}
pub fn rename_guarded(
    path: &Path,
    name: &str,
    journal: &crate::operation_journal::Context,
    expected: &crate::operation_journal::Fingerprint,
) -> windows::core::Result<()> {
    rename_inner(path, name, Some(journal), Some(expected), None)
}
pub fn move_guarded(
    path: &Path,
    target: &Path,
    journal: &crate::operation_journal::Context,
    expected: &crate::operation_journal::Fingerprint,
) -> windows::core::Result<()> {
    let name = target
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| error("The original filename cannot be represented safely."))?;
    rename_inner(path, name, Some(journal), Some(expected), Some(target))
}
fn rename_inner(
    path: &Path,
    name: &str,
    journal: Option<&crate::operation_journal::Context>,
    expected: Option<&crate::operation_journal::Fingerprint>,
    destination: Option<&Path>,
) -> windows::core::Result<()> {
    if !crate::file_ops::valid_name(name) {
        return Err(error("Choose a valid Windows file name."));
    }
    if !path.is_absolute() {
        return Err(error("Rename requires an absolute file path."));
    }
    let parent = path
        .parent()
        .ok_or_else(|| error("A volume root cannot be renamed."))?;
    let _parents = stable_parent(parent)?;
    let target = destination
        .map(Path::to_path_buf)
        .unwrap_or_else(|| parent.join(name));
    if !target.is_absolute() {
        return Err(error("Move requires an absolute destination."));
    }
    let target_parent = target
        .parent()
        .ok_or_else(|| error("A volume root cannot be replaced."))?;
    let _target_parents = if target_parent != parent {
        stable_parent(target_parent)?
    } else {
        vec![]
    };
    let action = if destination.is_some() {
        "move"
    } else {
        "rename"
    };
    let file = OpenOptions::new()
        .read(true)
        .access_mode(
            0x00010000 | FILE_READ_ATTRIBUTES | if expected.is_some() { 0x80000000 } else { 0 },
        )
        .share_mode(
            FILE_SHARE_READ
                | if expected.is_some() {
                    0
                } else {
                    FILE_SHARE_WRITE
                },
        )
        .custom_flags(
            FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_OPEN_NO_RECALL,
        )
        .open(path)
        .map_err(|e| error(format!("Cannot rename this item: {e}")))?;
    let meta = file.metadata().map_err(|e| error(e.to_string()))?;
    if meta.file_attributes()
        & (FILE_ATTRIBUTE_REPARSE_POINT | FILE_ATTRIBUTE_OFFLINE | 0x40000 | 0x400000)
        != 0
    {
        return Err(error("Rename of linked or offline items is unavailable."));
    }
    if path == target {
        return Ok(());
    }
    if let Some(expected) = expected {
        let current = crate::operation_journal::Fingerprint::read(&file).map_err(error)?;
        if !current.compatible(expected) {
            return Err(error(
                "This file changed after the operation. Undo was stopped without changing it.",
            ));
        }
    }
    let wide: Vec<u16> = target.as_os_str().encode_wide().collect();
    let offset = std::mem::offset_of!(FILE_RENAME_INFO, FileName);
    let bytes = offset
        .checked_add(
            wide.len()
                .checked_add(1)
                .and_then(|n| n.checked_mul(2))
                .ok_or_else(|| error("Rename path is too long"))?,
        )
        .ok_or_else(|| error("Rename path is too long"))?;
    let length: u32 = bytes
        .try_into()
        .map_err(|_| error("Rename path is too long"))?;
    // usize storage gives the flexible FILE_RENAME_INFO header its native alignment.
    let mut storage = vec![
        0usize;
        bytes
            .max(std::mem::size_of::<FILE_RENAME_INFO>())
            .div_ceil(std::mem::size_of::<usize>())
    ];
    let raw = storage.as_mut_ptr().cast::<FILE_RENAME_INFO>();
    unsafe {
        // FileNameLength excludes the terminating NUL, but Win32's DOS-path
        // conversion still requires it. Keep an explicit zero WCHAR in storage.
        (*raw).FileNameLength = (wide.len() * 2) as u32;
        std::ptr::copy_nonoverlapping(
            wide.as_ptr(),
            storage.as_mut_ptr().cast::<u8>().add(offset).cast(),
            wide.len(),
        );
    }
    if let Some(journal) = journal {
        journal.append(crate::operation_journal::Event::Before {
            job: journal.job,
            action: action.into(),
            source: Some(crate::operation_journal::WirePath::new(path)),
            folder: Some(crate::operation_journal::WirePath::new(target_parent)),
            name: Some(name.encode_utf16().collect()),
            proof: false,
        })?;
    }
    // The zeroed ReplaceIfExists member is FALSE. The kernel checks collision as
    // part of renaming this handle, including a destination created after UI validation.
    let success = unsafe {
        SetFileInformationByHandle(file.as_raw_handle(), FileRenameInfo, raw.cast(), length)
    } != 0;
    let failure = (!success).then(||{
        let original=windows::core::Error::from_thread();
        if matches!(original.code().0 as u32,0x800700b7|0x80070050){windows::core::Error::new(original.code(),format!("A file named {name:?} already exists. Choose another name. The original file was not {}.",if action == "move" {"moved"} else {"renamed"}))}else{original}
    });
    let identity = if success {
        let key = crate::operation_journal::Fingerprint::key(&file).ok();
        OpenOptions::new()
            .read(true)
            .share_mode(FILE_SHARE_READ | FILE_SHARE_DELETE)
            .custom_flags(
                FILE_FLAG_BACKUP_SEMANTICS
                    | FILE_FLAG_OPEN_REPARSE_POINT
                    | FILE_FLAG_OPEN_NO_RECALL,
            )
            .open(&target)
            .ok()
            .and_then(|reader| crate::operation_journal::Fingerprint::read(&reader).ok())
            .filter(|identity| Some((identity.volume, identity.id)) == key)
    } else {
        None
    };
    if let Some(journal) = journal {
        journal.append(crate::operation_journal::Event::After {
            job: journal.job,
            action: action.into(),
            source: Some(crate::operation_journal::WirePath::new(path)),
            target: success.then(|| crate::operation_journal::WirePath::new(&target)),
            parsing_name: None,
            hresult: failure.as_ref().map_or(0, |e| e.code().0),
            identity,
        })?;
    }
    if let Some(error) = failure {
        return Err(error);
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rename_path_buffer_is_terminated_at_every_alignment() {
        let temp = tempfile::tempdir().unwrap();
        for n in 1..=16 {
            let source = temp.path().join("source.txt");
            std::fs::write(&source, b"alignment verification").unwrap();
            let name = format!("{}.txt", "n".repeat(n));
            rename(&source, &name, None).unwrap();
            assert_eq!(
                std::fs::read(temp.path().join(&name)).unwrap(),
                b"alignment verification"
            );
            assert!(!source.exists());
        }
    }
    #[test]
    fn rename_collision_never_overwrites_either_file() {
        let t = tempfile::tempdir().unwrap();
        let a = t.path().join("a.txt");
        let b = t.path().join("b.txt");
        std::fs::write(&a, b"original").unwrap();
        std::fs::write(&b, b"existing").unwrap();
        assert!(rename(&a, "b.txt", None).is_err());
        assert_eq!(std::fs::read(&a).unwrap(), b"original");
        assert_eq!(std::fs::read(&b).unwrap(), b"existing");
    }
    #[test]
    fn rename_file_folder_and_case_only_preserve_contents() {
        let t = tempfile::tempdir().unwrap();
        let a = t.path().join("a.txt");
        std::fs::write(&a, b"original").unwrap();
        rename(&a, "A.txt", None).unwrap();
        let a = t.path().join("A.txt");
        rename(&a, "renamed.txt", None).unwrap();
        assert_eq!(
            std::fs::read(t.path().join("renamed.txt")).unwrap(),
            b"original"
        );
        let d = t.path().join("Folder");
        std::fs::create_dir(&d).unwrap();
        std::fs::write(d.join("nested"), b"inside").unwrap();
        rename(&d, "Renamed folder", None).unwrap();
        assert_eq!(
            std::fs::read(t.path().join("Renamed folder/nested")).unwrap(),
            b"inside"
        );
    }
    #[test]
    fn extended_long_paths_and_unicode_names_are_supported() {
        let temp = tempfile::tempdir().unwrap();
        let mut root = std::fs::canonicalize(temp.path()).unwrap();
        for _ in 0..14 {
            root = root.join("Long directory segment");
        }
        std::fs::create_dir_all(&root).unwrap();
        let original = root.join("caf\u{e9}.txt");
        std::fs::write(&original, b"long path data").unwrap();
        rename(&original, "R\u{e9}sum\u{e9}.txt", None).unwrap();
        assert_eq!(
            std::fs::read(root.join("R\u{e9}sum\u{e9}.txt")).unwrap(),
            b"long path data"
        );
    }
    #[test]
    fn linked_parent_cannot_redirect_rename_to_its_target() {
        use std::os::windows::process::CommandExt;
        let temp = tempfile::tempdir().unwrap();
        let target = temp.path().join("target");
        std::fs::create_dir(&target).unwrap();
        std::fs::write(target.join("original.txt"), b"keep target").unwrap();
        let link = temp.path().join("linked");
        let output = std::process::Command::new("cmd.exe")
            .args(["/d", "/c", "mklink", "/J"])
            .arg(&link)
            .arg(&target)
            .creation_flags(0x08000000)
            .output()
            .unwrap();
        if !output.status.success() {
            eprintln!(
                "Junction rename acceptance unverified: {}",
                String::from_utf8_lossy(&output.stderr)
            );
            return;
        }
        assert!(rename(&link.join("original.txt"), "renamed.txt", None).is_err());
        assert_eq!(
            std::fs::read(target.join("original.txt")).unwrap(),
            b"keep target"
        );
        assert!(!target.join("renamed.txt").exists());
        std::fs::remove_dir(link).unwrap();
    }
    #[test]
    fn leading_space_is_a_distinct_name_but_exact_collision_is_refused() {
        let temp = tempfile::tempdir().unwrap();
        let original = temp.path().join("a.txt");
        let existing = temp.path().join("existing.txt");
        std::fs::write(&original, b"original").unwrap();
        std::fs::write(&existing, b"existing").unwrap();
        let error = rename(&original, "existing.txt", None)
            .unwrap_err()
            .to_string();
        assert!(error.contains("already exists"));
        assert!(error.contains("original file was not renamed"));
        rename(&original, " existing.txt", None).unwrap();
        assert_eq!(
            std::fs::read(temp.path().join(" existing.txt")).unwrap(),
            b"original"
        );
        assert_eq!(std::fs::read(existing).unwrap(), b"existing");
    }
    #[test]
    fn journal_failure_stops_rename_before_mutation() {
        use crate::operation_journal::{Context, Journal};
        let t = tempfile::tempdir().unwrap();
        let p = t.path().join("a.txt");
        std::fs::write(&p, b"original").unwrap();
        let journal = Context {
            journal: std::sync::Arc::new(std::sync::Mutex::new(
                Journal::open(&t.path().join("log")).unwrap(),
            )),
            job: 1,
        };
        assert!(rename(&p, "renamed.txt", Some(&journal)).is_err());
        assert_eq!(std::fs::read(&p).unwrap(), b"original");
        assert!(!t.path().join("renamed.txt").exists());
    }
}
