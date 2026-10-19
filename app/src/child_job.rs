//! Lifetime and memory boundary for the short-lived PDF filter host.
//! Small direct kernel32 bindings avoid changing the pinned Windows backend.
use std::{
    ffi::c_void,
    io,
    os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle},
};
type Handle = *mut c_void;
#[repr(C)]
#[derive(Default)]
struct BasicLimits {
    process_time: i64,
    job_time: i64,
    flags: u32,
    min_working_set: usize,
    max_working_set: usize,
    active_processes: u32,
    affinity: usize,
    priority: u32,
    scheduling: u32,
}
#[repr(C)]
#[derive(Default)]
struct ExtendedLimits {
    basic: BasicLimits,
    io_counters: [u64; 6],
    process_memory: usize,
    job_memory: usize,
    peak_process_memory: usize,
    peak_job_memory: usize,
}
#[link(name = "kernel32")]
unsafe extern "system" {
    fn CreateJobObjectW(attributes: *const c_void, name: *const u16) -> Handle;
    fn OpenJobObjectW(access: u32, inherit: i32, name: *const u16) -> Handle;
    fn SetInformationJobObject(job: Handle, class: i32, info: *const c_void, len: u32) -> i32;
    fn AssignProcessToJobObject(job: Handle, process: Handle) -> i32;
    fn GetCurrentProcess() -> Handle;
}
fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(Some(0)).collect()
}
pub struct Job {
    _handle: OwnedHandle,
    pub name: String,
}
impl Job {
    pub fn new() -> io::Result<Self> {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let name = format!(
            "Local\\Seek-PDF-{}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        );
        let raw = unsafe { CreateJobObjectW(std::ptr::null(), wide(&name).as_ptr()) };
        if raw.is_null() {
            return Err(io::Error::last_os_error());
        }
        let handle = unsafe { OwnedHandle::from_raw_handle(raw) };
        let mut limits = ExtendedLimits::default();
        limits.basic.flags = 0x2000 | 0x100; // KILL_ON_JOB_CLOSE | PROCESS_MEMORY.
        limits.process_memory = 256 * 1024 * 1024;
        if unsafe {
            SetInformationJobObject(
                raw,
                9,
                (&limits as *const ExtendedLimits).cast(),
                std::mem::size_of_val(&limits) as u32,
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        Ok(Self {
            _handle: handle,
            name,
        })
    }
}
/// Attach before reading stdin or loading the filter. The host briefly opens
/// the parent's job, joins it, then closes its own handle. Thus only the parent
/// keeps the job alive, including if it exits between spawn and host startup.
pub fn attach_current(name: &str) -> io::Result<()> {
    let raw = unsafe { OpenJobObjectW(1, 0, wide(name).as_ptr()) };
    if raw.is_null() {
        return Err(io::Error::last_os_error());
    }
    let handle = unsafe { OwnedHandle::from_raw_handle(raw) };
    let result = unsafe { AssignProcessToJobObject(handle.as_raw_handle(), GetCurrentProcess()) };
    if result == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn closing_the_job_terminates_its_owned_child() {
        use std::{
            os::windows::process::CommandExt,
            process::{Command, Stdio},
            time::{Duration, Instant},
        };
        assert_eq!(std::mem::size_of::<ExtendedLimits>(), 144);
        if std::env::var_os("SEEK_STAGE_B_JOB_CHILD").is_some() {
            std::thread::sleep(Duration::from_secs(30));
            return;
        }
        let job = Job::new().unwrap();
        let mut child = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "child_job::tests::closing_the_job_terminates_its_owned_child",
            ])
            .env("SEEK_STAGE_B_JOB_CHILD", "1")
            .creation_flags(0x08000000)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .spawn()
            .unwrap();
        if unsafe { AssignProcessToJobObject(job._handle.as_raw_handle(), child.as_raw_handle()) }
            == 0
        {
            let _ = child.kill();
            let _ = child.wait();
            panic!("Cannot assign owned child: {}", io::Error::last_os_error());
        }
        drop(job);
        let until = Instant::now() + Duration::from_secs(2);
        while child.try_wait().unwrap().is_none() {
            if Instant::now() >= until {
                let _ = child.kill();
                let _ = child.wait();
                panic!("Owned child survived job close");
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    }
}
