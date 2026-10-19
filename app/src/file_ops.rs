//! Ordinary-user Shell operations. The owning STA blocks only this worker.
use std::{
    os::windows::ffi::{OsStrExt, OsStringExt},
    path::{Path, PathBuf},
};
use windows::{
    Win32::{Foundation::HWND, System::Com::*, UI::Shell::*},
    core::{PCWSTR, w},
};

#[derive(Clone, Debug)]
pub enum Operation {
    Clipboard {
        paths: Vec<PathBuf>,
        cut: bool,
    },
    Paste {
        folder: PathBuf,
    },
    Move {
        paths: Vec<PathBuf>,
        folder: PathBuf,
    },
    Rename {
        path: PathBuf,
        name: String,
    },
    NewFolder {
        folder: PathBuf,
        name: String,
    },
    Recycle {
        paths: Vec<PathBuf>,
    },
    DeletePermanently {
        paths: Vec<PathBuf>,
    },
    Dialog {
        path: PathBuf,
        properties: bool,
    },
}
pub struct Reply {
    pub result: Result<(), String>,
    pub refresh: Vec<PathBuf>,
}
#[derive(Clone)]
pub struct Worker {
    tx: Option<std::sync::mpsc::SyncSender<(Operation, usize)>>,
    thread: u32,
}
const FILE_OPERATION_MESSAGE: u32 = 0x8000 + 37;
impl Drop for Worker {
    fn drop(&mut self) {
        // Disconnect first, then wake the STA so its final receiver can exit.
        self.tx.take();
        unsafe {
            windows_sys::Win32::UI::WindowsAndMessaging::PostThreadMessageW(
                self.thread,
                FILE_OPERATION_MESSAGE,
                0,
                0,
            );
        }
    }
}
impl Worker {
    pub fn start() -> (Self, async_channel::Receiver<Reply>) {
        let (tx, rx) = std::sync::mpsc::sync_channel::<(Operation, usize)>(8);
        let (out, replies) = async_channel::unbounded();
        let (started, thread) = std::sync::mpsc::sync_channel(1);
        std::thread::Builder::new()
            .name("seek-file-operations".into())
            .spawn(move || {
                use windows_sys::Win32::UI::WindowsAndMessaging::*;
                let mut message = unsafe { std::mem::zeroed::<MSG>() };
                // Shell verbs can create modeless Windows dialogs on this STA.
                // A blocking message loop services those dialogs and the queue,
                // without polling or putting file operations on the UI thread.
                unsafe {
                    PeekMessageW(&mut message, std::ptr::null_mut(), 0, 0, PM_NOREMOVE);
                }
                let _ = started
                    .send(unsafe { windows_sys::Win32::System::Threading::GetCurrentThreadId() });
                unsafe {
                    if CoInitializeEx(None, COINIT_APARTMENTTHREADED).is_err() {
                        let _ = out.send_blocking(Reply {
                            result: Err("Cannot initialize Windows file operations".into()),
                            refresh: vec![],
                        });
                        return;
                    }
                }
                'messages: while unsafe { GetMessageW(&mut message, std::ptr::null_mut(), 0, 0) }
                    > 0
                {
                    if message.message == FILE_OPERATION_MESSAGE {
                        loop {
                            match rx.try_recv() {
                                Ok((op, owner)) => {
                                    let mut refresh = vec![];
                                    let result =
                                        execute(op, owner, &mut refresh).map_err(|e| e.to_string());
                                    if out.send_blocking(Reply { result, refresh }).is_err() {
                                        break 'messages;
                                    }
                                }
                                Err(std::sync::mpsc::TryRecvError::Empty) => break,
                                Err(std::sync::mpsc::TryRecvError::Disconnected) => break 'messages,
                            }
                        }
                    } else {
                        unsafe {
                            TranslateMessage(&message);
                            DispatchMessageW(&message);
                        }
                    }
                }
                unsafe {
                    CoUninitialize();
                }
            })
            .expect("file operation worker");
        (
            Self {
                tx: Some(tx),
                thread: thread.recv().expect("file operation STA startup"),
            },
            replies,
        )
    }
    pub fn send(&self, operation: Operation, owner: usize) -> Result<(), String> {
        self.tx
            .as_ref()
            .unwrap()
            .try_send((operation, owner))
            .map_err(|_| {
                "Another file operation is pending. Try again when it finishes.".to_owned()
            })?;
        if unsafe {
            windows_sys::Win32::UI::WindowsAndMessaging::PostThreadMessageW(
                self.thread,
                FILE_OPERATION_MESSAGE,
                0,
                0,
            )
        } == 0
        {
            return Err("Windows file operation worker is unavailable.".into());
        }
        Ok(())
    }
}
fn wide(path: &Path) -> Vec<u16> {
    path.as_os_str().encode_wide().chain(Some(0)).collect()
}
fn shell_path(path: &Path) -> PathBuf {
    let wide: Vec<u16> = path.as_os_str().encode_wide().collect();
    let prefix: Vec<u16> = "\\\\?\\".encode_utf16().collect();
    if let Some(rest) = wide.strip_prefix(prefix.as_slice()) {
        let unc: Vec<u16> = "UNC\\".encode_utf16().collect();
        if let Some(rest) = rest.strip_prefix(unc.as_slice()) {
            return std::ffi::OsString::from_wide(&[&[92u16, 92][..], rest].concat()).into();
        }
        if rest.get(1) == Some(&58) {
            return std::ffi::OsString::from_wide(rest).into();
        }
    }
    path.into()
}
fn item(path: &Path) -> windows::core::Result<IShellItem> {
    unsafe {
        SHCreateItemFromParsingName(PCWSTR(wide(&shell_path(path)).as_ptr()), None::<&IBindCtx>)
    }
}
pub fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name != "."
        && name != ".."
        && !name.ends_with([' ', '.'])
        && !name.chars().any(|c| c < ' ' || "<>:\"/\\|?*".contains(c))
        && ![
            "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7",
            "COM8", "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
        ]
        .contains(
            &name
                .split('.')
                .next()
                .unwrap_or("")
                .to_ascii_uppercase()
                .as_str(),
        )
}
fn invalid(message: &str) -> windows::core::Error {
    windows::core::Error::new(windows::Win32::Foundation::E_INVALIDARG, message)
}
fn execute(op: Operation, owner: usize, refresh: &mut Vec<PathBuf>) -> windows::core::Result<()> {
    match op {
        Operation::Clipboard { paths, cut } => clipboard_write(&paths, cut, owner),
        Operation::Dialog { path, properties } => {
            unsafe {
                let mut info = SHELLEXECUTEINFOW {
                    cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as _,
                    fMask: SEE_MASK_NOASYNC
                        | if properties {
                            SEE_MASK_INVOKEIDLIST
                        } else {
                            SEE_MASK_CLASSNAME
                        },
                    hwnd: HWND(owner as _),
                    lpVerb: if properties {
                        w!("properties")
                    } else {
                        w!("openas")
                    },
                    // Resolve the system picker verb, not an association's
                    // application-specific implementation of "openas".
                    lpClass: if properties {
                        PCWSTR::null()
                    } else {
                        w!("Unknown")
                    },
                    nShow: 1,
                    ..Default::default()
                };
                // Keep the UTF-16 allocation alive through the Shell call.
                let path = wide(&shell_path(&path));
                info.lpFile = PCWSTR(path.as_ptr());
                ShellExecuteExW(&mut info)?;
            }
            Ok(())
        }
        op => unsafe {
            let files: IFileOperation =
                CoCreateInstance(&FileOperation, None, CLSCTX_INPROC_SERVER)?;
            files.SetOwnerWindow(HWND(owner as _))?;
            // Permanent deletion is reached only after the separate GPUI confirmation.
            let flags = if matches!(&op, Operation::DeletePermanently { .. }) {
                FOF_NOCONFIRMATION | FOF_NOERRORUI | FOFX_EARLYFAILURE
            } else {
                FOFX_ADDUNDORECORD
                    | FOFX_RECYCLEONDELETE
                    | FOF_WANTNUKEWARNING
                    | FOF_NOCONFIRMMKDIR
                    | FOF_NOERRORUI
                    | FOFX_EARLYFAILURE
                    | if matches!(&op, Operation::Recycle { .. }) {
                        FOF_NOCONFIRMATION
                    } else {
                        FILEOPERATION_FLAGS(0)
                    }
            };
            files.SetOperationFlags(flags)?;
            let op = match op {
                Operation::Paste { folder } => {
                    let (paths, cut) = clipboard_read()?;
                    if cut {
                        Operation::Move { paths, folder }
                    } else {
                        copy(&files, &paths, &folder, refresh)?;
                        files.PerformOperations()?;
                        return finish(&files);
                    }
                }
                other => other,
            };
            match op {
                Operation::Move { paths, folder } => {
                    let dest = item(&folder)?;
                    refresh.push(folder.clone());
                    for path in paths {
                        if folder == path || folder.starts_with(&path) {
                            return Err(invalid("A folder cannot be moved into itself."));
                        }
                        if let Some(p) = path.parent() {
                            refresh.push(p.into());
                        }
                        let src = item(&path)?;
                        files.MoveItem(
                            &src,
                            &dest,
                            PCWSTR::null(),
                            None::<&IFileOperationProgressSink>,
                        )?;
                    }
                }
                Operation::Rename { path, name } => {
                    if !valid_name(&name) {
                        return Err(invalid("Choose a valid Windows file name."));
                    }
                    if let Some(p) = path.parent() {
                        refresh.push(p.into());
                    }
                    let src = item(&path)?;
                    let name = wide(Path::new(&name));
                    files.RenameItem(
                        &src,
                        PCWSTR(name.as_ptr()),
                        None::<&IFileOperationProgressSink>,
                    )?;
                }
                Operation::NewFolder { folder, name } => {
                    if !valid_name(&name) {
                        return Err(invalid("Choose a valid Windows folder name."));
                    }
                    let dest = item(&folder)?;
                    refresh.push(folder);
                    let name = wide(Path::new(&name));
                    files.NewItem(
                        &dest,
                        0x10,
                        PCWSTR(name.as_ptr()),
                        PCWSTR::null(),
                        None::<&IFileOperationProgressSink>,
                    )?;
                }
                Operation::Recycle { paths } => {
                    let guard: IFileOperationProgressSink =
                        crate::recycle_guard::RecycleGuard.into();
                    for path in paths {
                        recycle_allowed(&path)?;
                        if let Some(p) = path.parent() {
                            refresh.push(p.into());
                        }
                        let src = item(&path)?;
                        files.DeleteItem(&src, &guard)?;
                    }
                }
                Operation::DeletePermanently { paths } => {
                    for path in paths {
                        if let Some(parent) = path.parent() {
                            refresh.push(parent.into());
                        }
                        files.DeleteItem(&item(&path)?, None::<&IFileOperationProgressSink>)?;
                    }
                }
                _ => unreachable!(),
            }
            files.PerformOperations()?;
            finish(&files)
        },
    }
}
unsafe fn copy(
    files: &IFileOperation,
    paths: &[PathBuf],
    folder: &Path,
    refresh: &mut Vec<PathBuf>,
) -> windows::core::Result<()> {
    let dest = item(folder)?;
    refresh.push(folder.into());
    for path in paths {
        if folder == path || folder.starts_with(path) {
            return Err(invalid("Choose a destination outside the source folder."));
        }
        let src = item(path)?;
        unsafe {
            files.CopyItem(
                &src,
                &dest,
                PCWSTR::null(),
                None::<&IFileOperationProgressSink>,
            )?;
        }
    }
    Ok(())
}
unsafe fn finish(files: &IFileOperation) -> windows::core::Result<()> {
    if unsafe { files.GetAnyOperationsAborted()? }.as_bool() {
        Err(windows::core::Error::new(
            windows::Win32::Foundation::E_ABORT,
            "File operation cancelled or skipped. Some items may be unchanged.",
        ))
    } else {
        Ok(())
    }
}
fn recycle_allowed(path: &Path) -> windows::core::Result<()> {
    // Recycle is deliberately unavailable on UNC/removable destinations. The
    // Windows 8+ recycle-only flag above never requests permanent deletion.
    let path = std::fs::canonicalize(path).map_err(|e| invalid(&e.to_string()))?;
    let text = path.to_string_lossy();
    if text.starts_with("\\\\?\\UNC\\") || text.starts_with("\\\\") && !text.starts_with("\\\\?\\")
    {
        return Err(invalid(
            "This location cannot be safely recycled in Seek. Use Explorer.",
        ));
    }
    let native = text.strip_prefix("\\\\?\\").unwrap_or(&text);
    if native.len() < 3 || native.as_bytes()[1] != b':' {
        return Err(invalid("No local Recycle Bin is available."));
    }
    let root = wide(Path::new(&native[..3]));
    if unsafe { windows_sys::Win32::Storage::FileSystem::GetDriveTypeW(root.as_ptr()) } != 3 {
        return Err(invalid("Seek only recycles files on local fixed drives."));
    }
    Ok(())
}
fn clipboard_write(paths: &[PathBuf], cut: bool, owner: usize) -> windows::core::Result<()> {
    use windows::Win32::{
        Foundation::HANDLE,
        System::{DataExchange::*, Memory::*},
    };
    if paths.is_empty() {
        return Err(invalid("Select a file or folder first."));
    }
    let mut data = vec![0u8; 20];
    data[..4].copy_from_slice(&20u32.to_ne_bytes());
    data[16..20].copy_from_slice(&1u32.to_ne_bytes());
    for path in paths {
        for c in wide(path) {
            data.extend_from_slice(&c.to_ne_bytes());
        }
    }
    data.extend_from_slice(&[0, 0]);
    unsafe {
        OpenClipboard(Some(HWND(owner as _)))?;
        let result = (|| {
            EmptyClipboard()?;
            for (format, bytes) in [
                (15, data),
                (
                    RegisterClipboardFormatW(w!("Preferred DropEffect")),
                    if cut { 2u32 } else { 1u32 }.to_ne_bytes().to_vec(),
                ),
            ] {
                let h = GlobalAlloc(GMEM_MOVEABLE, bytes.len())?;
                let p = GlobalLock(h);
                if p.is_null() {
                    let _ = windows::Win32::Foundation::GlobalFree(Some(h));
                    return Err(windows::core::Error::from_thread());
                }
                std::ptr::copy_nonoverlapping(bytes.as_ptr(), p.cast(), bytes.len());
                let _ = GlobalUnlock(h);
                if let Err(e) = SetClipboardData(format, Some(HANDLE(h.0))) {
                    let _ = windows::Win32::Foundation::GlobalFree(Some(h));
                    return Err(e);
                }
            }
            Ok(())
        })();
        let _ = CloseClipboard();
        result
    }
}
fn clipboard_read() -> windows::core::Result<(Vec<PathBuf>, bool)> {
    use windows::Win32::System::{DataExchange::*, Memory::*};
    unsafe {
        OpenClipboard(None)?;
        let result = (|| {
            let h = GetClipboardData(15)?;
            let drop = HDROP(h.0);
            let count = DragQueryFileW(drop, u32::MAX, None);
            if count == 0 || count > 100000 {
                return Err(invalid(
                    "The clipboard does not contain a supported file list.",
                ));
            }
            let mut paths = Vec::with_capacity(count as usize);
            for i in 0..count {
                let len = DragQueryFileW(drop, i, None);
                let mut name = vec![0u16; len as usize + 1];
                DragQueryFileW(drop, i, Some(&mut name));
                paths.push(PathBuf::from(std::ffi::OsString::from_wide(
                    &name[..len as usize],
                )));
            }
            let mut cut = false;
            if let Ok(h) = GetClipboardData(RegisterClipboardFormatW(w!("Preferred DropEffect"))) {
                let h = windows::Win32::Foundation::HGLOBAL(h.0);
                if GlobalSize(h) >= 4 {
                    let p = GlobalLock(h);
                    if !p.is_null() {
                        cut = std::ptr::read_unaligned(p.cast::<u32>()) == 2;
                        let _ = GlobalUnlock(h);
                    }
                }
            }
            Ok((paths, cut))
        })();
        let _ = CloseClipboard();
        result
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn names_cannot_escape_or_use_windows_devices() {
        for n in [
            "",
            "..",
            "../other",
            "A/B",
            "C:\\secret",
            "CON",
            "nul.txt",
            "name.",
            "name ",
            "a:b",
        ] {
            assert!(!valid_name(n), "{n}");
        }
        assert!(valid_name("Résumé 🐈.md"));
    }
}
#[cfg(test)]
mod operation_tests {
    use super::*;
    fn run(worker: &Worker, rx: &async_channel::Receiver<Reply>, op: Operation) {
        worker.send(op, 0).unwrap();
        let until = std::time::Instant::now() + std::time::Duration::from_secs(30);
        loop {
            match rx.try_recv() {
                Ok(reply) => {
                    reply.result.unwrap();
                    return;
                }
                Err(async_channel::TryRecvError::Empty) => {
                    assert!(std::time::Instant::now() < until, "Operation timed out");
                    std::thread::sleep(std::time::Duration::from_millis(10));
                }
                Err(e) => panic!("{e}"),
            }
        }
    }
    #[test]
    fn shell_worker_creates_renames_moves_and_recycles_only_owned_files() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        let a = root.join("Résumé 🐈.txt");
        std::fs::write(&a, "Original data").unwrap();
        let (worker, rx) = Worker::start();
        run(
            &worker,
            &rx,
            Operation::NewFolder {
                folder: std::fs::canonicalize(root).unwrap(),
                name: "Destination".into(),
            },
        );
        let dest = root.join("Destination");
        assert!(dest.is_dir());
        run(
            &worker,
            &rx,
            Operation::Rename {
                path: a,
                name: "Renamed.txt".into(),
            },
        );
        let a = root.join("Renamed.txt");
        assert_eq!(std::fs::read_to_string(&a).unwrap(), "Original data");
        run(
            &worker,
            &rx,
            Operation::Move {
                paths: vec![a.clone()],
                folder: dest.clone(),
            },
        );
        let b = dest.join("Renamed.txt");
        assert!(!a.exists());
        assert_eq!(std::fs::read_to_string(&b).unwrap(), "Original data");
        run(
            &worker,
            &rx,
            Operation::Recycle {
                paths: vec![b.clone()],
            },
        );
        assert!(!b.exists());
    }
}
