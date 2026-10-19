use std::{os::windows::ffi::OsStringExt, path::PathBuf};
/// Resolve redirected Known Folders as the ordinary user, including OneDrive.
pub fn favorites() -> Vec<(String, PathBuf)> {
    use windows_sys::Win32::{System::Com::CoTaskMemFree, UI::Shell::*};
    [
        ("Downloads", FOLDERID_Downloads),
        ("Desktop", FOLDERID_Desktop),
        ("Documents", FOLDERID_Documents),
        ("Pictures", FOLDERID_Pictures),
        ("Videos", FOLDERID_Videos),
        ("Music", FOLDERID_Music),
    ]
    .into_iter()
    .filter_map(|(name, id)| {
        let mut wide = std::ptr::null_mut();
        let path = unsafe {
            if SHGetKnownFolderPath(&id, 0, std::ptr::null_mut(), &mut wide) >= 0 && !wide.is_null()
            {
                let mut len = 0;
                while *wide.add(len) != 0 {
                    len += 1;
                }
                let path = PathBuf::from(std::ffi::OsString::from_wide(
                    std::slice::from_raw_parts(wide, len),
                ));
                CoTaskMemFree(wide.cast());
                Some(path)
            } else {
                None
            }
        };
        path.filter(|p| p.is_dir()).map(|p| (name.into(), p))
    })
    .collect()
}
