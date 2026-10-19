//! Explicit administrator commands. Never called by an ordinary app launch.
use seek_engine::{helper, kernel::Handle};
use std::{
    io,
    path::PathBuf,
    time::{Duration, Instant},
};
use windows_sys::Win32::System::SystemServices::{ACCESS_ALLOWED_ACE_TYPE, ACCESS_DENIED_ACE_TYPE};
use windows_sys::Win32::{
    Foundation::*,
    Security::{Authorization::*, *},
    Storage::FileSystem::*,
    System::{Services::*, Threading::*},
    UI::Shell::*,
};
struct Service(SC_HANDLE);
fn context(operation: &str, error: io::Error) -> io::Error {
    io::Error::new(error.kind(), format!("{operation}: {error}"))
}
impl Drop for Service {
    fn drop(&mut self) {
        unsafe {
            CloseServiceHandle(self.0);
        }
    }
}
fn manager(access: u32) -> io::Result<Service> {
    let h = unsafe { OpenSCManagerW(std::ptr::null(), std::ptr::null(), access) };
    if h.is_null() {
        Err(io::Error::last_os_error())
    } else {
        Ok(Service(h))
    }
}
fn installed_path(sid: &str) -> io::Result<PathBuf> {
    use std::os::windows::ffi::OsStringExt;
    let mut raw = std::ptr::null_mut();
    let result =
        unsafe { SHGetKnownFolderPath(&FOLDERID_ProgramFiles, 0, std::ptr::null_mut(), &mut raw) };
    if result < 0 {
        return Err(io::Error::other(format!(
            "Program Files resolution failed: {result:#x}"
        )));
    }
    let mut n = 0;
    unsafe {
        while *raw.add(n) != 0 {
            n += 1;
        }
    }
    let path = PathBuf::from(std::ffi::OsString::from_wide(unsafe {
        std::slice::from_raw_parts(raw, n)
    }));
    unsafe {
        windows_sys::Win32::System::Com::CoTaskMemFree(raw as _);
    }
    Ok(path.join("Seek Helper").join(sid).join("seek-helper.exe"))
}
fn protected_directory(path: &std::path::Path) -> io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    let wide: Vec<_> = path.as_os_str().encode_wide().chain([0]).collect();
    let mut descriptor = std::ptr::null_mut();
    if unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            helper::wide("O:BAD:P(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)(A;OICI;GRGX;;;BU)").as_ptr(),
            SDDL_REVISION_1,
            &mut descriptor,
            std::ptr::null_mut(),
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    let security = SECURITY_ATTRIBUTES {
        nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: descriptor,
        bInheritHandle: 0,
    };
    let ok = unsafe { CreateDirectoryW(wide.as_ptr(), &security) };
    let error = io::Error::last_os_error();
    unsafe {
        LocalFree(descriptor);
    }
    if ok != 0 {
        return Ok(());
    }
    if error.raw_os_error() != Some(ERROR_ALREADY_EXISTS as i32) {
        return Err(error);
    }
    let attributes = unsafe { GetFileAttributesW(wide.as_ptr()) };
    if attributes == INVALID_FILE_ATTRIBUTES
        || attributes & FILE_ATTRIBUTE_REPARSE_POINT != 0
        || attributes & FILE_ATTRIBUTE_DIRECTORY == 0
    {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "Helper installation directory is not a regular protected directory",
        ));
    }
    let mut owner = std::ptr::null_mut();
    let mut dacl = std::ptr::null_mut();
    let mut security = std::ptr::null_mut();
    let error = unsafe {
        GetNamedSecurityInfoW(
            wide.as_ptr(),
            SE_FILE_OBJECT,
            OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION,
            &mut owner,
            std::ptr::null_mut(),
            &mut dacl,
            std::ptr::null_mut(),
            &mut security,
        )
    };
    if error != 0 {
        return Err(io::Error::from_raw_os_error(error as i32));
    }
    let safe = (|| -> bool {
        if owner.is_null()
            || dacl.is_null()
            || unsafe {
                IsWellKnownSid(owner, WinBuiltinAdministratorsSid)
                    | IsWellKnownSid(owner, WinLocalSystemSid)
            } == 0
        {
            return false;
        }
        let forbidden = FILE_WRITE_DATA
            | FILE_APPEND_DATA
            | FILE_WRITE_EA
            | FILE_WRITE_ATTRIBUTES
            | FILE_DELETE_CHILD
            | DELETE
            | WRITE_DAC
            | WRITE_OWNER
            | GENERIC_WRITE
            | GENERIC_ALL;
        for n in 0..unsafe { (*dacl).AceCount } as u32 {
            let mut ace = std::ptr::null_mut();
            if unsafe { GetAce(dacl, n, &mut ace) } == 0 {
                return false;
            }
            let header = unsafe { &*(ace as *const ACE_HEADER) };
            if header.AceType == ACCESS_DENIED_ACE_TYPE as u8 {
                continue;
            }
            if header.AceType != ACCESS_ALLOWED_ACE_TYPE as u8 {
                return false;
            }
            let allow = unsafe { &*(ace as *const ACCESS_ALLOWED_ACE) };
            let sid = &allow.SidStart as *const u32 as *mut _;
            if allow.Mask & forbidden != 0
                && unsafe {
                    IsWellKnownSid(sid, WinBuiltinAdministratorsSid)
                        | IsWellKnownSid(sid, WinLocalSystemSid)
                } == 0
            {
                return false;
            }
        }
        true
    })();
    unsafe {
        LocalFree(security);
    }
    if safe {
        Ok(())
    } else {
        Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "Existing helper directory has unsafe ownership/permissions",
        ))
    }
}
fn binary(path: &std::path::Path, sid: &str) -> String {
    format!("\"{}\" service --sid {sid}", path.display())
}
fn secure_file(path: &std::path::Path) -> io::Result<()> {
    let mut descriptor = std::ptr::null_mut();
    if unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            helper::wide("O:BAD:P(A;;FA;;;SY)(A;;FA;;;BA)(A;;GRGX;;;BU)").as_ptr(),
            SDDL_REVISION_1,
            &mut descriptor,
            std::ptr::null_mut(),
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    let mut owner = std::ptr::null_mut();
    let mut dacl = std::ptr::null_mut();
    let mut defaulted = 0;
    let mut present = 0;
    let result = (|| -> io::Result<()> {
        if unsafe { GetSecurityDescriptorOwner(descriptor, &mut owner, &mut defaulted) } == 0
            || unsafe {
                GetSecurityDescriptorDacl(descriptor, &mut present, &mut dacl, &mut defaulted)
            } == 0
        {
            return Err(io::Error::last_os_error());
        }
        let error = unsafe {
            SetNamedSecurityInfoW(
                helper::wide(&path.to_string_lossy()).as_ptr(),
                SE_FILE_OBJECT,
                OWNER_SECURITY_INFORMATION
                    | DACL_SECURITY_INFORMATION
                    | PROTECTED_DACL_SECURITY_INFORMATION,
                owner,
                std::ptr::null_mut(),
                dacl,
                std::ptr::null(),
            )
        };
        if error == 0 {
            Ok(())
        } else {
            Err(io::Error::from_raw_os_error(error as i32))
        }
    })();
    unsafe {
        LocalFree(descriptor);
    }
    result
}
fn check_binary(service: &Service, expected: &str) -> io::Result<()> {
    let mut n = 0;
    unsafe {
        QueryServiceConfigW(service.0, std::ptr::null_mut(), 0, &mut n);
    }
    if n == 0 {
        return Err(io::Error::last_os_error());
    }
    let mut buffer = vec![0usize; (n as usize).div_ceil(std::mem::size_of::<usize>())];
    if unsafe { QueryServiceConfigW(service.0, buffer.as_mut_ptr() as _, n, &mut n) } == 0 {
        return Err(io::Error::last_os_error());
    }
    let config = unsafe { &*(buffer.as_ptr() as *const QUERY_SERVICE_CONFIGW) };
    let mut n = 0;
    unsafe {
        while *config.lpBinaryPathName.add(n) != 0 {
            n += 1;
        }
    }
    let actual =
        String::from_utf16_lossy(unsafe { std::slice::from_raw_parts(config.lpBinaryPathName, n) });
    if actual == expected {
        Ok(())
    } else {
        Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "Existing service has a different binary; refusing to change it",
        ))
    }
}
fn stop(service: &Service) -> io::Result<()> {
    let mut process_status: SERVICE_STATUS_PROCESS = unsafe { std::mem::zeroed() };
    let mut needed = 0;
    if unsafe {
        QueryServiceStatusEx(
            service.0,
            SC_STATUS_PROCESS_INFO,
            &mut process_status as *mut _ as _,
            std::mem::size_of::<SERVICE_STATUS_PROCESS>() as u32,
            &mut needed,
        )
    } == 0
    {
        return Err(context(
            "Query Seek helper process before stopping",
            io::Error::last_os_error(),
        ));
    }
    if process_status.dwServiceType & SERVICE_WIN32_OWN_PROCESS == 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Seek helper must own its service process",
        ));
    }
    // Capture the exact process while its PID is valid. SERVICE_STOPPED can be
    // reported before the dispatcher exits and releases its executable image.
    let process = if process_status.dwProcessId != 0
        && matches!(
            process_status.dwCurrentState,
            SERVICE_RUNNING | SERVICE_PAUSE_PENDING | SERVICE_PAUSED | SERVICE_CONTINUE_PENDING
        ) {
        match unsafe {
            Handle::owned(OpenProcess(
                PROCESS_SYNCHRONIZE,
                0,
                process_status.dwProcessId,
            ))
        } {
            Ok(handle) => Some(handle),
            Err(error) if error.raw_os_error() == Some(ERROR_INVALID_PARAMETER as i32) => None,
            Err(error) => return Err(context("Open Seek helper process for exit wait", error)),
        }
    } else {
        None
    };
    let deadline = Instant::now() + Duration::from_secs(15);
    let mut s: SERVICE_STATUS = unsafe { std::mem::zeroed() };
    if unsafe { ControlService(service.0, SERVICE_CONTROL_STOP, &mut s) } == 0
        && unsafe { GetLastError() } != ERROR_SERVICE_NOT_ACTIVE
    {
        return Err(io::Error::last_os_error());
    }
    loop {
        if unsafe { QueryServiceStatus(service.0, &mut s) } == 0 {
            return Err(io::Error::last_os_error());
        }
        if s.dwCurrentState == SERVICE_STOPPED {
            if let Some(process) = &process {
                let remaining = deadline
                    .saturating_duration_since(Instant::now())
                    .as_millis()
                    .min(u32::MAX as u128) as u32;
                match unsafe { WaitForSingleObject(process.raw(), remaining) } {
                    WAIT_OBJECT_0 => {}
                    WAIT_TIMEOUT => {
                        return Err(io::Error::new(
                            io::ErrorKind::TimedOut,
                            "Seek helper reported stopped but its process did not exit",
                        ));
                    }
                    _ => {
                        return Err(context(
                            "Wait for Seek helper process exit",
                            io::Error::last_os_error(),
                        ));
                    }
                }
            }
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "Helper service did not stop",
            ));
        }
        // Only the explicit installer polls SCM; the service/app never polls.
        std::thread::sleep(Duration::from_millis(50));
    }
}
struct InstallRollback {
    path: PathBuf,
    temporary: PathBuf,
    backup: PathBuf,
    service: Option<Service>,
    created: bool,
    was_running: bool,
    replaced: bool,
    had_binary: bool,
    committed: bool,
}
impl Drop for InstallRollback {
    fn drop(&mut self) {
        if !self.committed {
            let result = (|| -> io::Result<()> {
                if let Some(service) = &self.service {
                    stop(service)?;
                    if self.created && unsafe { DeleteService(service.0) } == 0 {
                        return Err(io::Error::last_os_error());
                    }
                }
                if self.replaced {
                    if self.had_binary {
                        replace(&self.backup, &self.path)?;
                    } else {
                        std::fs::remove_file(&self.path)?;
                    }
                }
                if !self.created
                    && self.was_running
                    && let Some(service) = &self.service
                    && unsafe { StartServiceW(service.0, 0, std::ptr::null()) } == 0
                {
                    return Err(io::Error::last_os_error());
                }
                Ok(())
            })();
            if let Err(error) = result {
                eprintln!(
                    "Seek helper installation rollback failed: {error}. Run uninstall --sid to remove only this helper."
                );
            }
        }
        let _ = std::fs::remove_file(&self.temporary);
        // Preserve a backup if rollback failed; it is an owned, protected file.
        if self.committed || !self.replaced || !self.backup.exists() {
            let _ = std::fs::remove_file(&self.backup);
        }
        if !self.had_binary && !self.committed && !self.path.exists() {
            let _ = std::fs::remove_dir(self.path.parent().unwrap());
            let _ = std::fs::remove_dir(self.path.parent().unwrap().parent().unwrap());
        }
    }
}
fn replace(source: &std::path::Path, target: &std::path::Path) -> io::Result<()> {
    replace_until(source, target, Instant::now() + Duration::from_secs(5))
}
fn replace_until(
    source: &std::path::Path,
    target: &std::path::Path,
    deadline: Instant,
) -> io::Result<()> {
    loop {
        if unsafe {
            MoveFileExW(
                helper::wide(&source.to_string_lossy()).as_ptr(),
                helper::wide(&target.to_string_lossy()).as_ptr(),
                MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
            )
        } != 0
        {
            return Ok(());
        }
        let error = io::Error::last_os_error();
        // Mapped executable images can yield ACCESS_DENIED, not just a sharing
        // violation. Retry briefly without changing ownership or permissions.
        if !matches!(error.raw_os_error(), Some(code) if code == ERROR_SHARING_VIOLATION as i32 || code == ERROR_ACCESS_DENIED as i32)
            || Instant::now() >= deadline
        {
            return Err(error);
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}
pub fn install(sid: &str) -> io::Result<()> {
    helper::validate_sid(sid)?;
    let manager = manager(SC_MANAGER_CONNECT | SC_MANAGER_CREATE_SERVICE)
        .map_err(|e| context("Open service manager for Seek helper update", e))?;
    let path = installed_path(sid)?;
    let dir = path.parent().unwrap();
    protected_directory(dir.parent().unwrap())
        .map_err(|e| context("Check Seek helper parent directory", e))?;
    protected_directory(dir).map_err(|e| context("Check Seek helper user directory", e))?;
    let name = helper::wide(&helper::service_name(sid));
    let command = binary(&path, sid);
    let existing = unsafe {
        OpenServiceW(
            manager.0,
            name.as_ptr(),
            SERVICE_QUERY_CONFIG | SERVICE_QUERY_STATUS | SERVICE_STOP | SERVICE_START | WRITE_DAC,
        )
    };
    let existing = if existing.is_null() {
        let error = io::Error::last_os_error();
        if error.raw_os_error() != Some(ERROR_SERVICE_DOES_NOT_EXIST as i32) {
            return Err(context("Open existing Seek helper service", error));
        }
        None
    } else {
        let service = Service(existing);
        check_binary(&service, &command)?;
        Some(service)
    };
    let mut transaction = InstallRollback {
        path: path.clone(),
        temporary: dir.join(format!("seek-helper-{}.tmp", std::process::id())),
        backup: dir.join(format!("seek-helper-{}.backup", std::process::id())),
        service: existing,
        created: false,
        was_running: false,
        replaced: false,
        had_binary: path.exists(),
        committed: false,
    };
    if let Some(service) = &transaction.service {
        let mut status: SERVICE_STATUS = unsafe { std::mem::zeroed() };
        if unsafe { QueryServiceStatus(service.0, &mut status) } == 0 {
            return Err(io::Error::last_os_error());
        }
        transaction.was_running = status.dwCurrentState != SERVICE_STOPPED;
        stop(service).map_err(|e| context("Stop previous Seek helper", e))?;
    }
    let source = std::env::current_exe()?;
    if source != path {
        if transaction.had_binary {
            std::fs::copy(&path, &transaction.backup)
                .map_err(|e| context("Copy previous Seek helper to rollback backup", e))?;
            secure_file(&transaction.backup)
                .map_err(|e| context("Protect Seek helper rollback backup", e))?;
        }
        std::fs::copy(&source, &transaction.temporary)
            .map_err(|e| context("Stage new Seek helper executable", e))?;
        secure_file(&transaction.temporary)
            .map_err(|e| context("Protect staged Seek helper executable", e))?;
        replace(&transaction.temporary, &path)
            .map_err(|e| context("Replace stopped Seek helper executable", e))?;
        transaction.replaced = true;
    }
    if transaction.service.is_none() {
        let h = unsafe {
            CreateServiceW(
                manager.0,
                name.as_ptr(),
                helper::wide("Seek optional NTFS helper").as_ptr(),
                SERVICE_ALL_ACCESS,
                SERVICE_WIN32_OWN_PROCESS,
                SERVICE_AUTO_START,
                SERVICE_ERROR_NORMAL,
                helper::wide(&command).as_ptr(),
                std::ptr::null(),
                std::ptr::null_mut(),
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
            )
        };
        if h.is_null() {
            return Err(io::Error::last_os_error());
        }
        transaction.service = Some(Service(h));
        transaction.created = true;
    }
    let service = transaction.service.as_ref().unwrap();
    let mut descriptor = std::ptr::null_mut();
    if unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            helper::wide(&format!(
                "D:P(A;;GA;;;SY)(A;;GA;;;BA)(A;;CCLCSWRPWPLOCRRC;;;{sid})"
            ))
            .as_ptr(),
            SDDL_REVISION_1,
            &mut descriptor,
            std::ptr::null_mut(),
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    let ok = unsafe { SetServiceObjectSecurity(service.0, DACL_SECURITY_INFORMATION, descriptor) };
    unsafe {
        LocalFree(descriptor);
    }
    if ok == 0 {
        return Err(io::Error::last_os_error());
    }
    if unsafe { StartServiceW(service.0, 0, std::ptr::null()) } == 0
        && unsafe { GetLastError() } != ERROR_SERVICE_ALREADY_RUNNING
    {
        return Err(io::Error::last_os_error());
    }
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        let mut status: SERVICE_STATUS = unsafe { std::mem::zeroed() };
        if unsafe { QueryServiceStatus(service.0, &mut status) } == 0 {
            return Err(io::Error::last_os_error());
        }
        if status.dwCurrentState == SERVICE_RUNNING {
            break;
        }
        if status.dwCurrentState == SERVICE_STOPPED || Instant::now() >= deadline {
            return Err(io::Error::other("Seek helper did not reach running state"));
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    transaction.committed = true;
    println!(
        "Installed {} at {}. Restart Seek to use it.",
        helper::service_name(sid),
        path.display()
    );
    Ok(())
}
pub fn uninstall(sid: &str) -> io::Result<()> {
    helper::validate_sid(sid)?;
    let manager = manager(SC_MANAGER_CONNECT)?;
    let path = installed_path(sid)?;
    let handle = unsafe {
        OpenServiceW(
            manager.0,
            helper::wide(&helper::service_name(sid)).as_ptr(),
            SERVICE_QUERY_CONFIG | SERVICE_QUERY_STATUS | SERVICE_STOP | DELETE,
        )
    };
    if handle.is_null() {
        let e = io::Error::last_os_error();
        return if e.raw_os_error() == Some(ERROR_SERVICE_DOES_NOT_EXIST as i32) {
            Ok(())
        } else {
            Err(e)
        };
    }
    let service = Service(handle);
    check_binary(&service, &binary(&path, sid))?;
    protected_directory(path.parent().unwrap().parent().unwrap())?;
    protected_directory(path.parent().unwrap())?;
    stop(&service)?;
    if unsafe { DeleteService(service.0) } == 0 {
        return Err(io::Error::last_os_error());
    }
    drop(service);
    if std::env::current_exe()? == path {
        if unsafe {
            MoveFileExW(
                helper::wide(&path.to_string_lossy()).as_ptr(),
                std::ptr::null(),
                MOVEFILE_DELAY_UNTIL_REBOOT,
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
    } else {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            match std::fs::remove_file(&path) {
                Ok(()) => break,
                Err(e) if e.kind() == io::ErrorKind::NotFound => break,
                Err(e)
                    if e.raw_os_error() == Some(ERROR_SHARING_VIOLATION as i32)
                        && Instant::now() < deadline =>
                {
                    std::thread::sleep(Duration::from_millis(50))
                }
                Err(e) => return Err(e),
            }
        }
        let _ = std::fs::remove_dir(path.parent().unwrap());
        let _ = std::fs::remove_dir(path.parent().unwrap().parent().unwrap());
    }
    println!("Uninstalled {}.", helper::service_name(sid));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs::File, os::windows::io::AsRawHandle};
    use windows_sys::Win32::System::Memory::*;

    fn fixture() -> tempfile::TempDir {
        let repo = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .canonicalize()
            .unwrap();
        let base = repo.join(".bench-data/stage2-update-tests");
        std::fs::create_dir_all(&base).unwrap();
        assert!(base.canonicalize().unwrap().starts_with(&repo));
        tempfile::Builder::new()
            .prefix("seek-image-update-")
            .tempdir_in(base)
            .unwrap()
    }

    struct Image {
        view: usize,
        _mapping: Handle,
        _file: File,
    }
    impl Drop for Image {
        fn drop(&mut self) {
            unsafe {
                UnmapViewOfFile(MEMORY_MAPPED_VIEW_ADDRESS {
                    Value: self.view as _,
                });
            }
        }
    }
    fn map_image(path: &std::path::Path) -> Image {
        let file = File::open(path).unwrap();
        let mapping = unsafe {
            Handle::owned(CreateFileMappingW(
                file.as_raw_handle(),
                std::ptr::null(),
                PAGE_READONLY | SEC_IMAGE,
                0,
                0,
                std::ptr::null(),
            ))
            .unwrap()
        };
        let view = unsafe { MapViewOfFile(mapping.raw(), FILE_MAP_READ, 0, 0, 0) };
        assert!(!view.Value.is_null(), "{}", io::Error::last_os_error());
        Image {
            view: view.Value as usize,
            _mapping: mapping,
            _file: file,
        }
    }

    #[test]
    fn executable_image_access_denied_is_retried_until_the_image_is_released() {
        let dir = fixture();
        let target = dir.path().join("old-helper.exe");
        let source = dir.path().join("new-helper.tmp");
        std::fs::copy(std::env::current_exe().unwrap(), &target).unwrap();
        std::fs::write(&source, b"new helper payload").unwrap();
        let image = map_image(&target);
        assert_eq!(
            unsafe {
                MoveFileExW(
                    helper::wide(&source.to_string_lossy()).as_ptr(),
                    helper::wide(&target.to_string_lossy()).as_ptr(),
                    MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
                )
            },
            0
        );
        let error = io::Error::last_os_error();
        assert_eq!(error.raw_os_error(), Some(ERROR_ACCESS_DENIED as i32));
        println!(
            "Mapped executable replacement reproduced Windows error {}",
            error.raw_os_error().unwrap()
        );
        let release = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(150));
            drop(image);
        });
        let result = replace(&source, &target);
        release.join().unwrap();
        result.unwrap();
        assert_eq!(std::fs::read(&target).unwrap(), b"new helper payload");
        assert!(!source.exists());
    }

    #[test]
    fn permanent_access_denial_is_bounded_and_keeps_both_original_files() {
        let dir = fixture();
        let target = dir.path().join("old-helper.exe");
        let source = dir.path().join("new-helper.tmp");
        std::fs::write(&target, b"old helper payload").unwrap();
        std::fs::write(&source, b"new helper payload").unwrap();
        let original_permissions = std::fs::metadata(&target).unwrap().permissions();
        let mut permissions = original_permissions.clone();
        permissions.set_readonly(true);
        std::fs::set_permissions(&target, permissions).unwrap();
        let started = Instant::now();
        let result = replace_until(&source, &target, started + Duration::from_millis(100));
        std::fs::set_permissions(&target, original_permissions).unwrap();
        assert_eq!(
            result.unwrap_err().raw_os_error(),
            Some(ERROR_ACCESS_DENIED as i32)
        );
        assert!(started.elapsed() < Duration::from_secs(1));
        assert_eq!(std::fs::read(&target).unwrap(), b"old helper payload");
        assert_eq!(std::fs::read(&source).unwrap(), b"new helper payload");
    }
}
