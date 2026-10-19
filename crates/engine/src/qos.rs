pub fn utility() {
    unsafe {
        windows_sys::Win32::System::Threading::SetThreadPriority(
            windows_sys::Win32::System::Threading::GetCurrentThread(),
            windows_sys::Win32::System::Threading::THREAD_PRIORITY_BELOW_NORMAL,
        );
    }
}
pub fn user_interactive() {}
pub fn user_initiated() {}
