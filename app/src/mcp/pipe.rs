//! The local channel between `seek.exe --mcp` and the running app, after
//! Glint's control channel: a per-user named pipe that refuses remote
//! clients and lets in only this user's account (and SYSTEM). Each
//! connection carries newline-delimited JSON requests and replies.
use serde_json::Value;
use std::{
    io::{BufRead, BufReader, Write},
    os::windows::io::FromRawHandle,
    path::Path,
    sync::Arc,
    time::{Duration, Instant},
};

pub type Handler = Arc<dyn Fn(Value) -> Value + Send + Sync>;

pub fn name(data: &Path) -> Result<String, String> {
    Ok(format!(r"\\.\pipe\{}-agents", crate::instance::key(data)?))
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}

/// Serves `handler` on the pipe from its own threads, for the life of the
/// app. Each connection gets a thread; requests on it are answered in order.
pub fn serve(name: String, handler: Handler) -> Result<(), String> {
    use windows_sys::Win32::{
        Foundation::{ERROR_PIPE_CONNECTED, GetLastError, INVALID_HANDLE_VALUE, LocalFree},
        Security::{Authorization::ConvertStringSecurityDescriptorToSecurityDescriptorW, SECURITY_ATTRIBUTES},
        Storage::FileSystem::PIPE_ACCESS_DUPLEX,
        System::Pipes::{
            ConnectNamedPipe, CreateNamedPipeW, PIPE_READMODE_BYTE, PIPE_REJECT_REMOTE_CLIENTS, PIPE_TYPE_BYTE,
            PIPE_UNLIMITED_INSTANCES, PIPE_WAIT,
        },
    };
    let sid = seek_engine::helper::user_sid().map_err(|e| e.to_string())?;
    // Protected DACL: this user and SYSTEM only.
    let sddl = wide(&format!("D:P(A;;GA;;;{sid})(A;;GA;;;SY)"));
    std::thread::Builder::new()
        .name("seek-agents-pipe".into())
        .spawn(move || {
            let pipe = wide(&name);
            loop {
                let mut descriptor = std::ptr::null_mut();
                let ok = unsafe {
                    ConvertStringSecurityDescriptorToSecurityDescriptorW(sddl.as_ptr(), 1, &mut descriptor, std::ptr::null_mut())
                };
                if ok == 0 {
                    return;
                }
                let attributes = SECURITY_ATTRIBUTES {
                    nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
                    lpSecurityDescriptor: descriptor,
                    bInheritHandle: 0,
                };
                let raw = unsafe {
                    CreateNamedPipeW(
                        pipe.as_ptr(),
                        PIPE_ACCESS_DUPLEX,
                        PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
                        PIPE_UNLIMITED_INSTANCES,
                        64 * 1024,
                        64 * 1024,
                        0,
                        &attributes,
                    )
                };
                unsafe { LocalFree(descriptor) };
                if raw == INVALID_HANDLE_VALUE {
                    return;
                }
                let connected = unsafe { ConnectNamedPipe(raw, std::ptr::null_mut()) } != 0
                    || unsafe { GetLastError() } == ERROR_PIPE_CONNECTED;
                // Owns the handle from here; closing the file closes the pipe.
                let file = unsafe { std::fs::File::from_raw_handle(raw as _) };
                if !connected {
                    continue;
                }
                let handler = handler.clone();
                let _ = std::thread::Builder::new()
                    .name("seek-agents-client".into())
                    .spawn(move || answer(file, handler));
            }
        })
        .map(|_| ())
        .map_err(|e| e.to_string())
}

fn answer(file: std::fs::File, handler: Handler) {
    let Ok(mut writer) = file.try_clone() else { return };
    let mut reader = BufReader::new(file);
    let mut line = String::new();
    loop {
        line.clear();
        match reader.read_line(&mut line) {
            Ok(0) | Err(_) => return,
            Ok(_) => {}
        }
        let reply = match serde_json::from_str::<Value>(&line) {
            Ok(request) => handler(request),
            Err(error) => serde_json::json!({"error": {"code": "invalid_argument", "message": format!("Malformed request: {error}"), "retryable": false}}),
        };
        if writer
            .write_all(format!("{reply}\n").as_bytes())
            .and_then(|_| writer.flush())
            .is_err()
        {
            return;
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum CallError {
    /// No app is listening.
    NotRunning,
    Failed(String),
}

/// One request and its reply on a fresh connection.
pub fn call(name: &str, request: &Value) -> Result<Value, CallError> {
    use windows_sys::Win32::System::Pipes::WaitNamedPipeW;
    const ERROR_PIPE_BUSY: i32 = 231;
    let deadline = Instant::now() + Duration::from_secs(5);
    let file = loop {
        match std::fs::OpenOptions::new().read(true).write(true).open(name) {
            Ok(file) => break file,
            Err(e) if e.raw_os_error() == Some(ERROR_PIPE_BUSY) && Instant::now() < deadline => unsafe {
                WaitNamedPipeW(wide(name).as_ptr(), 500);
            },
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Err(CallError::NotRunning),
            Err(e) => return Err(CallError::Failed(e.to_string())),
        }
    };
    let mut writer = file.try_clone().map_err(|e| CallError::Failed(e.to_string()))?;
    writer
        .write_all(format!("{request}\n").as_bytes())
        .and_then(|_| writer.flush())
        .map_err(|e| CallError::Failed(e.to_string()))?;
    let mut line = String::new();
    BufReader::new(file)
        .read_line(&mut line)
        .map_err(|e| CallError::Failed(e.to_string()))?;
    serde_json::from_str(&line).map_err(|_| CallError::Failed("Seek sent an unreadable reply.".into()))
}

/// Whether a Seek for this data folder is running (its single-instance
/// lock exists), whether or not its pipe is answering at this moment.
pub fn app_running(data: &Path) -> bool {
    use windows_sys::Win32::{Foundation::CloseHandle, System::Threading::OpenMutexW};
    let Ok(key) = crate::instance::key(data) else { return false };
    const SYNCHRONIZE: u32 = 0x0010_0000;
    let handle = unsafe { OpenMutexW(SYNCHRONIZE, 0, wide(&format!("Local\\{key}")).as_ptr()) };
    if handle.is_null() {
        return false;
    }
    unsafe { CloseHandle(handle) };
    true
}

/// Starts Seek in the background (minimized, without focus) when it isn't
/// running, inheriting no handles, so it outlives this process without
/// holding the agent's pipes open.
pub fn launch(exe: &Path, data: &Path) -> Result<(), String> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::{
        Foundation::CloseHandle,
        System::Threading::{
            CREATE_NEW_PROCESS_GROUP, CreateProcessW, DETACHED_PROCESS, PROCESS_INFORMATION, STARTUPINFOW,
        },
    };
    let application: Vec<u16> = exe.as_os_str().encode_wide().chain(Some(0)).collect();
    let mut command = wide(&format!("\"{}\" --background --data \"{}\"", exe.display(), data.display()));
    let startup = STARTUPINFOW {
        cb: std::mem::size_of::<STARTUPINFOW>() as u32,
        ..unsafe { std::mem::zeroed() }
    };
    let mut info: PROCESS_INFORMATION = unsafe { std::mem::zeroed() };
    let ok = unsafe {
        CreateProcessW(
            application.as_ptr(),
            command.as_mut_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            0,
            DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP,
            std::ptr::null(),
            std::ptr::null(),
            &startup,
            &mut info,
        )
    };
    if ok == 0 {
        return Err(format!("Couldn't start Seek: {}", std::io::Error::last_os_error()));
    }
    unsafe {
        CloseHandle(info.hThread);
        CloseHandle(info.hProcess);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_request_round_trips_and_a_missing_app_is_not_running() {
        let dir = tempfile::tempdir().unwrap();
        let pipe = name(dir.path()).unwrap();
        assert_eq!(call(&pipe, &serde_json::json!({})), Err(CallError::NotRunning));
        serve(pipe.clone(), Arc::new(|request| serde_json::json!({"echo": request["n"]}))).unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        let reply = loop {
            match call(&pipe, &serde_json::json!({"n": 7})) {
                Ok(reply) => break reply,
                Err(_) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(20)),
                Err(e) => panic!("{e:?}"),
            }
        };
        assert_eq!(reply["echo"], 7);
    }
}
