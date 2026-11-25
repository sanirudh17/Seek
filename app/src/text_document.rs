//! Complete bounded text snapshots and recoverable atomic saves. Never save an excerpt.
use std::{
    fs::{File, OpenOptions},
    io::{Read, Write},
    os::windows::fs::{MetadataExt, OpenOptionsExt},
    path::{Path, PathBuf},
    sync::Arc,
};
use windows_sys::Win32::Storage::FileSystem::*;
pub const LIMIT: usize = 2 * 1024 * 1024;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Encoding {
    Utf8(bool),
    Utf16(bool),
    Utf32(bool),
}
impl Encoding {
    pub fn label(self) -> &'static str {
        match self {
            Self::Utf8(false) => "UTF-8",
            Self::Utf8(true) => "UTF-8 BOM",
            Self::Utf16(true) => "UTF-16 LE",
            Self::Utf16(false) => "UTF-16 BE",
            Self::Utf32(true) => "UTF-32 LE",
            Self::Utf32(false) => "UTF-32 BE",
        }
    }
    fn encode(self, text: &str) -> Vec<u8> {
        match self {
            Self::Utf8(bom) => {
                let mut bytes = if bom { vec![0xef, 0xbb, 0xbf] } else { vec![] };
                bytes.extend_from_slice(text.as_bytes());
                bytes
            }
            Self::Utf16(le) => {
                let mut bytes = if le {
                    vec![0xff, 0xfe]
                } else {
                    vec![0xfe, 0xff]
                };
                for word in text.encode_utf16() {
                    bytes.extend_from_slice(&if le {
                        word.to_le_bytes()
                    } else {
                        word.to_be_bytes()
                    });
                }
                bytes
            }
            Self::Utf32(le) => {
                let mut bytes = if le {
                    vec![0xff, 0xfe, 0, 0]
                } else {
                    vec![0, 0, 0xfe, 0xff]
                };
                for c in text.chars() {
                    bytes.extend_from_slice(&if le {
                        (c as u32).to_le_bytes()
                    } else {
                        (c as u32).to_be_bytes()
                    });
                }
                bytes
            }
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Eol {
    Lf,
    CrLf,
    Cr,
    Mixed,
}
impl Eol {
    pub fn label(self) -> &'static str {
        match self {
            Self::Lf => "LF",
            Self::CrLf => "CRLF",
            Self::Cr => "CR",
            Self::Mixed => "Mixed line endings",
        }
    }
}
#[derive(Clone, Debug)]
pub struct Document {
    pub path: PathBuf,
    raw: Arc<Vec<u8>>,
    pub text: Arc<String>,
    pub encoding: Option<Encoding>,
    pub eol: Eol,
    pub read_only: Option<String>,
    pub backup: Option<PathBuf>,
    original: Option<Arc<Vec<u8>>>,
}
fn open(path: &Path, share: u32) -> Result<File, String> {
    OpenOptions::new()
        .read(true)
        .share_mode(share)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_OPEN_NO_RECALL)
        .open(path)
        .map_err(|e| e.to_string())
}
fn bytes(file: &File) -> Result<Vec<u8>, String> {
    let mut raw = Vec::new();
    file.take((LIMIT + 1) as u64)
        .read_to_end(&mut raw)
        .map_err(|e| e.to_string())?;
    Ok(raw)
}
fn parse(path: &Path, raw: Vec<u8>, read_only: bool, truncated: bool) -> Result<Document, String> {
    let decoded = crate::text_decode::decode(&raw, truncated)?;
    let encoding = if raw.starts_with(&[0xff, 0xfe, 0, 0]) {
        Some(Encoding::Utf32(true))
    } else if raw.starts_with(&[0, 0, 0xfe, 0xff]) {
        Some(Encoding::Utf32(false))
    } else if raw.starts_with(&[0xff, 0xfe]) {
        Some(Encoding::Utf16(true))
    } else if raw.starts_with(&[0xfe, 0xff]) {
        Some(Encoding::Utf16(false))
    } else if raw.starts_with(&[0xef, 0xbb, 0xbf]) {
        Some(Encoding::Utf8(true))
    } else if std::str::from_utf8(&raw).is_ok_and(|text| text == decoded) {
        Some(Encoding::Utf8(false))
    } else {
        None
    };
    let crlf = decoded.matches("\r\n").count();
    let lf = decoded.matches('\n').count() - crlf;
    let cr = decoded.matches('\r').count() - crlf;
    let eol = if usize::from(crlf > 0) + usize::from(lf > 0) + usize::from(cr > 0) > 1 {
        Eol::Mixed
    } else if crlf > 0 {
        Eol::CrLf
    } else if cr > 0 {
        Eol::Cr
    } else {
        Eol::Lf
    };
    let normalized = decoded.replace("\r\n", "\n").replace('\r', "\n");
    let line_limit = normalized.lines().count() > 50_000;
    let reason = if line_limit {
        Some("Only the first 50,000 lines are shown. This file cannot be edited here.")
    } else if truncated {
        Some("Only the first 2 MiB is shown. Truncated files cannot be edited.")
    } else if read_only {
        Some("This file is read only or has multiple hard links.")
    } else if encoding.is_none() {
        Some("This encoding is available for reading only; saving could change its bytes.")
    } else if eol == Eol::Mixed {
        Some("Mixed line endings are preserved in read-only mode.")
    } else {
        None
    };
    Ok(Document {
        path: path.into(),
        raw: Arc::new(raw),
        text: Arc::new(if line_limit {
            normalized.split_inclusive('\n').take(50_000).collect()
        } else {
            normalized
        }),
        encoding,
        eol,
        read_only: reason.map(str::to_owned),
        backup: None,
        original: None,
    })
}
pub fn load(path: &Path) -> Result<Document, String> {
    let file = open(path, FILE_SHARE_READ)?;
    let meta = file.metadata().map_err(|e| e.to_string())?;
    if !meta.is_file()
        || meta.file_attributes()
            & (FILE_ATTRIBUTE_REPARSE_POINT | FILE_ATTRIBUTE_OFFLINE | 0x40000 | 0x400000)
            != 0
    {
        return Err("Linked or unavailable files cannot be read here.".into());
    }
    let mut standard = FILE_STANDARD_INFO::default();
    use std::os::windows::io::AsRawHandle;
    let standard_ok = unsafe {
        GetFileInformationByHandleEx(
            file.as_raw_handle(),
            FileStandardInfo,
            (&mut standard as *mut FILE_STANDARD_INFO).cast(),
            std::mem::size_of_val(&standard) as u32,
        )
    } != 0;
    let raw = bytes(&file)?;
    let truncated = raw.len() > LIMIT;
    let raw = if truncated {
        raw[..LIMIT].to_vec()
    } else {
        raw
    };
    parse(
        path,
        raw,
        meta.permissions().readonly() || !standard_ok || standard.NumberOfLinks > 1,
        truncated,
    )
}
#[cfg(test)]
pub fn save(document: &Document, text: &str) -> Result<Document, String> {
    save_with_options(document, text, true)
}
/// Write `text` to a new file beside the document, "name copy.ext" (then
/// "name copy 2.ext", ...), in the document's encoding and line endings.
/// Never replaces an existing file; the original is untouched.
pub fn save_copy(document: &Document, text: &str) -> Result<PathBuf, String> {
    let encoding = document.encoding.ok_or("Unknown encoding")?;
    let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
    let text = match document.eol {
        Eol::CrLf => normalized.replace('\n', "\r\n"),
        Eol::Cr => normalized.replace('\n', "\r"),
        Eol::Lf | Eol::Mixed => normalized,
    };
    let raw = encoding.encode(&text);
    let folder = document.path.parent().ok_or("The file has no folder")?;
    let name = document
        .path
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or("The file name cannot be represented")?;
    let (stem, extension) = match name.rfind('.') {
        Some(dot) if dot > 0 => (&name[..dot], &name[dot..]),
        _ => (name, ""),
    };
    for n in 1..1000 {
        let candidate = if n == 1 {
            format!("{stem} copy{extension}")
        } else {
            format!("{stem} copy {n}{extension}")
        };
        let path = folder.join(candidate);
        match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(mut file) => {
                use std::io::Write;
                if let Err(error) = file.write_all(&raw).and_then(|_| file.sync_all()) {
                    drop(file);
                    let _ = std::fs::remove_file(&path);
                    return Err(error.to_string());
                }
                return Ok(path);
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e.to_string()),
        }
    }
    Err("No free name for a copy".into())
}
pub fn save_with_options(
    document: &Document,
    text: &str,
    keep_backup: bool,
) -> Result<Document, String> {
    if let Some(reason) = &document.read_only {
        return Err(reason.clone());
    }
    let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
    let text = normalized.as_str();
    if text == document.text.as_str() {
        return Ok(document.clone());
    }
    let encoding = document.encoding.ok_or("Unknown encoding")?;
    let text = match document.eol {
        Eol::CrLf => text.replace('\n', "\r\n"),
        Eol::Cr => text.replace('\n', "\r"),
        Eol::Lf => text.to_owned(),
        Eol::Mixed => return Err("Mixed line endings cannot be edited".into()),
    };
    let raw = encoding.encode(&text);
    if raw.len() > LIMIT {
        return Err("The edited file exceeds the 2 MiB editing limit.".into());
    }
    let existing = open(&document.path, FILE_SHARE_READ | FILE_SHARE_DELETE)?;
    let meta = existing.metadata().map_err(|e| e.to_string())?;
    if !meta.is_file()
        || meta.file_attributes()
            & (FILE_ATTRIBUTE_REPARSE_POINT | FILE_ATTRIBUTE_OFFLINE | 0x40000 | 0x400000)
            != 0
        || bytes(&existing)? != *document.raw
    {
        return Err(
            "This file changed outside Seek. Reopen it before saving; your draft is still here."
                .into(),
        );
    }
    if meta.permissions().readonly() {
        return Err("This file is now read only. Your draft is still here.".into());
    }
    use std::os::windows::io::AsRawHandle;
    let mut standard = FILE_STANDARD_INFO::default();
    if unsafe {
        GetFileInformationByHandleEx(
            existing.as_raw_handle(),
            FileStandardInfo,
            (&mut standard as *mut FILE_STANDARD_INFO).cast(),
            std::mem::size_of_val(&standard) as u32,
        )
    } == 0
        || standard.NumberOfLinks != 1
    {
        return Err("The file now has multiple links or its identity cannot be verified. Your draft is still here.".into());
    }
    let folder = document.path.parent().ok_or("No parent folder")?;
    let id = unsafe { windows::Win32::System::Com::CoCreateGuid() }.map_err(|e| e.to_string())?;
    let stem = format!(".seek-save-{id:?}");
    let stage = folder.join(format!("{stem}.tmp"));
    let backup = folder.join(format!("{stem}.bak"));
    struct Stage(PathBuf);
    impl Drop for Stage {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }
    let mut staged = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&stage)
        .map_err(|e| e.to_string())?;
    let _cleanup = Stage(stage.clone());
    staged
        .write_all(&raw)
        .and_then(|_| staged.sync_all())
        .map_err(|e| e.to_string())?;
    drop(staged);
    let wide = |p: &Path| seek_engine::paths::extended(&seek_engine::paths::internal(p));
    let target_w = wide(&document.path);
    let stage_w = wide(&stage);
    let backup_w = wide(&backup);
    if unsafe {
        ReplaceFileW(
            target_w.as_ptr(),
            stage_w.as_ptr(),
            backup_w.as_ptr(),
            0,
            std::ptr::null(),
            std::ptr::null(),
        )
    } == 0
    {
        return Err(format!(
            "Save failed: {}. Your draft is retained. Any recovery copy is at {}",
            std::io::Error::last_os_error(),
            backup.display()
        ));
    }
    // The backup captures the replaced file atomically. Keep it even after success for recovery/undo.
    // A last-moment external replacement must never destroy the external version.
    let prior = open(&backup, FILE_SHARE_READ)
        .and_then(|file| bytes(&file))
        .map_err(|e| {
            format!(
                "Saved, but cannot verify the recovery copy at {}: {e}",
                backup.display()
            )
        })?;
    if prior != *document.raw {
        return Err(format!(
            "The file changed during save. Both versions were preserved; the previous file is at {}. Your draft remains open.",
            backup.display()
        ));
    }
    drop(existing);
    let mut saved = load(&document.path)?;
    if saved.raw.as_ref() != &raw {
        return Err(format!(
            "The file changed again during save. Your draft and the recovery copy at {} were retained.",
            backup.display()
        ));
    }
    saved.backup = if keep_backup || std::fs::remove_file(&backup).is_err() {
        Some(backup)
    } else {
        None
    };
    saved.original = Some(document.raw.clone());
    Ok(saved)
}
impl Document {
    pub fn can_undo(&self) -> bool {
        self.original.is_some()
    }
}
#[cfg(test)]
pub fn undo(current: &Document) -> Result<Document, String> {
    undo_with_options(current, true)
}
pub fn undo_with_options(current: &Document, keep_backup: bool) -> Result<Document, String> {
    let raw = current.original.clone().ok_or("No saved change to undo")?;
    if let Some(backup) = &current.backup
        && load(backup)?.raw.as_ref() != raw.as_ref()
    {
        return Err("The recovery copy changed. Undo was stopped; both files were kept.".into());
    }
    let original = parse(&current.path, raw.as_ref().clone(), false, false)?;
    // Use the same expected-current snapshot and replacement/backup path as a normal save.
    // Preserve original encoding and EOL by constructing its normalized draft.
    let mut expected = current.clone();
    expected.encoding = original.encoding;
    expected.eol = original.eol;
    expected.read_only = original.read_only;
    let mut restored = save_with_options(&expected, &original.text, keep_backup)?;
    if restored.raw.as_ref() != raw.as_ref() {
        return Err("Undo encoding verification failed; recovery copies were retained.".into());
    }
    restored.backup = None;
    restored.original = None;
    Ok(restored)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn encoding_bom_and_line_endings_round_trip_and_undo() {
        for encoding in [
            Encoding::Utf8(false),
            Encoding::Utf8(true),
            Encoding::Utf16(true),
            Encoding::Utf16(false),
            Encoding::Utf32(true),
            Encoding::Utf32(false),
        ] {
            for eol in ["\n", "\r\n", "\r"] {
                let temp = tempfile::tempdir().unwrap();
                let path = temp.path().join("owned.txt");
                let original = encoding.encode(&format!(
                    "R\u{e9}sum\u{e9}{eol}\u{4e2d}\u{6587} \u{1f980}{eol}"
                ));
                std::fs::write(&path, &original).unwrap();
                let doc = load(&path).unwrap();
                assert!(doc.read_only.is_none());
                let saved = save(&doc, "Updated\n\u{4e2d}\u{6587} \u{1f980}\n").unwrap();
                assert_eq!(
                    std::fs::read(&path).unwrap(),
                    encoding.encode(&format!("Updated{eol}\u{4e2d}\u{6587} \u{1f980}{eol}"))
                );
                undo(&saved).unwrap();
                assert_eq!(std::fs::read(&path).unwrap(), original);
            }
        }
    }
    #[test]
    fn conflicts_and_readonly_inputs_preserve_both_disk_and_draft() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("owned.txt");
        std::fs::write(&path, b"original").unwrap();
        let doc = load(&path).unwrap();
        std::fs::write(&path, b"external").unwrap();
        assert!(save(&doc, "draft").is_err());
        assert_eq!(std::fs::read(&path).unwrap(), b"external");
        std::fs::write(&path, b"one\r\ntwo\n").unwrap();
        let doc = load(&path).unwrap();
        assert!(doc.read_only.is_some());
        assert!(save(&doc, "draft").is_err());
        std::fs::write(&path, vec![b'a'; LIMIT + 1]).unwrap();
        let doc = load(&path).unwrap();
        assert!(doc.read_only.is_some());
        assert!(save(&doc, "draft").is_err());
        assert_eq!(std::fs::metadata(&path).unwrap().len(), (LIMIT + 1) as u64);
        std::fs::write(&path, [0, 1, 2, 3, 0xff]).unwrap();
        assert!(load(&path).is_err());
    }
    #[test]
    fn line_cap_unknown_encoding_and_hard_links_never_expose_save() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("owned.txt");
        std::fs::write(&path, "line\n".repeat(50_001)).unwrap();
        let doc = load(&path).unwrap();
        assert!(doc.read_only.is_some());
        assert_eq!(doc.text.lines().count(), 50_000);
        assert!(save(&doc, "draft").is_err());
        std::fs::write(&path, [0, b'a', 0, b'b', 0, b'c', 0, b'd']).unwrap();
        let doc = load(&path).unwrap();
        assert!(doc.read_only.is_some());
        assert!(save(&doc, "draft").is_err());
        std::fs::write(&path, b"original").unwrap();
        let doc = load(&path).unwrap();
        let linked = temp.path().join("linked.txt");
        std::fs::hard_link(&path, &linked).unwrap();
        assert!(load(&path).unwrap().read_only.is_some());
        assert!(save(&doc, "draft").is_err());
        assert_eq!(std::fs::read(linked).unwrap(), b"original");
    }
    #[test]
    fn first_markdown_checkbox_save_keeps_exact_backup_when_enabled() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("Tasks.md");
        let original = b"# Tasks\r\n\r\n- [ ] First check\r\n";
        std::fs::write(&path, original).unwrap();
        let document = load(&path).unwrap();
        let task = crate::markdown_tasks::tasks(&document.text).remove(0);
        let draft =
            crate::markdown_tasks::toggle(&document.text, task.offset, task.checked).unwrap();
        let saved = save_with_options(&document, &draft, true).unwrap();
        assert_eq!(
            std::fs::read(saved.backup.as_ref().unwrap()).unwrap(),
            original
        );
        assert!(std::fs::read_to_string(&path).unwrap().contains("[x]"));
    }
    #[test]
    fn save_copy_keeps_encoding_and_never_replaces_files() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("notes.txt");
        std::fs::write(&path, b"one\r\ntwo\r\n").unwrap();
        std::fs::write(temp.path().join("notes copy.txt"), b"taken").unwrap();
        let document = load(&path).unwrap();
        let copy = save_copy(&document, "one\nchanged\n").unwrap();
        assert_eq!(copy.file_name().unwrap(), "notes copy 2.txt");
        assert_eq!(std::fs::read(&copy).unwrap(), b"one\r\nchanged\r\n");
        assert_eq!(std::fs::read(&path).unwrap(), b"one\r\ntwo\r\n");
        assert_eq!(
            std::fs::read(temp.path().join("notes copy.txt")).unwrap(),
            b"taken"
        );
    }
    #[test]
    fn optional_backups_do_not_leave_files_but_session_undo_still_restores_exact_bytes() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("owned.txt");
        std::fs::write(&path, b"original\r\n").unwrap();
        let saved = save_with_options(&load(&path).unwrap(), "edited\n", false).unwrap();
        assert!(saved.backup.is_none());
        assert!(saved.can_undo());
        assert_eq!(std::fs::read_dir(temp.path()).unwrap().count(), 1);
        let restored = undo_with_options(&saved, false).unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"original\r\n");
        assert!(!restored.can_undo());
        assert_eq!(std::fs::read_dir(temp.path()).unwrap().count(), 1);
    }
    #[test]
    fn undo_refuses_external_edits_and_tampered_recovery_copies() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("owned.txt");
        std::fs::write(&path, b"original").unwrap();
        let saved = save(&load(&path).unwrap(), "saved").unwrap();
        std::fs::write(&path, b"external").unwrap();
        assert!(undo(&saved).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), b"external");
        std::fs::write(&path, b"saved").unwrap();
        std::fs::write(saved.backup.as_ref().unwrap(), b"changed backup").unwrap();
        assert!(undo(&saved).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), b"saved");
    }
    #[test]
    fn failed_replacement_retains_original_and_cleans_owned_staging_file() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("owned.txt");
        std::fs::write(&path, b"original").unwrap();
        let doc = load(&path).unwrap();
        let lock = open(&path, FILE_SHARE_READ).unwrap();
        assert!(save(&doc, "draft").is_err());
        assert_eq!(std::fs::read(&path).unwrap(), b"original");
        assert_eq!(std::fs::read_dir(temp.path()).unwrap().count(), 1);
        drop(lock);
    }
}
