//! Shared executable resources also identify the native window and taskbar.
use windows_sys::Win32::UI::HiDpi::GetDpiForWindow;
use windows_sys::Win32::UI::WindowsAndMessaging::*;

// MAKEINTRESOURCE(1) is an integer resource identifier, never a dereferenced pointer.
#[allow(clippy::manual_dangling_ptr)]
pub fn apply(window: &gpui::Window) {
    let Some(hwnd) = crate::instance::hwnd(window) else {
        return;
    };
    unsafe {
        let module = GetWindowLongPtrW(hwnd, GWLP_HINSTANCE) as *mut core::ffi::c_void;
        // LR_SHARED keeps resource-owned handles valid for the process lifetime.
        // Windows chooses the closest frame and scales it for this window's DPI.
        let dpi = GetDpiForWindow(hwnd).max(96);
        for (kind, logical_size) in [(ICON_SMALL, 16u32), (ICON_BIG, 32u32)] {
            let size = (logical_size * dpi / 96) as i32;
            let icon = LoadImageW(
                module,
                1usize as *const u16,
                IMAGE_ICON,
                size,
                size,
                LR_SHARED,
            );
            if !icon.is_null() {
                SendMessageW(hwnd, WM_SETICON, kind as usize, icon as isize);
            }
        }
    }
}
