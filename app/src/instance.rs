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
pub fn acquire(data: &Path) -> Result<Option<Instance>, String> {
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
    let mutex = unsafe { CreateMutexW(std::ptr::null(), 0, wide(&base).as_ptr()) };
    if mutex.is_null() {
        return Err(std::io::Error::last_os_error().to_string());
    }
    let exists = unsafe { GetLastError() } == ERROR_ALREADY_EXISTS;
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
    if let Some(h) = hwnd(window) {
        unsafe {
            ShowWindow(h, SW_RESTORE);
            SetForegroundWindow(h);
        }
    }
    window.activate_window();
}
pub fn round(window: &gpui::Window) {
    if let Some(h) = hwnd(window) {
        let round = 2u32;
        unsafe {
            windows_sys::Win32::Graphics::Dwm::DwmSetWindowAttribute(
                h,
                33,
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
    // Leave native backdrop composition to GPUI, as Zeron does. Extending a
    // second DWM frame underneath it can paint native caption glyphs twice.
    caption_theme(window, dark);
    round(window);
}
