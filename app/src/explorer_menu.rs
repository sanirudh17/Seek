//! Explorer integration, both opt-in and per-user (HKCU only, so no
//! administrator rights); every command is the running seek.exe's real path.
//!
//! - The right-click menu (Phase 8.6): "Open in Seek" on folders, drives and
//!   a folder's background, and "Show in Seek" on files. On Windows 11 these
//!   sit under "Show more options".
//! - The default file manager: folders and drives that are opened (including
//!   double-clicks in Explorer and other apps opening a folder) and Win+E open
//!   in Seek. This is how the Files app does it: a default verb for
//!   Directory and Drive, and File Explorer's "open new window" command.
//!   Explorer's own verbs stay, so "Open" and "Open in new window" still
//!   work; places Seek can't show (Control Panel, Recycle Bin…) are handed to
//!   Explorer; turning it off deletes only Seek's keys, restoring Windows.
use std::path::Path;
use windows_sys::Win32::System::Registry::*;

const KEYS: [&str; 4] = [
    r"Software\Classes\Directory\shell\Seek",
    r"Software\Classes\Directory\Background\shell\Seek",
    r"Software\Classes\Drive\shell\Seek",
    r"Software\Classes\*\shell\Seek",
];

/// The verb that becomes the default for folders and drives.
const VERB: &str = "SeekOpen";
const DEFAULT_PARENTS: [&str; 2] = [r"Software\Classes\Directory\shell", r"Software\Classes\Drive\shell"];
/// File Explorer's own item, which Win+E and the taskbar's Explorer open.
const WIN_E: &str = r"Software\Classes\CLSID\{52205fd8-5dfb-447d-801a-d0b52f2e83e1}\shell\opennewwindow";

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(Some(0)).collect()
}

fn set(key: &str, name: Option<&str>, value: &str) -> Result<(), String> {
    let mut handle: HKEY = std::ptr::null_mut();
    let status = unsafe {
        RegCreateKeyExW(
            HKEY_CURRENT_USER,
            wide(key).as_ptr(),
            0,
            std::ptr::null(),
            REG_OPTION_NON_VOLATILE,
            KEY_SET_VALUE,
            std::ptr::null(),
            &mut handle,
            std::ptr::null_mut(),
        )
    };
    if status != 0 {
        return Err(std::io::Error::from_raw_os_error(status as i32).to_string());
    }
    let data = wide(value);
    let name = name.map(wide);
    let status = unsafe {
        RegSetValueExW(
            handle,
            name.as_ref().map_or(std::ptr::null(), |n| n.as_ptr()),
            0,
            REG_SZ,
            data.as_ptr().cast(),
            (data.len() * 2) as u32,
        )
    };
    unsafe {
        RegCloseKey(handle);
    }
    if status != 0 {
        return Err(std::io::Error::from_raw_os_error(status as i32).to_string());
    }
    Ok(())
}

/// The command each entry runs: `"<exe>" --open "%V"`.
pub fn command(exe: &Path) -> String {
    format!("\"{}\" --open \"%V\"", exe.display())
}

/// Seek as an application Windows knows: listed in "Open with" and "Choose
/// an app" (opening a file shows it in Seek), with its name and icon, and
/// in Settings > Apps through its registered capabilities.
const APPLICATION: &str = r"Software\Classes\Applications\seek.exe";
const CAPABILITIES: &str = r"Software\Seek\Capabilities";
const REGISTERED: &str = r"Software\RegisteredApplications";

fn register_application(exe: &Path) -> Result<(), String> {
    let icon = format!("\"{}\",0", exe.display());
    set(APPLICATION, Some("FriendlyAppName"), "Seek")?;
    set(&format!(r"{APPLICATION}\DefaultIcon"), None, &icon)?;
    set(&format!(r"{APPLICATION}\shell\open"), Some("FriendlyAppName"), "Seek")?;
    set(&format!(r"{APPLICATION}\shell\open\command"), None, &file_command(exe))?;
    set(CAPABILITIES, Some("ApplicationName"), "Seek")?;
    set(CAPABILITIES, Some("ApplicationDescription"), "Find, browse and organize your files")?;
    set(CAPABILITIES, Some("ApplicationIcon"), &icon)?;
    set(REGISTERED, Some("Seek"), CAPABILITIES)?;
    Ok(())
}

pub fn install() -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    register_application(&exe)?;
    for key in KEYS {
        let file = key.contains(r"\*\");
        set(key, None, if file { "Show in Seek" } else { "Open in Seek" })?;
        set(key, Some("Icon"), &format!("\"{}\",0", exe.display()))?;
        set(
            &format!(r"{key}\command"),
            None,
            &if file { file_command(&exe) } else { command(&exe) },
        )?;
    }
    Ok(())
}

/// The command "Show in Seek" runs on a file: its folder, showing it.
pub fn file_command(exe: &Path) -> String {
    format!("\"{}\" --open \"%1\"", exe.display())
}

fn read(key: &str, name: Option<&str>) -> Option<String> {
    let mut size = 0u32;
    let name = name.map(wide);
    let name_ptr = name.as_ref().map_or(std::ptr::null(), |n| n.as_ptr());
    let key = wide(key);
    let status = unsafe {
        RegGetValueW(HKEY_CURRENT_USER, key.as_ptr(), name_ptr, RRF_RT_REG_SZ, std::ptr::null_mut(), std::ptr::null_mut(), &mut size)
    };
    if status != 0 || size < 2 {
        return None;
    }
    let mut buffer = vec![0u16; size as usize / 2];
    let status = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            key.as_ptr(),
            name_ptr,
            RRF_RT_REG_SZ,
            std::ptr::null_mut(),
            buffer.as_mut_ptr().cast(),
            &mut size,
        )
    };
    (status == 0).then(|| String::from_utf16_lossy(&buffer).trim_end_matches('\0').to_string())
}

fn delete_tree(key: &str) -> Result<(), String> {
    let status = unsafe { RegDeleteTreeW(HKEY_CURRENT_USER, wide(key).as_ptr()) };
    if status != 0 && status != 2 {
        return Err(std::io::Error::from_raw_os_error(status as i32).to_string());
    }
    Ok(())
}

/// Whether Seek is the default file manager.
pub fn is_default() -> bool {
    read(DEFAULT_PARENTS[0], None).as_deref() == Some(VERB)
}

/// Makes Seek the default file manager, or gives the role back to Explorer.
pub fn set_default(on: bool) -> Result<(), String> {
    if on {
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        for parent in DEFAULT_PARENTS {
            let verb = format!(r"{parent}\{VERB}");
            set(&verb, None, "Open in Seek")?;
            set(&verb, Some("Icon"), &format!("\"{}\",0", exe.display()))?;
            set(&format!(r"{verb}\command"), None, &format!("\"{}\" --open \"%1\"", exe.display()))?;
            set(parent, None, VERB)?;
        }
        set(&format!(r"{WIN_E}\command"), None, &format!("\"{}\"", exe.display()))?;
        // An empty DelegateExecute makes Windows run the command above
        // instead of File Explorer's own handler.
        set(&format!(r"{WIN_E}\command"), Some("DelegateExecute"), "")?;
        Ok(())
    } else {
        for parent in DEFAULT_PARENTS {
            if read(parent, None).as_deref() == Some(VERB) {
                let status = unsafe {
                    RegDeleteKeyValueW(HKEY_CURRENT_USER, wide(parent).as_ptr(), std::ptr::null())
                };
                if status != 0 && status != 2 {
                    return Err(std::io::Error::from_raw_os_error(status as i32).to_string());
                }
            }
            delete_tree(&format!(r"{parent}\{VERB}"))?;
        }
        delete_tree(WIN_E)
    }
}

/// Open in Seek and Show in Seek are always in Explorer's menu: added when
/// missing, and after Seek moves (a new install folder) pointed at the
/// running seek.exe, so opening a folder never runs a missing program. The
/// uninstaller removes them.
///
/// A build run from a cargo `target` folder (a developer's test copy) never
/// touches them: otherwise testing a build takes the folder handler, the menu
/// and Win+E away from the installed Seek and points them at a binary that is
/// rebuilt, locked or deleted. The installed Seek takes them back on its next
/// start, since any entry that isn't its own counts as stale.
pub fn refresh() {
    let Ok(exe) = std::env::current_exe() else { return };
    if development_build(&exe) {
        return;
    }
    let ours = format!("\"{}\"", exe.display());
    let stale = |key: &str| read(&format!(r"{key}\command"), None).is_some_and(|c| !c.starts_with(&ours));
    let unregistered = read(&format!(r"{APPLICATION}\shell\open\command"), None)
        .is_none_or(|c| !c.starts_with(&ours))
        || read(REGISTERED, Some("Seek")).is_none();
    if unregistered || KEYS.iter().any(|key| stale(key) || read(&format!(r"{key}\command"), None).is_none()) {
        let _ = install();
    }
    if is_default() && (stale(&format!(r"{}\{VERB}", DEFAULT_PARENTS[0])) || stale(WIN_E)) {
        let _ = set_default(true);
    }
}

/// A seek.exe built by cargo and run from its `target` folder.
fn development_build(exe: &Path) -> bool {
    exe.components()
        .any(|c| c.as_os_str().eq_ignore_ascii_case("target"))
        && exe
            .components()
            .any(|c| ["debug", "release"].iter().any(|p| c.as_os_str().eq_ignore_ascii_case(p)))
}

/// Takes the entries out again (the uninstaller does this for real).
#[cfg(test)]
pub fn remove() -> Result<(), String> {
    for key in KEYS {
        // Already gone is fine.
        delete_tree(key)?;
    }
    Ok(())
}

#[cfg(test)]
pub fn installed() -> bool {
    let mut handle: HKEY = std::ptr::null_mut();
    let key = wide(&format!(r"{}\command", KEYS[0]));
    let status =
        unsafe { RegOpenKeyExW(HKEY_CURRENT_USER, key.as_ptr(), 0, KEY_READ, &mut handle) };
    if status == 0 {
        unsafe {
            RegCloseKey(handle);
        }
    }
    status == 0
}

#[cfg(test)]
mod tests {
    /// Run alone (`cargo test -p seek explorer_keys -- --ignored`): HKCU is
    /// redirected for the whole process to a throwaway key, so the person's
    /// real Explorer settings are never touched.
    #[test]
    #[ignore = "redirects HKCU for the whole test process; run alone"]
    fn explorer_keys_install_and_give_everything_back() {
        use windows_sys::Win32::System::Registry::*;
        let scratch = super::wide(&format!(r"Software\SeekRegistryTest\{}", std::process::id()));
        let mut root: HKEY = std::ptr::null_mut();
        unsafe {
            assert_eq!(
                RegCreateKeyExW(HKEY_CURRENT_USER, scratch.as_ptr(), 0, std::ptr::null(), 0, KEY_ALL_ACCESS, std::ptr::null(), &mut root, std::ptr::null_mut()),
                0
            );
            assert_eq!(RegOverridePredefKey(HKEY_CURRENT_USER, root), 0);
        }
        let result = std::panic::catch_unwind(|| {
            assert!(!super::installed() && !super::is_default());
            super::install().unwrap();
            assert!(super::installed());
            assert!(super::read(r"Software\Classes\*\shell\Seek", None).as_deref() == Some("Show in Seek"));
            // Listed by Windows as an app: "Open with", "Choose an app".
            assert_eq!(super::read(super::APPLICATION, Some("FriendlyAppName")).as_deref(), Some("Seek"));
            assert!(
                super::read(&format!(r"{}\shell\open\command", super::APPLICATION), None)
                    .is_some_and(|c| c.ends_with(r#"--open "%1""#))
            );
            assert_eq!(super::read(super::REGISTERED, Some("Seek")).as_deref(), Some(super::CAPABILITIES));
            super::set_default(true).unwrap();
            assert!(super::is_default());
            let exe = std::env::current_exe().unwrap();
            assert_eq!(
                super::read(r"Software\Classes\Drive\shell\SeekOpen\command", None).unwrap(),
                format!("\"{}\" --open \"%1\"", exe.display())
            );
            assert_eq!(super::read(&format!(r"{}\command", super::WIN_E), Some("DelegateExecute")).unwrap_or_default(), "");
            super::set_default(false).unwrap();
            assert!(!super::is_default());
            assert!(super::read(r"Software\Classes\Directory\shell", None).is_none());
            assert!(super::read(&format!(r"{}\command", super::WIN_E), None).is_none());
            // The menu stays until it is removed on its own.
            assert!(super::installed());
            super::remove().unwrap();
            assert!(!super::installed());
        });
        unsafe {
            RegOverridePredefKey(HKEY_CURRENT_USER, std::ptr::null_mut());
            RegCloseKey(root);
            RegDeleteTreeW(HKEY_CURRENT_USER, scratch.as_ptr());
            RegDeleteKeyW(HKEY_CURRENT_USER, super::wide(r"Software\SeekRegistryTest").as_ptr());
        }
        result.unwrap();
    }

    #[test]
    fn test_builds_leave_explorer_alone() {
        use std::path::Path;
        assert!(super::development_build(Path::new(r"C:\src\Seek\target\x86_64-pc-windows-msvc\debug\seek.exe")));
        assert!(super::development_build(Path::new(r"C:\src\Seek\target\release\seek.exe")));
        assert!(!super::development_build(Path::new(r"C:\Users\a\AppData\Local\Programs\Seek\seek.exe")));
        assert!(!super::development_build(Path::new(r"D:\Apps\Seek\seek.exe")));
    }

    #[test]
    fn command_quotes_the_exe_and_the_folder() {
        let command = super::command(std::path::Path::new(r"C:\Program Files\Seek\seek.exe"));
        assert_eq!(command, r#""C:\Program Files\Seek\seek.exe" --open "%V""#);
        let file = super::file_command(std::path::Path::new(r"C:\Program Files\Seek\seek.exe"));
        assert_eq!(file, r#""C:\Program Files\Seek\seek.exe" --open "%1""#);
    }
}
