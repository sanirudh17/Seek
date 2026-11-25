//! Streaming undo evidence for local files, directory topology and named streams.
use crate::operation_journal::Fingerprint;
use sha2::{Digest, Sha256};
use std::{
    fs::{File, OpenOptions},
    io::Read,
    os::windows::{
        ffi::{OsStrExt, OsStringExt},
        fs::{MetadataExt, OpenOptionsExt},
        io::AsRawHandle,
    },
    path::{Path, PathBuf},
};
use windows_sys::Win32::{
    Foundation::{ERROR_HANDLE_EOF, ERROR_INVALID_PARAMETER, GetLastError, INVALID_HANDLE_VALUE},
    Storage::FileSystem::*,
};

fn opened(path: &Path) -> Result<File, String> {
    OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_DELETE)
        .custom_flags(
            FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_OPEN_NO_RECALL,
        )
        .open(path)
        .map_err(|e| e.to_string())
}
pub(crate) fn final_path(file: &File) -> Result<PathBuf, String> {
    let mut buffer = vec![0u16; 512];
    loop {
        let n = unsafe {
            GetFinalPathNameByHandleW(
                file.as_raw_handle(),
                buffer.as_mut_ptr(),
                buffer.len() as u32,
                0,
            )
        } as usize;
        if n == 0 {
            return Err(std::io::Error::last_os_error().to_string());
        }
        if n < buffer.len() {
            buffer.truncate(n);
            return Ok(std::ffi::OsString::from_wide(&buffer).into());
        }
        if n > 32768 {
            return Err("Undo path exceeds the Windows path limit".into());
        }
        buffer.resize(n + 1, 0);
    }
}
fn field(hash: &mut Sha256, bytes: &[u8]) {
    hash.update((bytes.len() as u64).to_le_bytes());
    hash.update(bytes);
}
fn wide_field(hash: &mut Sha256, wide: &[u16]) {
    hash.update((wide.len() as u64).to_le_bytes());
    for c in wide {
        hash.update(c.to_le_bytes());
    }
}
struct Search(windows_sys::Win32::Foundation::HANDLE);
impl Drop for Search {
    fn drop(&mut self) {
        unsafe {
            FindClose(self.0);
        }
    }
}
fn streams(path: &Path, hash: &mut Sha256) -> Result<(), String> {
    let path_wide = path.as_os_str().encode_wide().collect::<Vec<_>>();
    let mut terminated = path_wide.clone();
    terminated.push(0);
    let mut data = WIN32_FIND_STREAM_DATA::default();
    let handle = unsafe {
        FindFirstStreamW(
            terminated.as_ptr(),
            FindStreamInfoStandard,
            (&mut data as *mut WIN32_FIND_STREAM_DATA).cast(),
            0,
        )
    };
    if handle == INVALID_HANDLE_VALUE {
        return match unsafe { GetLastError() } {
            ERROR_HANDLE_EOF | ERROR_INVALID_PARAMETER => Ok(()),
            _ => Err(std::io::Error::last_os_error().to_string()),
        };
    }
    let search = Search(handle);
    let mut names = vec![];
    loop {
        crate::operation_control::check()?;
        let length = data
            .cStreamName
            .iter()
            .position(|c| *c == 0)
            .ok_or("Invalid Windows stream name")?;
        let name = data.cStreamName[..length].to_vec();
        if name != "::$DATA".encode_utf16().collect::<Vec<_>>() {
            names.push((name, data.StreamSize));
        }
        if unsafe { FindNextStreamW(search.0, (&mut data as *mut WIN32_FIND_STREAM_DATA).cast()) }
            == 0
        {
            if unsafe { GetLastError() } != ERROR_HANDLE_EOF {
                return Err(std::io::Error::last_os_error().to_string());
            }
            break;
        }
    }
    names.sort_by(|a, b| a.0.cmp(&b.0));
    for (name, expected_size) in names {
        if expected_size < 0 {
            return Err("Invalid stream size".into());
        }
        wide_field(hash, &name);
        let stream_path = PathBuf::from(std::ffi::OsString::from_wide(
            &[path_wide.as_slice(), name.as_slice()].concat(),
        ));
        let mut stream = opened(&stream_path)?;
        let mut digest = Sha256::new();
        let mut bytes = 0u64;
        let mut buffer = [0u8; 64 * 1024];
        loop {
            crate::operation_control::check()?;
            let n = stream.read(&mut buffer).map_err(|e| e.to_string())?;
            if n == 0 {
                break;
            }
            bytes = bytes
                .checked_add(n as u64)
                .ok_or("Stream length overflow")?;
            digest.update(&buffer[..n]);
        }
        if bytes != expected_size as u64 {
            return Err("Named stream changed while recording undo evidence".into());
        }
        hash.update(bytes.to_le_bytes());
        field(hash, &digest.finalize());
    }
    Ok(())
}
pub fn read(file: &File) -> Result<Fingerprint, String> {
    read_inner(file, 0)
}
fn read_inner(file: &File, depth: usize) -> Result<Fingerprint, String> {
    crate::operation_control::check()?;
    // Guard the call stack; Windows paths normally hit their own limit first.
    if depth > 512 {
        return Err("Directory nesting is too deep for undo evidence".into());
    }
    let meta = file.metadata().map_err(|e| e.to_string())?;
    if meta.file_attributes()
        & (FILE_ATTRIBUTE_REPARSE_POINT | FILE_ATTRIBUTE_OFFLINE | 0x40000 | 0x400000)
        != 0
    {
        return Err("Undo does not follow links or hydrate offline data".into());
    }
    let (volume, id) = Fingerprint::key(file)?;
    let path = final_path(file)?;
    let mut content = Sha256::new();
    content.update(b"seek-content-v2");
    let mut result = if meta.is_file() {
        let result = Fingerprint::read_legacy(file)?;
        content.update(b"file");
        content.update(result.bytes.to_le_bytes());
        field(&mut content, &result.hash);
        result
    } else if meta.is_dir() {
        content.update(b"directory");
        let mut identity = Sha256::new();
        identity.update(b"seek-tree-v2");
        let names = directory_names(&path)?;
        let mut total = 0u64;
        for name in &names {
            crate::operation_control::check()?;
            let child = opened(&path.join(name))?;
            let snapshot = read_inner(&child, depth + 1)?;
            let name = name.encode_wide().collect::<Vec<_>>();
            wide_field(&mut identity, &name);
            identity.update(snapshot.volume.to_le_bytes());
            field(&mut identity, &snapshot.id);
            field(&mut identity, &snapshot.hash);
            wide_field(&mut content, &name);
            field(
                &mut content,
                &snapshot.content_hash.ok_or("Incomplete child snapshot")?,
            );
            total = total
                .checked_add(snapshot.bytes)
                .ok_or("Directory size overflow")?;
        }
        let after_names = directory_names(&path)?;
        if names != after_names {
            return Err("Directory contents changed while recording undo evidence".into());
        }
        Fingerprint {
            volume,
            id,
            bytes: total,
            hash: identity.finalize().into(),
            content_hash: None,
            directory: true,
        }
    } else {
        return Err("Unsupported filesystem object".into());
    };
    streams(&path, &mut content)?;
    result.content_hash = Some(content.finalize().into());
    // Include streams in identity guards as well as cross-volume content checks.
    if result.directory {
        let mut all = Sha256::new();
        all.update(result.hash);
        all.update(result.content_hash.unwrap());
        result.hash = all.finalize().into();
    }
    if Fingerprint::key(&opened(&path)?)? != (volume, id) {
        return Err("Snapshot path was replaced".into());
    }
    Ok(result)
}
fn directory_names(path: &Path) -> Result<Vec<std::ffi::OsString>, String> {
    let mut names = vec![];
    for entry in std::fs::read_dir(path).map_err(|e| e.to_string())? {
        crate::operation_control::check()?;
        names
            .try_reserve(1)
            .map_err(|_| "Insufficient memory for the directory snapshot")?;
        names.push(entry.map_err(|e| e.to_string())?.file_name());
    }
    names.sort_by(|a, b| a.encode_wide().cmp(b.encode_wide()));
    Ok(names)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn large_files_and_named_streams_have_complete_evidence() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("large.bin");
        File::create(&path)
            .unwrap()
            .set_len(17 * 1024 * 1024)
            .unwrap();
        let original = Fingerprint::at(&path).unwrap();
        assert_eq!(original.bytes, 17 * 1024 * 1024);
        let stream = PathBuf::from(format!("{}:notes", path.display()));
        std::fs::write(&stream, b"first stream").unwrap();
        let named = Fingerprint::at(&path).unwrap();
        assert_eq!(named.hash, original.hash);
        assert_ne!(named.content_hash, original.content_hash);
        assert!(!named.compatible(&original));
        std::fs::write(&stream, b"changed stream").unwrap();
        assert!(!Fingerprint::at(&path).unwrap().compatible(&named));
        let mut legacy = original.clone();
        legacy.content_hash = None;
        assert!(named.compatible(&legacy));
    }
    #[test]
    fn directory_content_is_portable_but_identity_detects_replacements() {
        let temp = tempfile::tempdir().unwrap();
        let a = temp.path().join("a");
        let b = temp.path().join("b");
        std::fs::create_dir(&a).unwrap();
        std::fs::create_dir(&b).unwrap();
        for folder in [&a, &b] {
            std::fs::create_dir(folder.join("nested")).unwrap();
            std::fs::write(folder.join("nested/data.txt"), b"same bytes").unwrap();
        }
        let first = Fingerprint::at(&a).unwrap();
        let copy = Fingerprint::at(&b).unwrap();
        assert!(first.directory);
        assert_eq!(first.content_hash, copy.content_hash);
        assert_ne!(first.hash, copy.hash);
        let renamed = temp.path().join("renamed");
        std::fs::rename(&a, &renamed).unwrap();
        assert_eq!(Fingerprint::at(&renamed).unwrap(), first);
        std::fs::write(renamed.join("nested/data.txt"), b"other data").unwrap();
        assert!(!Fingerprint::at(&renamed).unwrap().compatible(&first));
    }
    #[test]
    fn cancelled_snapshot_stops_without_changing_data() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("data.txt");
        std::fs::write(&path, b"untouched").unwrap();
        let file = opened(&path).unwrap();
        let token = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true));
        {
            let _scope = crate::operation_control::enter(token);
            assert!(read(&file).unwrap_err().contains("cancelled"));
        }
        assert!(read(&file).is_ok());
        assert_eq!(std::fs::read(path).unwrap(), b"untouched");
    }
}
