//! Read-only Windows allocation metadata. Call on the existing metadata worker.
use std::{
    os::windows::{ffi::OsStrExt, fs::MetadataExt, io::FromRawHandle},
    path::Path,
};
use windows_sys::Win32::{Foundation::INVALID_HANDLE_VALUE, Storage::FileSystem::*};

/// Unknown, changed, directory or reparse items return None; never guess allocation.
pub fn allocation_size(path: &Path, expected_length: u64) -> Option<u64> {
    let wide: Vec<_> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    let handle = unsafe {
        CreateFileW(
            wide.as_ptr(),
            FILE_READ_ATTRIBUTES,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            std::ptr::null(),
            OPEN_EXISTING,
            FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_NO_RECALL,
            std::ptr::null_mut(),
        )
    };
    if handle == INVALID_HANDLE_VALUE || handle.is_null() {
        return None;
    }
    // The File takes ownership and closes the handle on every early return.
    let file = unsafe { std::fs::File::from_raw_handle(handle) };
    let metadata = file.metadata().ok()?;
    if !metadata.is_file()
        || metadata.len() != expected_length
        || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
    {
        return None;
    }
    let mut info = FILE_STANDARD_INFO::default();
    let success = unsafe {
        GetFileInformationByHandleEx(
            handle,
            FileStandardInfo,
            (&mut info as *mut FILE_STANDARD_INFO).cast(),
            std::mem::size_of_val(&info) as u32,
        )
    } != 0;
    if !(success
        && !info.Directory
        && !info.DeletePending
        && info.EndOfFile >= 0
        && info.EndOfFile as u64 == expected_length
        && info.AllocationSize >= 0)
    {
        return None;
    }
    if metadata.file_attributes() & (FILE_ATTRIBUTE_COMPRESSED | FILE_ATTRIBUTE_SPARSE_FILE) != 0 {
        let mut compression = FILE_COMPRESSION_INFO::default();
        let success = unsafe {
            GetFileInformationByHandleEx(
                handle,
                FileCompressionInfo,
                (&mut compression as *mut FILE_COMPRESSION_INFO).cast(),
                std::mem::size_of_val(&compression) as u32,
            )
        } != 0;
        return (success && compression.CompressedFileSize >= 0)
            .then_some(compression.CompressedFileSize as u64);
    }
    Some(info.AllocationSize as u64)
}

/// Volume and owner are independent: denied security access must not hide volume data.
pub fn storage(path: &Path) -> (Option<String>, Option<String>) {
    use std::os::windows::io::AsRawHandle;
    let open = |access| {
        let wide: Vec<_> = path.as_os_str().encode_wide().chain(Some(0)).collect();
        let handle = unsafe {
            CreateFileW(
                wide.as_ptr(),
                access,
                FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                std::ptr::null(),
                OPEN_EXISTING,
                FILE_FLAG_OPEN_REPARSE_POINT
                    | FILE_FLAG_BACKUP_SEMANTICS
                    | FILE_FLAG_OPEN_NO_RECALL,
                std::ptr::null_mut(),
            )
        };
        if handle == INVALID_HANDLE_VALUE || handle.is_null() {
            None
        } else {
            Some(unsafe { std::fs::File::from_raw_handle(handle) })
        }
    };
    let volume = open(FILE_READ_ATTRIBUTES).and_then(|file| {
        let mut label = vec![0u16; 256];
        let mut fs = vec![0u16; 256];
        let ok = unsafe {
            GetVolumeInformationByHandleW(
                file.as_raw_handle(),
                label.as_mut_ptr(),
                label.len() as u32,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                fs.as_mut_ptr(),
                fs.len() as u32,
            )
        } != 0;
        if !ok {
            return None;
        }
        let text = |v: &[u16]| {
            String::from_utf16_lossy(&v[..v.iter().position(|c| *c == 0).unwrap_or(v.len())])
        };
        let label = text(&label);
        let fs = text(&fs);
        Some(if label.is_empty() {
            fs
        } else {
            format!("{label} ({fs})")
        })
    });
    let owner = open(READ_CONTROL).and_then(|file| {
        use windows_sys::Win32::Security::{Authorization::*, *};
        let mut sid = std::ptr::null_mut();
        let mut descriptor = std::ptr::null_mut();
        let code = unsafe {
            GetSecurityInfo(
                file.as_raw_handle(),
                SE_FILE_OBJECT,
                OWNER_SECURITY_INFORMATION,
                &mut sid,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                &mut descriptor,
            )
        };
        if code != 0 {
            return None;
        }
        // GetSecurityInfo allocates this descriptor with LocalAlloc. Keep it alive through SID lookup.
        struct Descriptor(*mut std::ffi::c_void);
        impl Drop for Descriptor {
            fn drop(&mut self) {
                unsafe {
                    windows_sys::Win32::Foundation::LocalFree(self.0);
                }
            }
        }
        let _descriptor = Descriptor(descriptor);
        if sid.is_null() {
            return None;
        }
        let (mut name_len, mut domain_len, mut kind) = (0, 0, 0);
        unsafe {
            LookupAccountSidW(
                std::ptr::null(),
                sid,
                std::ptr::null_mut(),
                &mut name_len,
                std::ptr::null_mut(),
                &mut domain_len,
                &mut kind,
            );
        }
        if name_len == 0 || name_len > 32768 || domain_len > 32768 {
            return None;
        }
        let mut name = vec![0u16; name_len as usize];
        let mut domain = vec![0u16; domain_len.max(1) as usize];
        if unsafe {
            LookupAccountSidW(
                std::ptr::null(),
                sid,
                name.as_mut_ptr(),
                &mut name_len,
                domain.as_mut_ptr(),
                &mut domain_len,
                &mut kind,
            )
        } == 0
        {
            return None;
        }
        let name = String::from_utf16_lossy(&name[..name_len as usize]);
        let domain = String::from_utf16_lossy(&domain[..domain_len as usize]);
        Some(if domain.is_empty() {
            name
        } else {
            format!("{domain}\\{name}")
        })
    });
    (volume, owner)
}

pub fn attributes(flags: u32) -> String {
    let labels = [
        (FILE_ATTRIBUTE_DIRECTORY, "Directory"),
        (FILE_ATTRIBUTE_READONLY, "Read only"),
        (FILE_ATTRIBUTE_HIDDEN, "Hidden"),
        (FILE_ATTRIBUTE_SYSTEM, "System"),
        (FILE_ATTRIBUTE_ARCHIVE, "Archive"),
        (FILE_ATTRIBUTE_COMPRESSED, "Compressed"),
        (FILE_ATTRIBUTE_ENCRYPTED, "Encrypted"),
        (FILE_ATTRIBUTE_SPARSE_FILE, "Sparse"),
        (FILE_ATTRIBUTE_OFFLINE, "Offline"),
        (FILE_ATTRIBUTE_REPARSE_POINT, "Reparse point"),
    ];
    let values: Vec<_> = labels
        .into_iter()
        .filter_map(|(flag, label)| (flags & flag != 0).then_some(label))
        .collect();
    if values.is_empty() {
        "Normal".into()
    } else {
        values.join(", ")
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn native_storage_metadata_is_read_only_and_missing_values_remain_unknown() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("owned.txt");
        std::fs::write(&path, b"owned").unwrap();
        let (volume, owner) = super::storage(&path);
        assert!(volume.is_some());
        assert!(owner.is_some());
        assert_eq!(std::fs::read(&path).unwrap(), b"owned");
        assert_eq!(super::storage(&root.path().join("missing")), (None, None));
    }
    #[test]
    fn allocation_is_read_only_and_unknown_items_are_not_reported_as_zero() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("owned.bin");
        std::fs::write(&path, b"owned metadata").unwrap();
        assert!(super::allocation_size(&path, 14).is_some());
        assert!(super::allocation_size(&path, 999).is_none());
        assert_eq!(std::fs::read(&path).unwrap(), b"owned metadata");
        assert!(super::allocation_size(root.path(), 0).is_none());
        std::fs::remove_file(&path).unwrap();
        assert!(super::allocation_size(&path, 14).is_none());
    }
    #[test]
    fn metadata_does_not_follow_a_file_symlink_when_creation_is_permitted() {
        let root = tempfile::tempdir().unwrap();
        let target = root.path().join("target.bin");
        std::fs::write(&target, b"private target").unwrap();
        let link = root.path().join("link.bin");
        match std::os::windows::fs::symlink_file(&target, &link) {
            Ok(()) => assert!(super::allocation_size(&link, 14).is_none()),
            Err(error) => eprintln!("Symlink acceptance unverified: creation unavailable: {error}"),
        }
    }
}
