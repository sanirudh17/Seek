//! Explorer-style addresses are expanded only on Enter, never passed to a shell.
use std::path::PathBuf;
#[link(name = "kernel32")]
unsafe extern "system" {
    fn ExpandEnvironmentStringsW(source: *const u16, target: *mut u16, size: u32) -> u32;
}
pub fn address(text: &str) -> Option<Result<PathBuf, String>> {
    // Older Name suggestions may already have decorated an address as a name
    // token. Recover that single literal; never reinterpret a compound query.
    let raw = text.trim();
    let text = raw
        .strip_prefix("name:")
        .filter(|v| v.starts_with('"') && v.ends_with('"'))
        .unwrap_or(raw)
        .trim_matches('"');
    let explicit = text.starts_with('%')
        || text.starts_with("\\\\")
        || text.as_bytes().get(1) == Some(&b':')
        || text == "~"
        || text.starts_with("~\\")
        || text.starts_with("~/");
    if !explicit {
        return None;
    }
    Some(expand(text))
}
fn expand(text: &str) -> Result<PathBuf, String> {
    use std::os::windows::ffi::OsStringExt;
    if text.contains('\0') {
        return Err("Invalid folder address".into());
    }
    let text = if text == "~" || text.starts_with("~\\") || text.starts_with("~/") {
        format!("%USERPROFILE%{}", &text[1..])
    } else {
        text.to_owned()
    };
    let source: Vec<u16> = text.encode_utf16().chain(Some(0)).collect();
    let size = unsafe { ExpandEnvironmentStringsW(source.as_ptr(), std::ptr::null_mut(), 0) };
    if size == 0 || size > 32768 {
        return Err("Could not expand folder address".into());
    }
    let mut target = vec![0u16; size as usize];
    let used = unsafe { ExpandEnvironmentStringsW(source.as_ptr(), target.as_mut_ptr(), size) };
    if used == 0 || used > size {
        return Err("Folder address changed; try again".into());
    }
    Ok(PathBuf::from(std::ffi::OsString::from_wide(
        &target[..used as usize - 1],
    )))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn expands_current_user_variables_and_preserves_unicode() {
        for name in [
            "TEMP",
            "temp",
            "USERPROFILE",
            "LOCALAPPDATA",
            "APPDATA",
            "SystemRoot",
        ] {
            let path = address(&format!("%{name}%")).unwrap().unwrap();
            assert_eq!(path, PathBuf::from(std::env::var_os(name).unwrap()));
        }
        assert_eq!(
            address("%TEMP%\\café").unwrap().unwrap(),
            std::env::temp_dir().join("café")
        );
        assert!(address("notes about %TEMP%").is_none());
        assert!(address("ext:pdf").is_none());
        assert_eq!(
            address("name:\"%TEMP%\"").unwrap().unwrap(),
            std::env::temp_dir()
        );
        assert_eq!(
            address("%SEEK_MISSING_ADDRESS_VARIABLE_7391%")
                .unwrap()
                .unwrap(),
            PathBuf::from("%SEEK_MISSING_ADDRESS_VARIABLE_7391%")
        );
    }
}
