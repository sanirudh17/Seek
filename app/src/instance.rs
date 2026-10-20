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
pub fn apply_backdrop(window: &gpui::Window, glass: bool, dark: bool) {
    caption_theme(window, dark);
    round(window);
    if let Some(hwnd) = hwnd(window) {
        system_acrylic(hwnd, glass);
    }
}

// Windows 11's system material remains compositor-owned across maximize,
// restore and activation. The older accent policy can disappear while DWM
// animates the window. Unsupported systems retain GPUI's existing fallback.
// No second extended client frame or native caption is introduced.
fn system_acrylic(hwnd: HWND, glass: bool) -> bool {
    let material = if glass { 3u32 } else { 1u32 }; // DWMSBT_TRANSIENTWINDOW / NONE
    let result = unsafe {
        windows_sys::Win32::Graphics::Dwm::DwmSetWindowAttribute(
            hwnd,
            38,
            (&material as *const u32).cast(),
            4,
        )
    };
    if result < 0 {
        return false;
    }
    if !glass {
        return true;
    }
    #[repr(C)]
    struct Policy {
        state: u32,
        flags: u32,
        color: u32,
        animation: u32,
    }
    #[repr(C)]
    struct Attribute {
        attribute: u32,
        data: *mut std::ffi::c_void,
        size: usize,
    }
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetModuleHandleW(name: *const u16) -> *mut std::ffi::c_void;
        fn GetProcAddress(module: *mut std::ffi::c_void, name: *const u8) -> *mut std::ffi::c_void;
    }
    let module = unsafe { GetModuleHandleW(windows::core::w!("user32.dll").as_ptr()) };
    let address =
        unsafe { GetProcAddress(module, c"SetWindowCompositionAttribute".as_ptr().cast()) };
    if address.is_null() {
        let none = 1u32;
        unsafe {
            windows_sys::Win32::Graphics::Dwm::DwmSetWindowAttribute(
                hwnd,
                38,
                (&none as *const u32).cast(),
                4,
            );
        }
        return false;
    }
    let set: unsafe extern "system" fn(HWND, *mut Attribute) -> i32 =
        unsafe { std::mem::transmute(address) };
    let mut policy = Policy {
        state: 0,
        flags: 2,
        color: 0,
        animation: 0,
    };
    let mut data = Attribute {
        attribute: 19,
        data: (&mut policy as *mut Policy).cast(),
        size: std::mem::size_of::<Policy>(),
    };
    let applied = unsafe { set(hwnd, &mut data) != 0 };
    if !applied {
        let none = 1u32;
        unsafe {
            windows_sys::Win32::Graphics::Dwm::DwmSetWindowAttribute(
                hwnd,
                38,
                (&none as *const u32).cast(),
                4,
            );
        }
    }
    applied
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

#[cfg(test)]
mod material_tests {
    use super::*;
    #[test]
    fn compositor_material_can_be_applied_and_removed_on_an_owned_window() {
        use windows_sys::Win32::Graphics::Dwm::DwmGetWindowAttribute;
        unsafe {
            let window = CreateWindowExW(
                0,
                windows::core::w!("STATIC").as_ptr(),
                windows::core::w!("Seek material test").as_ptr(),
                WS_OVERLAPPEDWINDOW,
                0,
                0,
                320,
                240,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null(),
            );
            assert!(!window.is_null());
            let applied = system_acrylic(window, true);
            if applied {
                let mut material = 0u32;
                let result =
                    DwmGetWindowAttribute(window, 38, (&mut material as *mut u32).cast(), 4);
                assert!(result >= 0);
                assert_eq!(material, 3);
                assert!(system_acrylic(window, false));
                assert!(
                    DwmGetWindowAttribute(window, 38, (&mut material as *mut u32).cast(), 4) >= 0
                );
                assert_eq!(material, 1);
                println!(
                    "DWM system acrylic: set/read/remove passed on a hidden owned window; transitions not visually tested"
                );
            } else {
                println!("DWM system acrylic unavailable; existing legacy fallback retained");
            }
            assert_ne!(DestroyWindow(window), 0);
        }
    }
}
