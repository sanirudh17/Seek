//! Shared cancellable Win32 I/O. Every wait blocks on a kernel event.
use std::{io, sync::Arc};
use windows_sys::Win32::{
    Foundation::*,
    Storage::FileSystem::*,
    System::{IO::*, Threading::*},
};

pub struct Handle(HANDLE);
unsafe impl Send for Handle {}
unsafe impl Sync for Handle {}
impl Handle {
    /// Takes exclusive ownership of a valid handle returned by Win32.
    /// # Safety
    /// `handle` must be valid and must not have another owner that closes it.
    pub unsafe fn owned(handle: HANDLE) -> io::Result<Self> {
        if handle.is_null() || handle == INVALID_HANDLE_VALUE {
            Err(io::Error::last_os_error())
        } else {
            Ok(Self(handle))
        }
    }
    pub fn raw(&self) -> HANDLE {
        self.0
    }
}
impl Drop for Handle {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.0);
        }
    }
}
#[derive(Clone)]
pub struct Cancel(Arc<Handle>);
impl Cancel {
    pub fn new() -> io::Result<Self> {
        Ok(Self(Arc::new(unsafe {
            Handle::owned(CreateEventW(std::ptr::null(), 1, 0, std::ptr::null()))?
        })))
    }
    pub fn raw(&self) -> HANDLE {
        self.0.raw()
    }
    pub fn cancel(&self) {
        unsafe {
            SetEvent(self.raw());
        }
    }
    pub fn cancelled(&self) -> bool {
        unsafe { WaitForSingleObject(self.raw(), 0) == WAIT_OBJECT_0 }
    }
}
fn event() -> io::Result<Handle> {
    unsafe { Handle::owned(CreateEventW(std::ptr::null(), 1, 0, std::ptr::null())) }
}
pub fn complete(
    handle: &Handle,
    ov: &mut OVERLAPPED,
    cancel: &Cancel,
    timeout: u32,
) -> io::Result<u32> {
    let wait = unsafe { WaitForMultipleObjects(2, [cancel.raw(), ov.hEvent].as_ptr(), 0, timeout) };
    let mut count = 0;
    if wait != WAIT_OBJECT_0 + 1 {
        unsafe {
            CancelIoEx(handle.raw(), &*ov);
            GetOverlappedResult(handle.raw(), ov, &mut count, 1);
        }
        return Err(if wait == WAIT_TIMEOUT {
            io::Error::new(io::ErrorKind::TimedOut, "Windows I/O timeout")
        } else {
            io::Error::new(io::ErrorKind::ConnectionAborted, "Windows I/O cancelled")
        });
    }
    if unsafe { GetOverlappedResult(handle.raw(), ov, &mut count, 0) } == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(count)
    }
}
pub fn control<T>(
    handle: &Handle,
    code: u32,
    input: Option<&T>,
    output: &mut [u8],
    cancel: &Cancel,
) -> io::Result<usize> {
    if cancel.cancelled() {
        return Err(io::Error::new(io::ErrorKind::Interrupted, "cancelled"));
    }
    let event = event()?;
    let mut ov: OVERLAPPED = unsafe { std::mem::zeroed() };
    ov.hEvent = event.raw();
    let mut count = 0;
    let (ptr, len) = input.map_or((std::ptr::null(), 0), |v| {
        (v as *const T as *const _, std::mem::size_of::<T>() as u32)
    });
    let ok = unsafe {
        DeviceIoControl(
            handle.raw(),
            code,
            ptr,
            len,
            output.as_mut_ptr() as _,
            output.len() as u32,
            &mut count,
            &mut ov,
        )
    };
    if ok != 0 {
        return Ok(count as usize);
    }
    let e = io::Error::last_os_error();
    if e.raw_os_error() != Some(ERROR_IO_PENDING as i32) {
        return Err(e);
    }
    Ok(complete(handle, &mut ov, cancel, INFINITE)? as usize)
}
pub struct Pipe {
    pub handle: Arc<Handle>,
    pub cancel: Cancel,
}
pub fn read_at(
    handle: &Handle,
    offset: u64,
    bytes: &mut [u8],
    cancel: &Cancel,
) -> io::Result<usize> {
    if cancel.cancelled() {
        return Err(io::Error::new(
            io::ErrorKind::ConnectionAborted,
            "read cancelled",
        ));
    }
    let event = event()?;
    let mut ov: OVERLAPPED = unsafe { std::mem::zeroed() };
    ov.hEvent = event.raw();
    ov.Anonymous.Anonymous.Offset = offset as u32;
    ov.Anonymous.Anonymous.OffsetHigh = (offset >> 32) as u32;
    let mut count = 0;
    let ok = unsafe {
        ReadFile(
            handle.raw(),
            bytes.as_mut_ptr(),
            u32::try_from(bytes.len()).map_err(io::Error::other)?,
            &mut count,
            &mut ov,
        )
    };
    if ok != 0 {
        return Ok(count as usize);
    }
    let e = io::Error::last_os_error();
    if e.raw_os_error() == Some(ERROR_IO_PENDING as i32) {
        complete(handle, &mut ov, cancel, INFINITE).map(|n| n as usize)
    } else {
        Err(e)
    }
}
impl io::Read for Pipe {
    fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
        if bytes.is_empty() {
            return Ok(0);
        }
        if self.cancel.cancelled() {
            return Err(io::Error::new(
                io::ErrorKind::ConnectionAborted,
                "pipe cancelled",
            ));
        }
        let event = event()?;
        let mut ov: OVERLAPPED = unsafe { std::mem::zeroed() };
        ov.hEvent = event.raw();
        let mut count = 0;
        let ok = unsafe {
            ReadFile(
                self.handle.raw(),
                bytes.as_mut_ptr(),
                bytes.len().min(u32::MAX as usize) as u32,
                &mut count,
                &mut ov,
            )
        };
        if ok != 0 {
            return Ok(count as usize);
        }
        let e = io::Error::last_os_error();
        match e.raw_os_error() {
            Some(x) if x == ERROR_BROKEN_PIPE as i32 => Ok(0),
            Some(x) if x == ERROR_IO_PENDING as i32 => {
                complete(&self.handle, &mut ov, &self.cancel, INFINITE).map(|v| v as usize)
            }
            _ => Err(e),
        }
    }
}
impl io::Write for Pipe {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if bytes.is_empty() {
            return Ok(0);
        }
        if self.cancel.cancelled() {
            return Err(io::Error::new(
                io::ErrorKind::ConnectionAborted,
                "pipe cancelled",
            ));
        }
        let event = event()?;
        let mut ov: OVERLAPPED = unsafe { std::mem::zeroed() };
        ov.hEvent = event.raw();
        let mut count = 0;
        let ok = unsafe {
            WriteFile(
                self.handle.raw(),
                bytes.as_ptr(),
                bytes.len().min(u32::MAX as usize) as u32,
                &mut count,
                &mut ov,
            )
        };
        if ok != 0 {
            return Ok(count as usize);
        }
        let e = io::Error::last_os_error();
        if e.raw_os_error() == Some(ERROR_IO_PENDING as i32) {
            complete(&self.handle, &mut ov, &self.cancel, INFINITE).map(|v| v as usize)
        } else {
            Err(e)
        }
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
