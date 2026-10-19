//! Blocking, cancellable ReadDirectoryChangesW; no polling or resident service.
use std::{
    io,
    sync::mpsc::Sender,
    thread::{self, JoinHandle},
};
use windows_sys::Win32::{
    Foundation::*,
    Storage::FileSystem::*,
    System::{IO::*, Threading::*},
};

#[derive(Debug)]
pub enum Change {
    Directories(Vec<Vec<u8>>),
    Overflow(Vec<u8>),
    Error(Vec<u8>, u32),
    Stop,
    Checkpoint(Sender<Result<(), String>>),
    Opened(Vec<u8>),
    HelperAvailable,
    /// All work queued before this reconciliation boundary has been applied.
    Reconciled(u64),
}

pub struct Watch {
    stop: usize,
    thread: Option<JoinHandle<()>>,
}
impl Watch {
    pub fn start(root: Vec<u8>, tx: Sender<Change>) -> io::Result<Self> {
        let wide = crate::paths::extended(&root);
        let directory = unsafe {
            CreateFileW(
                wide.as_ptr(),
                FILE_LIST_DIRECTORY,
                FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                std::ptr::null(),
                OPEN_EXISTING,
                FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OVERLAPPED,
                std::ptr::null_mut(),
            )
        };
        if directory == INVALID_HANDLE_VALUE {
            return Err(io::Error::last_os_error());
        }
        let stop = unsafe { CreateEventW(std::ptr::null(), 1, 0, std::ptr::null()) };
        let changed = unsafe { CreateEventW(std::ptr::null(), 1, 0, std::ptr::null()) };
        if stop.is_null() || changed.is_null() {
            unsafe {
                CloseHandle(directory);
                if !stop.is_null() {
                    CloseHandle(stop);
                }
                if !changed.is_null() {
                    CloseHandle(changed);
                }
            }
            return Err(io::Error::last_os_error());
        }
        let (dir_value, stop_value, changed_value) =
            (directory as usize, stop as usize, changed as usize);
        let (armed_tx, armed_rx) = std::sync::mpsc::sync_channel(1);
        let worker = thread::Builder::new()
            .name("seek-watcher".into())
            .spawn(move || {
                crate::qos::utility();
                let (directory, stop, changed) = (
                    dir_value as HANDLE,
                    stop_value as HANDLE,
                    changed_value as HANDLE,
                );
                let mut buffer = vec![0u32; 16_384]; // 64 KiB, DWORD aligned, valid on network shares.
                let mut armed = false;
                loop {
                    let mut overlapped: OVERLAPPED = unsafe { std::mem::zeroed() };
                    overlapped.hEvent = changed;
                    unsafe {
                        ResetEvent(changed);
                    }
                    let ok = unsafe {
                        ReadDirectoryChangesW(
                            directory,
                            buffer.as_mut_ptr() as _,
                            (buffer.len() * 4) as u32,
                            1,
                            FILE_NOTIFY_CHANGE_FILE_NAME
                                | FILE_NOTIFY_CHANGE_DIR_NAME
                                | FILE_NOTIFY_CHANGE_SIZE
                                | FILE_NOTIFY_CHANGE_LAST_WRITE
                                | FILE_NOTIFY_CHANGE_ATTRIBUTES
                                | FILE_NOTIFY_CHANGE_SECURITY,
                            std::ptr::null_mut(),
                            &mut overlapped,
                            None,
                        )
                    };
                    if ok == 0 && unsafe { GetLastError() } != ERROR_IO_PENDING {
                        let error = unsafe { GetLastError() };
                        if !armed {
                            let _ = armed_tx.send(Err(error));
                        }
                        let _ = tx.send(Change::Error(root.clone(), error));
                        break;
                    }
                    if !armed {
                        let _ = armed_tx.send(Ok(()));
                        armed = true;
                    }
                    let event =
                        unsafe { WaitForMultipleObjects(2, [stop, changed].as_ptr(), 0, INFINITE) };
                    if event != WAIT_OBJECT_0 + 1 {
                        unsafe {
                            CancelIoEx(directory, &overlapped);
                        }
                        let mut bytes = 0;
                        unsafe {
                            GetOverlappedResult(directory, &overlapped, &mut bytes, 1);
                        }
                        break;
                    }
                    let mut count = 0;
                    if unsafe { GetOverlappedResult(directory, &overlapped, &mut count, 0) } == 0
                        || count == 0
                    {
                        if tx.send(Change::Overflow(root.clone())).is_err() {
                            break;
                        }
                        continue;
                    }
                    let bytes = unsafe {
                        std::slice::from_raw_parts(buffer.as_ptr() as *const u8, count as usize)
                    };
                    let mut offset = 0;
                    let mut dirs = std::collections::BTreeSet::new();
                    while offset + 12 <= bytes.len() {
                        let next = u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap())
                            as usize;
                        let len =
                            u32::from_le_bytes(bytes[offset + 8..offset + 12].try_into().unwrap())
                                as usize;
                        if offset + 12 + len > bytes.len() || !len.is_multiple_of(2) {
                            dirs.insert(root.clone());
                            break;
                        }
                        let mut wide: Vec<u16> = bytes[offset + 12..offset + 12 + len]
                            .chunks_exact(2)
                            .map(|x| u16::from_le_bytes([x[0], x[1]]))
                            .collect();
                        for c in &mut wide {
                            if *c == 92 {
                                *c = 47;
                            }
                        }
                        let path = crate::live::join(&root, &crate::paths::encode(&wide));
                        let cut = path.iter().rposition(|&b| b == b'/').unwrap_or(0);
                        dirs.insert(path[..cut].to_vec());
                        if next == 0 {
                            break;
                        }
                        if next < 12 || offset + next >= bytes.len() {
                            dirs.insert(root.clone());
                            break;
                        }
                        offset += next;
                    }
                    if tx
                        .send(Change::Directories(dirs.into_iter().collect()))
                        .is_err()
                    {
                        break;
                    }
                }
                unsafe {
                    CloseHandle(directory);
                    CloseHandle(changed);
                }
            })?;
        let watch = Self {
            stop: stop_value,
            thread: Some(worker),
        };
        // The baseline may finish before a low-priority watcher thread is scheduled.
        // Do not publish it until the first Windows read is actually armed.
        match armed_rx.recv() {
            Ok(Ok(())) => Ok(watch),
            Ok(Err(error)) => Err(io::Error::from_raw_os_error(error as i32)),
            Err(_) => Err(io::Error::other(
                "Watcher stopped before registering its first read",
            )),
        }
    }
}
impl Drop for Watch {
    fn drop(&mut self) {
        unsafe {
            SetEvent(self.stop as HANDLE);
        }
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
        unsafe {
            CloseHandle(self.stop as HANDLE);
        }
    }
}
