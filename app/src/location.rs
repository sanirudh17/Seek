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

/// Complete a typed Windows path prefix against existing disk entries
/// (directories first, case-insensitive, bounded to 512 entries). Returns the
/// single match or longest common prefix if multiple match.
#[cfg_attr(not(test), allow(dead_code))]
pub fn tab_complete(typed: &str, show_hidden: bool) -> Option<String> {
    tab_complete_candidates(typed, None, show_hidden).map(|(chosen, _)| chosen)
}

/// Like [`tab_complete`], also returning the full ordered candidate list so
/// repeated `Tab` / `Shift+Tab` presses can cycle through matching entries,
/// and falling back to `base` when `typed` is a plain relative folder prefix.
pub fn tab_complete_candidates(
    typed: &str,
    base: Option<&std::path::Path>,
    show_hidden: bool,
) -> Option<(String, Vec<String>)> {
    let trimmed = typed.trim();
    if trimmed.is_empty() {
        return None;
    }
    let expanded = if let Some(addr) = address(trimmed) {
        addr.ok()?
    } else if let Some(base_dir) = base
        && !trimmed.contains([':', '*', '?', '"', '<', '>', '|'])
    {
        let cleaned = base_dir
            .to_string_lossy()
            .trim_start_matches(r"\\?\")
            .to_owned();
        let rel = trimmed.replace('/', r"\");
        PathBuf::from(cleaned).join(rel)
    } else {
        return None;
    };
    let raw = expanded.to_string_lossy().into_owned();
    let (dir, prefix) = if raw.ends_with('\\') || raw.ends_with('/') {
        (PathBuf::from(&raw), String::new())
    } else {
        let parent = expanded.parent()?.to_path_buf();
        let name = expanded
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        (parent, name)
    };
    let entries = std::fs::read_dir(&dir).ok()?;
    let prefix_lower = prefix.to_ascii_lowercase();
    let mut dirs = Vec::new();
    let mut files = Vec::new();
    for entry in entries.flatten().take(512) {
        let name = entry.file_name().to_string_lossy().into_owned();
        if !show_hidden && name.starts_with('.') {
            continue;
        }
        if !prefix_lower.is_empty() && !name.to_ascii_lowercase().starts_with(&prefix_lower) {
            continue;
        }
        let is_dir = entry.file_type().is_ok_and(|t| t.is_dir());
        if is_dir {
            dirs.push(name);
        } else {
            files.push(name);
        }
    }
    dirs.sort_by_key(|s| s.to_ascii_lowercase());
    files.sort_by_key(|s| s.to_ascii_lowercase());
    let matches: Vec<(String, bool)> = dirs
        .into_iter()
        .map(|n| (n, true))
        .chain(files.into_iter().map(|n| (n, false)))
        .collect();
    if matches.is_empty() {
        return None;
    }
    let candidates: Vec<String> = matches
        .iter()
        .map(|(name, is_dir)| {
            let mut out = dir.join(name).to_string_lossy().into_owned();
            if *is_dir && !out.ends_with('\\') {
                out.push('\\');
            }
            out
        })
        .collect();
    let chosen = if matches.len() == 1 {
        candidates[0].clone()
    } else {
        let first = &matches[0].0;
        let mut lcp_len = first.len();
        for (other, _) in &matches[1..] {
            lcp_len = first
                .chars()
                .zip(other.chars())
                .take_while(|(a, b)| a.eq_ignore_ascii_case(b))
                .map(|(a, _)| a.len_utf8())
                .sum::<usize>()
                .min(lcp_len);
        }
        if lcp_len > prefix.len() {
            dir.join(&first[..lcp_len]).to_string_lossy().into_owned()
        } else {
            candidates[0].clone()
        }
    };
    Some((chosen, candidates))
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

    #[test]
    fn tab_complete_completes_unique_and_common_prefix() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("ProjectsAlpha")).unwrap();
        std::fs::create_dir(dir.path().join("ProjectsBeta")).unwrap();
        std::fs::create_dir(dir.path().join("UniqueFolder")).unwrap();
        let u = tab_complete(&format!("{}\\Uni", dir.path().display()), false).unwrap();
        assert!(u.ends_with("UniqueFolder\\"), "got {u}");
        let p = tab_complete(&format!("{}\\Pro", dir.path().display()), false).unwrap();
        assert!(p.ends_with("Projects"), "got {p}");
        let (rel, cands) = tab_complete_candidates("Pro", Some(dir.path()), false).unwrap();
        assert!(rel.ends_with("Projects"), "got {rel}");
        assert_eq!(cands.len(), 2);
        assert!(cands[0].ends_with("ProjectsAlpha\\"));
        assert!(cands[1].ends_with("ProjectsBeta\\"));
    }
}
