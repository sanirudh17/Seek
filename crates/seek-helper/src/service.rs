use seek_engine::{helper, kernel::Cancel};
use std::{
    io,
    sync::{Mutex, OnceLock},
};
use windows_sys::Win32::System::Services::*;
static SID: OnceLock<String> = OnceLock::new();
static STOP: OnceLock<Cancel> = OnceLock::new();
static ACTIVE: Mutex<Option<Cancel>> = Mutex::new(None);
static STATUS: Mutex<usize> = Mutex::new(0);
fn status(state: u32, error: u32) {
    let handle = *STATUS.lock().unwrap() as SERVICE_STATUS_HANDLE;
    if handle.is_null() {
        return;
    }
    let s = SERVICE_STATUS {
        dwServiceType: SERVICE_WIN32_OWN_PROCESS,
        dwCurrentState: state,
        dwControlsAccepted: if state == SERVICE_RUNNING {
            SERVICE_ACCEPT_STOP | SERVICE_ACCEPT_SHUTDOWN
        } else {
            0
        },
        dwWin32ExitCode: error,
        dwServiceSpecificExitCode: 0,
        dwCheckPoint: 0,
        dwWaitHint: if state == SERVICE_STOP_PENDING {
            10000
        } else {
            0
        },
    };
    unsafe {
        SetServiceStatus(handle, &s);
    }
}
unsafe extern "system" fn control(
    code: u32,
    _: u32,
    _: *mut core::ffi::c_void,
    _: *mut core::ffi::c_void,
) -> u32 {
    if code == SERVICE_CONTROL_STOP || code == SERVICE_CONTROL_SHUTDOWN {
        status(SERVICE_STOP_PENDING, 0);
        if let Some(stop) = STOP.get() {
            stop.cancel();
        }
        if let Some(active) = ACTIVE.lock().unwrap().as_ref() {
            active.cancel();
        }
    }
    0
}
unsafe extern "system" fn main_service(_: u32, _: *mut *mut u16) {
    let sid = SID.get().unwrap();
    let name = helper::wide(&helper::service_name(sid));
    let handle = unsafe {
        RegisterServiceCtrlHandlerExW(name.as_ptr(), Some(control), std::ptr::null_mut())
    };
    if handle.is_null() {
        return;
    }
    *STATUS.lock().unwrap() = handle as usize;
    status(SERVICE_START_PENDING, 0);
    let result = (|| -> io::Result<()> {
        let stop = Cancel::new()?;
        let _ = STOP.set(stop.clone());
        status(SERVICE_RUNNING, 0);
        serve(sid, stop)
    })();
    status(
        SERVICE_STOPPED,
        result.err().and_then(|e| e.raw_os_error()).unwrap_or(0) as u32,
    );
}
pub fn dispatch(sid: String) -> io::Result<()> {
    let name = helper::wide(&helper::service_name(&sid));
    SID.set(sid)
        .map_err(|_| io::Error::other("service initialized twice"))?;
    let table = [
        SERVICE_TABLE_ENTRYW {
            lpServiceName: name.as_ptr() as _,
            lpServiceProc: Some(main_service),
        },
        SERVICE_TABLE_ENTRYW {
            lpServiceName: std::ptr::null_mut(),
            lpServiceProc: None,
        },
    ];
    if unsafe { StartServiceCtrlDispatcherW(table.as_ptr()) } == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}
pub fn serve(sid: &str, stop: Cancel) -> io::Result<()> {
    seek_engine::qos::utility();
    while !stop.cancelled() {
        let mut pipe = match helper::listen(sid, &helper::pipe_name(sid), stop.clone()) {
            Ok(p) => p,
            Err(_) if stop.cancelled() => break,
            Err(e) => return Err(e),
        };
        let cancel = Cancel::new()?;
        pipe.cancel = cancel.clone();
        *ACTIVE.lock().unwrap() = Some(cancel.clone());
        if stop.cancelled() {
            cancel.cancel();
        }
        let handle = pipe.handle.clone();
        if let Err(e) = crate::server::session(pipe, sid) {
            let mut reply = seek_engine::kernel::Pipe {
                handle,
                cancel: cancel.clone(),
            };
            let _ = helper::write(
                &mut reply,
                &helper::Response::Error {
                    message: e.to_string(),
                },
            );
        }
        cancel.cancel();
        ACTIVE.lock().unwrap().take();
    }
    Ok(())
}
