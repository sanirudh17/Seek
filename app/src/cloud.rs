//! OneDrive and other cloud placeholders (Phase 8.6). Seek never hydrates a
//! placeholder as a side effect of scanning or previewing; downloading
//! happens only when the person chooses Download.

/// FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS, RECALL_ON_OPEN and OFFLINE.
const CLOUD_ATTRIBUTES: u32 = 0x400000 | 0x40000 | 0x1000;

/// A file whose contents live in the cloud, not on this PC yet.
pub fn is_placeholder(path: &std::path::Path) -> bool {
    use std::os::windows::fs::MetadataExt;
    std::fs::symlink_metadata(path)
        .is_ok_and(|m| m.is_file() && m.file_attributes() & CLOUD_ATTRIBUTES != 0)
}

/// Downloads one placeholder's contents through the Cloud Files API.
pub fn download(path: &std::path::Path) -> Result<(), String> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::{
        Foundation::{CloseHandle, GENERIC_READ, INVALID_HANDLE_VALUE},
        Storage::{CloudFilters::CfHydratePlaceholder, FileSystem::*},
    };
    if !is_placeholder(path) {
        return Ok(());
    }
    let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    let handle = unsafe {
        CreateFileW(
            wide.as_ptr(),
            GENERIC_READ,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            std::ptr::null(),
            OPEN_EXISTING,
            FILE_ATTRIBUTE_NORMAL,
            std::ptr::null_mut(),
        )
    };
    if handle == INVALID_HANDLE_VALUE {
        return Err(std::io::Error::last_os_error().to_string());
    }
    let result = unsafe { CfHydratePlaceholder(handle, 0, -1, 0, std::ptr::null_mut()) };
    unsafe {
        CloseHandle(handle);
    }
    if result < 0 {
        return Err(format!(
            "The cloud provider could not download this file (0x{:08X})",
            result as u32
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn ordinary_files_are_not_placeholders_and_download_is_a_no_op() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("local.txt");
        std::fs::write(&file, "here").unwrap();
        assert!(!super::is_placeholder(&file));
        assert!(!super::is_placeholder(dir.path()));
        assert!(super::download(&file).is_ok());
    }
}
