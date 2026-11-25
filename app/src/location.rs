//! Explorer-style addresses are expanded, never passed to a shell.
use std::path::PathBuf;
#[link(name = "kernel32")]
unsafe extern "system" {
    fn ExpandEnvironmentStringsW(source: *const u16, target: *mut u16, size: u32) -> u32;
}

/// The folder or file an address in the search box names, if the text is an
/// address at all: `C:\…`, `\\server\share`, `%APPDATA%\…`, `~\…`, also as
/// terminals, PowerShell, Explorer and browsers copy them (see `normalize`).
pub fn address(text: &str) -> Option<Result<PathBuf, String>> {
    let text = normalize(text)?;
    let bytes = text.as_bytes();
    let explicit = text.starts_with('%')
        || text.starts_with(r"\\")
        || (bytes.get(1) == Some(&b':') && bytes[0].is_ascii_alphabetic())
        || text == "~"
        || text.starts_with(r"~\")
        || text.starts_with("~/");
    if !explicit {
        return None;
    }
    Some(expand(&text))
}

/// The address in what people paste: strips a `cd` / `Set-Location` in
/// front, quotes, a `file:///` scheme and trailing line breaks; turns
/// PowerShell's `$env:APPDATA` into `%APPDATA%` and forward slashes into
/// backslashes. None for text spread over several lines.
fn normalize(text: &str) -> Option<String> {
    // Older Name suggestions may already have decorated an address as a name
    // token. Recover that single literal; never reinterpret a compound query.
    let mut text = text.trim();
    if let Some(v) = text
        .strip_prefix("name:")
        .filter(|v| v.len() >= 2 && v.starts_with('"') && v.ends_with('"'))
    {
        text = v;
    }
    if text.contains(['\n', '\r']) {
        return None;
    }
    let lower = text.to_ascii_lowercase();
    const COMMANDS: [&str; 11] = [
        "cd /d ",
        "chdir /d ",
        "cd ",
        "chdir ",
        "pushd ",
        "set-location -literalpath ",
        "set-location -path ",
        "set-location ",
        "sl ",
        "explorer.exe ",
        "explorer ",
    ];
    if let Some(command) = COMMANDS.iter().find(|c| lower.starts_with(**c)) {
        text = text[command.len()..].trim_start();
    }
    let text = text.trim().trim_end_matches(';').trim();
    let text = text
        .strip_prefix('"')
        .and_then(|t| t.strip_suffix('"'))
        .or_else(|| text.strip_prefix('\'').and_then(|t| t.strip_suffix('\'')))
        .unwrap_or(text)
        .trim();
    let mut text = text.to_string();
    if starts_with_ignore_case(&text, "file:///") {
        text = percent_decode(&text[8..]);
    } else if starts_with_ignore_case(&text, "file://") {
        // file://server/share
        text = format!("//{}", percent_decode(&text[7..]));
    }
    // PowerShell: $env:APPDATA\Code or ${env:APPDATA}\Code.
    for prefix in ["${env:", "$env:"] {
        if text.len() > prefix.len() && starts_with_ignore_case(&text, prefix) {
            let rest = &text[prefix.len()..];
            let end = rest
                .find(|c: char| !(c.is_alphanumeric() || matches!(c, '_' | '(' | ')')))
                .unwrap_or(rest.len());
            let after = rest[end..].strip_prefix('}').unwrap_or(&rest[end..]);
            text = format!("%{}%{}", &rest[..end], after);
            break;
        }
    }
    // C:/Users/x and //server/share, as browsers and Git Bash copy them.
    if text.as_bytes().get(1) == Some(&b':') || text.starts_with("//") {
        text = text.replace('/', r"\");
    }
    // "C:users\x" is relative to C:'s current folder in a shell; here it
    // means C:\users\x.
    let bytes = text.as_bytes();
    if bytes.len() > 2 && bytes[1] == b':' && bytes[0].is_ascii_alphabetic() && bytes[2] != b'\\' {
        text.insert(2, '\\');
    }
    Some(text)
}

fn starts_with_ignore_case(text: &str, prefix: &str) -> bool {
    text.len() >= prefix.len()
        && text.is_char_boundary(prefix.len())
        && text[..prefix.len()].eq_ignore_ascii_case(prefix)
}

fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && let Some(hex) = text.get(i + 1..i + 3)
            && let Ok(value) = u8::from_str_radix(hex, 16)
        {
            out.push(value);
            i += 3;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn expand(text: &str) -> Result<PathBuf, String> {
    use std::os::windows::ffi::OsStringExt;
    if text.contains('\0') {
        return Err("Invalid folder address".into());
    }
    let text = if text == "~" || text.starts_with(r"~\") || text.starts_with("~/") {
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

    #[test]
    fn pasted_addresses_are_recognised() {
        let appdata = PathBuf::from(std::env::var_os("APPDATA").unwrap());
        // As terminals, Explorer, PowerShell and browsers copy addresses.
        for pasted in [
            "%appdata%",
            "  %AppData%\r\n",
            "cd %APPDATA%",
            "CD /d \"%APPDATA%\"",
            "Set-Location $env:APPDATA",
            "$env:appdata",
            "${env:APPDATA}",
            "'%APPDATA%'",
            "pushd %appdata%;",
        ] {
            assert_eq!(address(pasted).unwrap().unwrap(), appdata, "{pasted:?}");
        }
        assert_eq!(
            address(r"cd C:\Users\alex\projects").unwrap().unwrap(),
            PathBuf::from(r"C:\Users\alex\projects")
        );
        assert_eq!(
            address(r#""C:\Program Files\Seek""#).unwrap().unwrap(),
            PathBuf::from(r"C:\Program Files\Seek")
        );
        assert_eq!(
            address("C:/Users/Public/Documents").unwrap().unwrap(),
            PathBuf::from(r"C:\Users\Public\Documents")
        );
        assert_eq!(
            address("file:///C:/Program%20Files/Seek").unwrap().unwrap(),
            PathBuf::from(r"C:\Program Files\Seek")
        );
        assert_eq!(
            address("file://server/share/a%20b").unwrap().unwrap(),
            PathBuf::from(r"\\server\share\a b")
        );
        assert_eq!(
            address(r"$env:TEMP\café").unwrap().unwrap(),
            std::env::temp_dir().join("café")
        );
        assert_eq!(
            address("C:users/Public").unwrap().unwrap(),
            PathBuf::from(r"C:\users\Public")
        );
        assert!(address("cd notes").is_none());
        assert!(address("1:2").is_none());
        assert!(address("C:\\a\nC:\\b").is_none());
    }
}
