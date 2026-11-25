//! Report a Bug (Phase 8.5): a prefilled GitHub issue opened in the browser.
//! Nothing is sent by Seek; the person reads and edits the issue before
//! submitting it, and only the version, the Windows version and the path of
//! the diagnostics log are filled in.

pub const REPOSITORY: &str = "https://github.com/sanirudh17/Seek";

/// "Windows 11 Home 25H2 (build 26300)", from the registry.
pub fn windows_version() -> String {
    use windows_sys::Win32::System::Registry::*;
    let read = |name: &str| -> Option<String> {
        let key: Vec<u16> = "SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion\0"
            .encode_utf16()
            .collect();
        let value: Vec<u16> = format!("{name}\0").encode_utf16().collect();
        let mut buffer = [0u16; 128];
        let mut bytes = (buffer.len() * 2) as u32;
        let status = unsafe {
            RegGetValueW(
                HKEY_LOCAL_MACHINE,
                key.as_ptr(),
                value.as_ptr(),
                RRF_RT_REG_SZ,
                std::ptr::null_mut(),
                buffer.as_mut_ptr().cast(),
                &mut bytes,
            )
        };
        (status == 0).then(|| {
            let len = (bytes as usize / 2).saturating_sub(1).min(buffer.len());
            String::from_utf16_lossy(&buffer[..len])
        })
    };
    let build = read("CurrentBuild").unwrap_or_default();
    let mut product = read("ProductName").unwrap_or_else(|| "Windows".into());
    // Windows 11 still reports "Windows 10" in ProductName.
    if build.parse::<u32>().is_ok_and(|b| b >= 22000) {
        product = product.replace("Windows 10", "Windows 11");
    }
    match read("DisplayVersion") {
        Some(display) => format!("{product} {display} (build {build})"),
        None => format!("{product} (build {build})"),
    }
}

/// The new-issue address with its body filled in.
pub fn issue_url(version: &str, windows: &str, log: &std::path::Path) -> String {
    let body = format!(
        "**What happened**\n\n\n**What you expected**\n\n\n**Steps to reproduce**\n1. \n\n---\n- Seek: {version}\n- Windows: {windows}\n- Diagnostics log: `{}` (attach it if it helps; check it first, it lists file paths)\n",
        log.display()
    );
    let mut url = url::Url::parse(&format!("{REPOSITORY}/issues/new")).expect("valid url");
    url.query_pairs_mut()
        .append_pair("labels", "bug")
        .append_pair("body", &body);
    url.into()
}

#[cfg(test)]
mod tests {
    #[test]
    fn issue_url_carries_only_version_windows_and_log_path() {
        let url = super::issue_url(
            "1.2.3",
            "Windows 11 Home 25H2 (build 26300)",
            std::path::Path::new(r"C:\Users\me\AppData\Local\Seek\logs\preview-host.log"),
        );
        assert!(url.starts_with("https://github.com/sanirudh17/Seek/issues/new?"));
        let parsed = url::Url::parse(&url).unwrap();
        let body: String = parsed
            .query_pairs()
            .find(|(k, _)| k == "body")
            .unwrap()
            .1
            .into_owned();
        assert!(body.contains("Seek: 1.2.3"));
        assert!(body.contains("build 26300"));
        assert!(body.contains(r"logs\preview-host.log"));
        assert!(!super::windows_version().is_empty());
    }
}
