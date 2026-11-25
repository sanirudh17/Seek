use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use std::{
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};
use windows_sys::Win32::{Foundation::*, System::Threading::*, UI::WindowsAndMessaging::*};
struct Signal {
    handle: usize,
    stop: AtomicBool,
}
impl Drop for Signal {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.handle as HANDLE);
        }
    }
}
pub struct Instance {
    mutex: HANDLE,
    signal: Arc<Signal>,
    pub focus: async_channel::Receiver<()>,
}
/// Where a second launch leaves a folder for the running Seek to open.
pub fn request_file(data: &Path) -> std::path::PathBuf {
    data.join("open-request.txt")
}

/// `Seek-{user SID}-{hash of the data folder}`: names this user's Seek for
/// one data folder (its single-instance lock, its agent pipe).
pub fn key(data: &Path) -> Result<String, String> {
    let sid = seek_engine::helper::user_sid().map_err(|e| e.to_string())?;
    let data = std::path::absolute(data).map_err(|e| e.to_string())?;
    use std::os::windows::ffi::OsStrExt;
    let mut normalized: Vec<_> = seek_engine::paths::native(&seek_engine::paths::internal(&data))
        .as_os_str()
        .encode_wide()
        .collect();
    unsafe {
        CharUpperBuffW(normalized.as_mut_ptr(), normalized.len() as u32);
    }
    let hash = seek_engine::paths::encode(&normalized)
        .iter()
        .fold(0xcbf29ce484222325u64, |h, b| {
            (h ^ (*b as u64)).wrapping_mul(0x100000001b3)
        });
    Ok(format!("Seek-{sid}-{hash:016x}"))
}

/// This Seek's "closing" event, raised when its last window closes.
static CLOSING: std::sync::OnceLock<usize> = std::sync::OnceLock::new();

/// Tells a Seek started now that this one is on its way out (it still has its
/// index to save), so the new one waits and opens its own window instead of
/// asking this one to come forward.
pub fn mark_closing() {
    if let Some(event) = CLOSING.get() {
        unsafe {
            SetEvent(*event as HANDLE);
        }
    }
}

pub fn acquire(data: &Path, request: Option<&Path>) -> Result<Option<Instance>, String> {
    let sid = seek_engine::helper::user_sid().map_err(|e| e.to_string())?;
    let data = std::path::absolute(data).map_err(|e| e.to_string())?;
    use std::os::windows::ffi::OsStrExt;
    let mut normalized: Vec<_> = seek_engine::paths::native(&seek_engine::paths::internal(&data))
        .as_os_str()
        .encode_wide()
        .collect();
    unsafe {
        CharUpperBuffW(normalized.as_mut_ptr(), normalized.len() as u32);
    }
    let hash = seek_engine::paths::encode(&normalized)
        .iter()
        .fold(0xcbf29ce484222325u64, |h, b| {
            (h ^ (*b as u64)).wrapping_mul(0x100000001b3)
        });
    let base = format!("Local\\Seek-{sid}-{hash:016x}");
    let wide = |s: &str| s.encode_utf16().chain(Some(0)).collect::<Vec<_>>();
    // Manual-reset: once the running Seek starts closing, it stays raised.
    let closing = unsafe { CreateEventW(std::ptr::null(), 1, 0, wide(&format!("{base}-closing")).as_ptr()) };
    let mut mutex = unsafe { CreateMutexW(std::ptr::null(), 0, wide(&base).as_ptr()) };
    if mutex.is_null() {
        return Err(std::io::Error::last_os_error().to_string());
    }
    let mut exists = unsafe { GetLastError() } == ERROR_ALREADY_EXISTS;
    // The running Seek is closing (saving its index): wait for it to finish,
    // then become the running Seek, rather than waking one that is leaving
    // (which used to make a quick relaunch from the taskbar do nothing).
    if exists && !closing.is_null() && unsafe { WaitForSingleObject(closing, 0) } == WAIT_OBJECT_0 {
        let started = std::time::Instant::now();
        while exists && started.elapsed() < std::time::Duration::from_secs(20) {
            unsafe {
                CloseHandle(mutex);
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
            mutex = unsafe { CreateMutexW(std::ptr::null(), 0, wide(&base).as_ptr()) };
            if mutex.is_null() {
                return Err(std::io::Error::last_os_error().to_string());
            }
            exists = unsafe { GetLastError() } == ERROR_ALREADY_EXISTS;
        }
        if !exists {
            unsafe {
                ResetEvent(closing);
            }
        }
    }
    if !closing.is_null() && !exists {
        let _ = CLOSING.set(closing as usize);
    } else if !closing.is_null() {
        unsafe {
            CloseHandle(closing);
        }
    }
    let event = unsafe {
        CreateEventW(
            std::ptr::null(),
            0,
            0,
            wide(&format!("{base}-focus")).as_ptr(),
        )
    };
    if event.is_null() {
        unsafe {
            CloseHandle(mutex);
        }
        return Err(std::io::Error::last_os_error().to_string());
    }
    if exists {
        if let Some(folder) = request {
            // Hand the folder over before waking the running Seek.
            let _ = std::fs::write(request_file(&data), folder.to_string_lossy().as_bytes());
        }
        unsafe {
            AllowSetForegroundWindow(ASFW_ANY);
            SetEvent(event);
            CloseHandle(event);
            CloseHandle(mutex);
        }
        return Ok(None);
    }
    let signal = Arc::new(Signal {
        handle: event as usize,
        stop: AtomicBool::new(false),
    });
    let worker = signal.clone();
    let (tx, rx) = async_channel::bounded(1);
    std::thread::Builder::new()
        .name("seek-instance".into())
        .spawn(move || {
            loop {
                if unsafe { WaitForSingleObject(worker.handle as HANDLE, INFINITE) }
                    != WAIT_OBJECT_0
                    || worker.stop.load(Ordering::Acquire)
                {
                    break;
                }
                if tx.force_send(()).is_err() {
                    break;
                }
            }
        })
        .map_err(|e| e.to_string())?;
    Ok(Some(Instance {
        mutex,
        signal,
        focus: rx,
    }))
}
impl Drop for Instance {
    fn drop(&mut self) {
        self.signal.stop.store(true, Ordering::Release);
        unsafe {
            SetEvent(self.signal.handle as HANDLE);
            CloseHandle(self.mutex);
        }
    }
}
pub fn hwnd(window: &gpui::Window) -> Option<HWND> {
    match HasWindowHandle::window_handle(window).ok()?.as_raw() {
        RawWindowHandle::Win32(h) => Some(h.hwnd.get() as HWND),
        _ => None,
    }
}
pub fn restore(window: &mut gpui::Window) {
    // The backend restores minimized windows and defers startup activation
    // until the first painted frame. A direct ShowWindow here bypassed that.
    window.activate_window();
}
pub fn round(window: &gpui::Window) {
    if let Some(h) = hwnd(window) {
        let round = 2u32;
        unsafe {
            windows_sys::Win32::Graphics::Dwm::DwmSetWindowAttribute(
                h,
                33, // DWMWA_WINDOW_CORNER_PREFERENCE: round
                (&round as *const u32).cast(),
                4,
            );
        }
    }
}
#[allow(dead_code)]
pub fn caption_theme(window: &gpui::Window, dark: bool) {
    if let Some(h) = hwnd(window) {
        let value = i32::from(dark);
        unsafe {
            windows_sys::Win32::Graphics::Dwm::DwmSetWindowAttribute(
                h,
                20,
                (&value as *const i32).cast(),
                4,
            );
        }
    }
}
pub fn apply_backdrop(window: &gpui::Window, _glass: bool, dark: bool) {
    // Material selection belongs to the GPUI backend only.
    caption_theme(window, dark);
    round(window);
}

pub fn motion_enabled() -> bool {
    let mut enabled = 1i32;
    unsafe {
        SystemParametersInfoW(
            SPI_GETCLIENTAREAANIMATION,
            0,
            (&mut enabled as *mut i32).cast(),
            0,
        );
    }
    enabled != 0
}
