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
    BatchRename {
        entries: Vec<(PathBuf, String)>,
    },
    UndoBatchRename {
        redo: bool,
    },
    UndoCopy {
        redo: bool,
    },
    RedoRecycle,
    Copy {
        paths: Vec<PathBuf>,
        folder: PathBuf,
    },
    ArchiveHistory,
    PutBack {
        job: u64,
    },
    UndoMove {
        redo: bool,
    },
    UndoRename {
        redo: bool,
    },
    History,
    ReviewHistory,
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
    GroupSelection {
        paths: Vec<PathBuf>,
        folder: PathBuf,
        name: String,
    },
    /// Move reviewed files into subfolders of `folder`; see crate::organize.
    Organize {
        folder: PathBuf,
        groups: Vec<(String, Vec<crate::organize::Item>)>,
    },
    NewFolder {
        folder: PathBuf,
        name: String,
    },
    /// An empty file; an existing name is never replaced.
    NewFile {
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
pub const READ_ONLY_ARCHIVE: &str =
    "Items inside a ZIP file are read-only. Extract the ZIP file to change them.";

/// Whether an operation would change or hand out a path inside a ZIP file.
/// Archive contents are browsed read-only; nothing writes into them.
pub fn touches_archive(operation: &Operation) -> bool {
    let inside = |path: &std::path::Path| crate::archive::inside(path);
    match operation {
        Operation::BatchRename { entries } => entries.iter().any(|(p, _)| inside(p)),
        Operation::Copy { paths, folder }
        | Operation::Move { paths, folder }
        | Operation::GroupSelection { paths, folder, .. } => {
            inside(folder) || paths.iter().any(|p| inside(p))
        }
        Operation::Clipboard { paths, .. }
        | Operation::Recycle { paths }
        | Operation::DeletePermanently { paths } => paths.iter().any(|p| inside(p)),
        Operation::Paste { folder }
        | Operation::NewFolder { folder, .. }
        | Operation::NewFile { folder, .. }
        | Operation::Organize { folder, .. } => inside(folder),
        Operation::Rename { path, .. } | Operation::Dialog { path, .. } => inside(path),
        _ => false,
    }
}
pub struct Reply {
    pub result: Result<(), String>,
    pub refresh: Vec<PathBuf>,
    pub removed: Vec<PathBuf>,
    pub finished: bool,
    pub progress: Option<(u32, u32)>,
    pub history: Option<Result<crate::operation_journal::Snapshot, String>>,
}
#[derive(Clone)]
pub struct Worker {
    tx: Option<std::sync::mpsc::SyncSender<(Operation, usize, crate::operation_control::Token)>>,
    thread: u32,
    tokens: std::sync::Arc<std::sync::Mutex<Vec<std::sync::Weak<std::sync::atomic::AtomicBool>>>>,
}
const FILE_OPERATION_MESSAGE: u32 = 0x8000 + 37;
type History =
    Result<Option<std::sync::Arc<std::sync::Mutex<crate::operation_journal::Journal>>>, String>;
fn history_snapshot(history: &History) -> Result<crate::operation_journal::Snapshot, String> {
    let journal = history.as_ref().map_err(Clone::clone)?;
    journal
        .as_ref()
        .map(|journal| {
            journal
                .lock()
                .map(|journal| journal.snapshot.clone())
                .map_err(|_| "Operation history lock failed".into())
        })
        .unwrap_or_else(|| Ok(crate::operation_journal::Snapshot::default()))
}
fn perform_recorded(
    op: Operation,
    owner: usize,
    updates: &async_channel::Sender<Reply>,
    history: &History,
) -> Reply {
    if crate::operation_control::cancelled() {
        return Reply {
            result: Err("File operation cancelled before making changes.".into()),
            refresh: vec![],
            removed: vec![],
            finished: true,
            progress: None,
            history: Some(history_snapshot(history)),
        };
    }
    if matches!(&op, Operation::ReviewHistory) {
        let mut snapshot = history_snapshot(history);
        let result = match &mut snapshot {
            Ok(snapshot) => snapshot.review_interrupted(),
            Err(error) => Err(error.clone()),
        };
        return Reply {
            result,
            refresh: vec![],
            removed: vec![],
            finished: true,
            progress: None,
            history: Some(snapshot),
        };
    }
    if matches!(&op, Operation::ArchiveHistory) {
        let result = (|| {
            let journal = history
                .as_ref()
                .map_err(Clone::clone)?
                .as_ref()
                .ok_or("Operation history is unavailable")?;
            journal
                .lock()
                .map_err(|_| "Operation history lock failed")?
                .archive()
                .map(|_| ())
        })();
        let mut snapshot = history_snapshot(history);
        if let (Err(error), Ok(snapshot)) = (&result, &mut snapshot) {
            snapshot.warning = Some(format!("History was not archived: {error}"));
        }
        return Reply {
            result,
            refresh: vec![],
            removed: vec![],
            finished: true,
            progress: None,
            history: Some(snapshot),
        };
    }
    let targets = match &op {
        Operation::Recycle { paths } | Operation::DeletePermanently { paths } => paths.clone(),
        _ => vec![],
    };
    let mut refresh = vec![];
    let label = match &op {
        Operation::PutBack { job } => Some(
            if history_snapshot(history)
                .ok()
                .and_then(|snapshot| snapshot.jobs.into_iter().find(|row| row.id == *job))
                .is_some_and(|job| job.label == "Undo copy")
            {
                "Redo copy"
            } else {
                "Put Back"
            },
        ),
        Operation::BatchRename { .. } => Some("Batch rename"),
        Operation::UndoBatchRename { redo } => Some(if *redo {
            "Redo batch rename"
        } else {
            "Undo batch rename"
        }),
        Operation::UndoCopy { redo } => Some(if *redo { "Redo copy" } else { "Undo copy" }),
        Operation::RedoRecycle => Some("Redo recycle"),
        Operation::UndoMove { redo } => Some(if *redo { "Redo move" } else { "Undo move" }),
        Operation::UndoRename { redo } => Some(if *redo { "Redo rename" } else { "Undo rename" }),
        Operation::ArchiveHistory
        | Operation::History
        | Operation::ReviewHistory
        | Operation::Clipboard { .. }
        | Operation::Dialog { .. } => None,
        Operation::Paste { .. } => Some("Paste files"),
        Operation::Copy { .. } => Some("Copy files"),
        Operation::Move { .. } => Some("Move files"),
        Operation::Rename { path, name }
            if path.file_name() == Some(std::ffi::OsStr::new(name)) =>
        {
            None
        }
        Operation::Rename { .. } => Some("Rename"),
        Operation::NewFolder { .. } => Some("New folder"),
        Operation::NewFile { .. } => None,
        Operation::GroupSelection { .. } => Some("New folder with selection"),
        Operation::Organize { .. } => Some(ORGANIZE_LABEL),
        Operation::Recycle { .. } => Some("Move to Recycle Bin"),
        Operation::DeletePermanently { .. } => Some("Delete permanently"),
    };
    let context = label
        .map(
            |label| -> Result<Option<crate::operation_journal::Context>, String> {
                let journal = history.as_ref().map_err(Clone::clone)?;
                let Some(journal) = journal else {
                    return Ok(None);
                };
                let mut locked = journal
                    .lock()
                    .map_err(|_| "Operation history lock failed")?;
                let job = locked.next_job()?;
                locked.append(crate::operation_journal::Event::Begin {
                    job,
                    label: label.into(),
                })?;
                Ok(Some(crate::operation_journal::Context {
                    journal: journal.clone(),
                    job,
                }))
            },
        )
        .transpose()
        .map(Option::flatten);
    let result = match context {
        Ok(context) => {
            let mut result = execute_recorded(
                op,
                owner,
                &mut refresh,
                Some(updates.clone()),
                context.clone(),
            )
            .map_err(|e| e.to_string());
            if let Some(context) = context {
                if result.is_ok() {
                    let failed=context.journal.lock().map(|journal|journal.snapshot.jobs.iter().rev().find(|job|job.id==context.job).is_some_and(|job|job.outcomes.iter().any(|event|matches!(event,crate::operation_journal::Event::After{hresult,..} if *hresult<0)))).unwrap_or(true);
                    if failed {
                        result = Err(
                            "One or more items failed. Review operation history before retrying."
                                .into(),
                        );
                    }
                }
                if let Err(error) = context.append(crate::operation_journal::Event::End {
                    job: context.job,
                    error: result.as_ref().err().cloned(),
                }) {
                    result = Err(format!(
                        "Operation may have completed, but its history could not be saved. Review the files before retrying. {error}"
                    ));
                }
            }
            result
        }
        Err(error) => Err(format!(
            "File operation stopped before making changes: {error}"
        )),
    };
    Reply {
        result,
        refresh,
        removed: targets
            .into_iter()
            .filter(|path| {
                std::fs::symlink_metadata(path)
                    .is_err_and(|e| e.kind() == std::io::ErrorKind::NotFound)
            })
            .collect(),
        finished: true,
        progress: None,
        history: Some(history_snapshot(history)),
    }
}
impl Drop for Worker {
    fn drop(&mut self) {
        if std::sync::Arc::strong_count(&self.tokens) == 1 {
            self.cancel();
        }
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
    #[cfg(test)]
    pub fn start() -> (Self, async_channel::Receiver<Reply>) {
        Self::start_inner(None)
    }
    pub fn start_with_history(path: PathBuf) -> (Self, async_channel::Receiver<Reply>) {
        Self::start_inner(Some(path))
    }
    fn start_inner(history_path: Option<PathBuf>) -> (Self, async_channel::Receiver<Reply>) {
        let (tx, rx) =
            std::sync::mpsc::sync_channel::<(Operation, usize, crate::operation_control::Token)>(8);
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
                            removed: vec![],
                            finished: true,
                            progress: None,
                            history: None,
                        });
                        return;
                    }
                }
                let history = history_path
                    .map(|path| {
                        crate::operation_journal::Journal::open(&path)
                            .map(|journal| std::sync::Arc::new(std::sync::Mutex::new(journal)))
                    })
                    .transpose();
                let _ = out.send_blocking(Reply {
                    result: Ok(()),
                    refresh: vec![],
                    removed: vec![],
                    finished: false,
                    progress: None,
                    history: Some(history_snapshot(&history)),
                });
                'messages: while unsafe { GetMessageW(&mut message, std::ptr::null_mut(), 0, 0) }
                    > 0
                {
                    if message.message == FILE_OPERATION_MESSAGE {
                        loop {
                            match rx.try_recv() {
                                Ok((op, owner, token)) => {
                                    let _scope = crate::operation_control::enter(token);
                                    if out
                                        .send_blocking(perform_recorded(op, owner, &out, &history))
                                        .is_err()
                                    {
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
                tokens: Default::default(),
            },
            replies,
        )
    }
    pub fn send(&self, operation: Operation, owner: usize) -> Result<(), String> {
        if touches_archive(&operation) {
            return Err(READ_ONLY_ARCHIVE.into());
        }
        let token = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        if let Ok(mut tokens) = self.tokens.lock() {
            tokens.retain(|token| token.strong_count() != 0);
            tokens.push(std::sync::Arc::downgrade(&token));
        }
        self.tx
            .as_ref()
            .unwrap()
            .try_send((operation, owner, token.clone()))
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
            token.store(true, std::sync::atomic::Ordering::Relaxed);
            return Err("Windows file operation worker is unavailable.".into());
        }
        Ok(())
    }
    pub fn cancel(&self) {
        if let Ok(tokens) = self.tokens.lock() {
            for token in tokens.iter().filter_map(std::sync::Weak::upgrade) {
                token.store(true, std::sync::atomic::Ordering::Relaxed);
            }
        }
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
pub const ORGANIZE_LABEL: &str = "Organize folder";
/// Create each group's folder when missing and move its unchanged files in,
/// never replacing a name: a clash keeps both. A file that changed since the
/// review, or is no longer beside its group, stays where it is.
fn organize(
    folder: &Path,
    groups: &[(String, Vec<crate::organize::Item>)],
    refresh: &mut Vec<PathBuf>,
    journal: Option<&crate::operation_journal::Context>,
) -> windows::core::Result<()> {
    use crate::operation_journal::{Event, Fingerprint, WirePath};
    use std::{fs::OpenOptions, os::windows::fs::OpenOptionsExt};
    let context = journal.ok_or_else(|| invalid("Organize requires operation history"))?;
    let _held = crate::safe_rename::stable_parent(folder)?;
    refresh.push(folder.to_path_buf());
    let mut reserved = std::collections::HashSet::<PathBuf>::new();
    for (name, items) in groups {
        crate::operation_control::check().map_err(|e| invalid(&e))?;
        // A group is one folder ("Documents") or a year and month inside it
        // ("2024\05 May"); every level must be a valid name, and each level
        // created here is journaled, so undo removes them innermost first.
        let levels: Vec<&str> = name.split('\\').collect();
        if items.is_empty() || levels.len() > 2 || !levels.iter().all(|level| valid_name(level)) {
            continue;
        }
        let mut target = folder.to_path_buf();
        let mut usable = true;
        for level in levels {
            let parent = target.clone();
            target = parent.join(level);
            match std::fs::symlink_metadata(&target) {
                Ok(meta) => {
                    use std::os::windows::fs::MetadataExt;
                    if !meta.is_dir() || meta.file_attributes() & 0x400 != 0 {
                        usable = false;
                        break;
                    }
                }
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                    context.append(Event::Before {
                        job: context.job,
                        action: "new_folder".into(),
                        source: None,
                        folder: Some(WirePath::new(&parent)),
                        name: Some(level.encode_utf16().collect()),
                        proof: false,
                    })?;
                    let created = std::fs::create_dir(&target);
                    context.append(Event::After {
                        job: context.job,
                        action: "new_folder".into(),
                        source: None,
                        target: created.as_ref().ok().map(|_| WirePath::new(&target)),
                        parsing_name: None,
                        hresult: created.as_ref().err().map_or(0, |e| {
                            windows::core::HRESULT::from_win32(e.raw_os_error().unwrap_or(1) as u32).0
                        }),
                        identity: created.as_ref().ok().and_then(|_| Fingerprint::at(&target)),
                    })?;
                    if created.is_err() {
                        usable = false;
                        break;
                    }
                }
                Err(_) => {
                    usable = false;
                    break;
                }
            }
        }
        if !usable {
            continue;
        }
        let _target_held = crate::safe_rename::stable_parent(&target)?;
        refresh.push(target.clone());
        for item in items {
            crate::operation_control::check().map_err(|e| invalid(&e))?;
            if item.path.parent() != Some(folder) {
                continue;
            }
            let Some(file_name) = item.path.file_name().and_then(|n| n.to_str()) else {
                continue;
            };
            let Ok(file) = OpenOptions::new()
                .read(true)
                .share_mode(1 | 4)
                .custom_flags(0x02000000 | 0x00200000 | 0x00100000)
                .open(&item.path)
            else {
                continue;
            };
            let unchanged = file
                .metadata()
                .is_ok_and(|meta| meta.is_file() && crate::organize::stamp(&meta) == item.stamp);
            if !unchanged {
                continue;
            }
            let Ok(expected) = Fingerprint::read(&file) else {
                continue;
            };
            let Some(destination) = crate::organize::keep_both(&target, file_name, &|path| {
                reserved.contains(path) || crate::organize::exists(path)
            }) else {
                continue;
            };
            reserved.insert(destination.clone());
            // The rename itself refuses to replace, so a name taken after the
            // check fails this item instead of overwriting anything.
            let _ = crate::safe_rename::move_guarded(&item.path, &destination, context, &expected);
        }
    }
    Ok(())
}
fn invalid(message: &str) -> windows::core::Error {
    windows::core::Error::new(windows::Win32::Foundation::E_INVALIDARG, message)
}
fn recycle_guarded(
    items: &[crate::operation_journal::GuardedItem],
    owner: usize,
    refresh: &mut Vec<PathBuf>,
    updates: Option<async_channel::Sender<Reply>>,
    context: &crate::operation_journal::Context,
) -> windows::core::Result<()> {
    use std::{fs::OpenOptions, os::windows::fs::OpenOptionsExt};
    let mut held = vec![];
    let mut sources = vec![];
    for entry in items {
        crate::operation_control::check().map_err(|e| invalid(&e))?;
        recycle_allowed(&entry.path)?;
        let parent = entry
            .path
            .parent()
            .ok_or_else(|| invalid("The item has no parent folder"))?;
        held.extend(crate::safe_rename::stable_parent(parent)?);
        let file = OpenOptions::new()
            .read(true)
            .share_mode(1 | 4)
            .custom_flags(0x02000000 | 0x00200000 | 0x00100000)
            .open(&entry.path)
            .map_err(|_| invalid("An item is unavailable or busy. The inverse was stopped."))?;
        if !crate::operation_journal::Fingerprint::read(&file)
            .map_err(|e| invalid(&e))?
            .compatible(&entry.expected)
        {
            return Err(invalid(
                "An item changed after the operation. No items were authorized for recycling.",
            ));
        }
        held.push(file);
        sources.push(item(&entry.path)?);
        refresh.push(parent.into());
    }
    unsafe {
        let files: IFileOperation = CoCreateInstance(&FileOperation, None, CLSCTX_INPROC_SERVER)?;
        files.SetOwnerWindow(HWND(owner as _))?;
        files.SetOperationFlags(
            FOFX_ADDUNDORECORD | FOFX_RECYCLEONDELETE | FOF_WANTNUKEWARNING | FOF_NOCONFIRMATION,
        )?;
        let paths = items
            .iter()
            .map(|entry| entry.path.clone())
            .collect::<Vec<_>>();
        let sink: IFileOperationProgressSink = crate::file_progress::OperationProgress {
            updates: updates.unwrap_or_else(|| async_channel::unbounded().0),
            targets: paths.clone(),
            evidence_roots: paths,
            journal: Some(context.clone()),
            move_sources: Default::default(),
            copy_targets: Default::default(),
            proofs: Default::default(),
            required: items
                .iter()
                .map(|entry| (entry.path.clone(), entry.expected.clone()))
                .collect(),
        }
        .into();
        files.Advise(&sink)?;
        let guard: IFileOperationProgressSink = crate::recycle_guard::RecycleGuard.into();
        for source in &sources {
            files.DeleteItem(source, Some(&guard))?;
        }
        files.PerformOperations()?;
        finish(&files)
    }
}
fn move_with_shell_guard(
    plan: &crate::operation_journal::MovePlan,
    owner: usize,
    refresh: &mut Vec<PathBuf>,
    updates: Option<async_channel::Sender<Reply>>,
    context: &crate::operation_journal::Context,
) -> windows::core::Result<()> {
    use std::{fs::OpenOptions, os::windows::fs::OpenOptionsExt};
    let parent = plan
        .path
        .parent()
        .ok_or_else(|| invalid("Source parent is unavailable"))?;
    let destination = plan
        .target
        .parent()
        .ok_or_else(|| invalid("Original parent is unavailable"))?;
    let mut held = crate::safe_rename::stable_parent(parent)?;
    held.extend(crate::safe_rename::stable_parent(destination)?);
    match std::fs::symlink_metadata(&plan.target) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        _ => {
            return Err(invalid(
                "The original destination exists or cannot be checked. Nothing was replaced.",
            ));
        }
    }
    let file = OpenOptions::new()
        .read(true)
        .share_mode(1 | 4)
        .custom_flags(0x02000000 | 0x00200000 | 0x00100000)
        .open(&plan.path)
        .map_err(|e| invalid(&e.to_string()))?;
    if !crate::operation_journal::Fingerprint::read(&file)
        .map_err(|e| invalid(&e))?
        .compatible(&plan.expected)
    {
        return Err(invalid("The moved item changed. Its inverse was stopped."));
    }
    held.push(file);
    refresh.push(parent.into());
    refresh.push(destination.into());
    let name =
        wide(Path::new(plan.target.file_name().ok_or_else(|| {
            invalid("The original name is unavailable")
        })?));
    unsafe {
        let files: IFileOperation = CoCreateInstance(&FileOperation, None, CLSCTX_INPROC_SERVER)?;
        files.SetOwnerWindow(HWND(owner as _))?;
        files.SetOperationFlags(
            FOF_RENAMEONCOLLISION | FOF_NOCONFIRMMKDIR | FOFX_ADDUNDORECORD | FOFX_EARLYFAILURE,
        )?;
        let sink: IFileOperationProgressSink = crate::file_progress::OperationProgress {
            updates: updates.unwrap_or_else(|| async_channel::unbounded().0),
            targets: vec![],
            evidence_roots: vec![plan.path.clone()],
            required: [(plan.path.clone(), plan.expected.clone())]
                .into_iter()
                .collect(),
            journal: Some(context.clone()),
            move_sources: Default::default(),
            copy_targets: Default::default(),
            proofs: Default::default(),
        }
        .into();
        files.Advise(&sink)?;
        files.MoveItem(
            &item(&plan.path)?,
            &item(destination)?,
            PCWSTR(name.as_ptr()),
            None::<&IFileOperationProgressSink>,
        )?;
        files.PerformOperations()?;
        finish(&files)
    }
}
fn recycled_sources(
    paths: &[PathBuf],
) -> windows::core::Result<std::collections::HashMap<PathBuf, IShellItem>> {
    let bin: IShellItem =
        unsafe { SHGetKnownFolderItem(&FOLDERID_RecycleBinFolder, KF_FLAG_DEFAULT, None)? };
    let entries: IEnumShellItems =
        unsafe { bin.BindToHandler(None::<&IBindCtx>, &BHID_EnumItems)? };
    let mut sources = std::collections::HashMap::new();
    loop {
        let mut next = [None];
        let mut fetched = 0;
        unsafe {
            entries.Next(&mut next, Some(&mut fetched))?;
        }
        if fetched == 0 {
            break;
        }
        let candidate = next[0]
            .take()
            .ok_or_else(|| invalid("Windows returned an invalid recycled item."))?;
        if let Some(path) = crate::file_progress::path(Some(&candidate)) {
            for wanted in paths {
                if seek_engine::paths::cmp_ci(
                    &seek_engine::paths::internal(&path.path()),
                    &seek_engine::paths::internal(wanted),
                )
                .is_eq()
                {
                    sources.insert(wanted.clone(), candidate.clone());
                }
            }
        }
        if sources.len() == paths.len() {
            break;
        }
    }
    Ok(sources)
}
fn put_back(
    job: u64,
    owner: usize,
    refresh: &mut Vec<PathBuf>,
    updates: Option<async_channel::Sender<Reply>>,
    journal: Option<crate::operation_journal::Context>,
) -> windows::core::Result<()> {
    use std::{fs::OpenOptions, os::windows::fs::OpenOptionsExt};
    let context = journal.ok_or_else(|| invalid("Put Back requires operation history."))?;
    let plan = context
        .journal
        .lock()
        .map_err(|_| invalid("Operation history lock failed"))?
        .snapshot
        .put_back_plan(job)
        .ok_or_else(|| invalid("No confirmed recycled items remain available for Put Back."))?;
    // Restore through the Recycle Bin namespace and its canonical verb. Private
    // $I metadata is owned entirely by Windows, never edited or removed by Seek.
    let paths = plan
        .iter()
        .map(|entry| entry.recycled.clone())
        .collect::<Vec<_>>();
    let mut sources = recycled_sources(&paths)?;
    let mut held = vec![];
    let mut queued = vec![];
    // Validate the entire request before queuing its first mutation. Keep parent
    // identities and read locks alive through Shell execution, without blocking moves.
    for entry in &plan {
        let parent = entry
            .original
            .parent()
            .ok_or_else(|| invalid("The original parent is unavailable."))?;
        held.extend(crate::safe_rename::stable_parent(parent)?);
        held.extend(crate::safe_rename::stable_parent(
            entry
                .recycled
                .parent()
                .ok_or_else(|| invalid("The recycled parent is unavailable."))?,
        )?);
        match std::fs::symlink_metadata(&entry.original) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Ok(_) => {
                return Err(invalid(
                    "An item already exists at the original location. Put Back did not replace it. Move or rename that item before retrying.",
                ));
            }
            Err(e) => {
                return Err(invalid(&format!(
                    "Cannot inspect the original location: {e}"
                )));
            }
        }
        let file = OpenOptions::new()
            .read(true)
            .share_mode(1 | 4)
            .custom_flags(0x02000000 | 0x00200000 | 0x00100000)
            .open(&entry.recycled)
            .map_err(|_| invalid("The recorded recycled item is no longer available."))?;
        if !crate::operation_journal::Fingerprint::read(&file)
            .map_err(|e| invalid(&e))?
            .compatible(&entry.expected)
        {
            return Err(invalid(
                "The recorded recycled item changed. Put Back did not move it.",
            ));
        }
        let tracked_file = file.try_clone().map_err(|e| invalid(&e.to_string()))?;
        held.push(file);
        let source = sources
            .remove(&entry.recycled)
            .ok_or_else(|| invalid("The recorded item is no longer in the Recycle Bin."))?;
        let resolved = crate::file_progress::path(Some(&source))
            .ok_or_else(|| invalid("Windows could not identify the recycled item."))?
            .path();
        if !seek_engine::paths::cmp_ci(
            &seek_engine::paths::internal(&resolved),
            &seek_engine::paths::internal(&entry.recycled),
        )
        .is_eq()
        {
            return Err(invalid(
                "The recycled item no longer matches its recorded location.",
            ));
        }
        queued.push((source, tracked_file));
        refresh.push(parent.into());
    }
    for ((source, tracked_file), entry) in queued.iter().zip(&plan) {
        context.append(crate::operation_journal::Event::Before {
            job: context.job,
            action: "restore".into(),
            source: Some(crate::operation_journal::WirePath::new(&entry.recycled)),
            folder: entry
                .original
                .parent()
                .map(crate::operation_journal::WirePath::new),
            name: entry
                .original
                .file_name()
                .map(|name| name.encode_wide().collect()),
            proof: false,
        })?;
        let invoked = unsafe {
            use windows::Win32::UI::WindowsAndMessaging::*;
            let menu: IContextMenu = source.BindToHandler(None::<&IBindCtx>, &BHID_SFUIObject)?;
            let popup = CreatePopupMenu()?;
            let result = (|| {
                menu.QueryContextMenu(popup, 0, 1, 0x7fff, CMF_NORMAL)
                    .ok()?;
                let info = CMINVOKECOMMANDINFO {
                    cbSize: std::mem::size_of::<CMINVOKECOMMANDINFO>() as u32,
                    fMask: SEE_MASK_NOASYNC, // CMIC_MASK_NOASYNC shares this SDK flag.
                    hwnd: HWND(owner as _),
                    lpVerb: windows::core::PCSTR(c"undelete".as_ptr().cast()),
                    nShow: SW_SHOWNORMAL.0,
                    ..Default::default()
                };
                menu.InvokeCommand(&info)
            })();
            let _ = DestroyMenu(popup);
            result
        };
        let actual = crate::operation_snapshot::final_path(tracked_file).ok();
        let confirmed = invoked.is_ok()
            && actual.as_ref().is_some_and(|path| {
                !seek_engine::paths::cmp_ci(
                    &seek_engine::paths::internal(path),
                    &seek_engine::paths::internal(&entry.recycled),
                )
                .is_eq()
            })
            && crate::operation_journal::Fingerprint::key(tracked_file).ok()
                == Some((entry.expected.volume, entry.expected.id))
            && std::fs::symlink_metadata(&entry.recycled)
                .is_err_and(|e| e.kind() == std::io::ErrorKind::NotFound);
        let failure = invoked.err().or_else(|| (!confirmed).then(|| invalid(
            "Windows did not confirm restoration to the original location. Review the Recycle Bin and operation history before retrying.")));
        context.append(crate::operation_journal::Event::After {
            job: context.job,
            action: "restore".into(),
            source: Some(crate::operation_journal::WirePath::new(&entry.recycled)),
            target: confirmed
                .then(|| crate::operation_journal::WirePath::new(actual.as_ref().unwrap())),
            parsing_name: None,
            hresult: failure.as_ref().map_or(0, |e| e.code().0),
            identity: confirmed.then(|| entry.expected.clone()),
        })?;
        if let Some(error) = failure {
            return Err(error);
        }
        if let Some(updates) = &updates {
            let _ = updates.try_send(Reply {
                result: Ok(()),
                refresh: entry
                    .original
                    .parent()
                    .map(Path::to_path_buf)
                    .into_iter()
                    .collect(),
                removed: vec![],
                finished: false,
                progress: None,
                history: None,
            });
        }
    }
    Ok(())
}
fn execute_recorded(
    op: Operation,
    owner: usize,
    refresh: &mut Vec<PathBuf>,
    updates: Option<async_channel::Sender<Reply>>,
    journal: Option<crate::operation_journal::Context>,
) -> windows::core::Result<()> {
    let op = match op {
        Operation::Paste { folder } => {
            let (paths, cut) = clipboard_read()?;
            if cut {
                Operation::Move { paths, folder }
            } else {
                Operation::Copy { paths, folder }
            }
        }
        other => other,
    };
    if let Operation::BatchRename { entries } = &op {
        let plans = entries
            .iter()
            .map(|(path, name)| {
                if !valid_name(name) {
                    return Err(invalid("A proposed name is invalid"));
                }
                Ok(crate::operation_journal::MovePlan {
                    path: path.clone(),
                    target: path.with_file_name(name),
                    expected: crate::operation_journal::Fingerprint::at(path)
                        .ok_or_else(|| invalid("A selected item is unavailable"))?,
                })
            })
            .collect::<windows::core::Result<Vec<_>>>()?;
        let context = journal
            .as_ref()
            .ok_or_else(|| invalid("Batch rename requires operation history"))?;
        refresh.extend(
            plans
                .iter()
                .filter_map(|plan| plan.path.parent().map(Path::to_path_buf)),
        );
        return crate::batch_rename::execute(plans, context);
    }
    if let Operation::UndoBatchRename { redo } = &op {
        let context = journal
            .as_ref()
            .ok_or_else(|| invalid("Undo requires operation history"))?;
        let plans = context
            .journal
            .lock()
            .map_err(|_| invalid("History lock failed"))?
            .snapshot
            .batch_rename_plan(*redo, Some(context.job))
            .ok_or_else(|| invalid("No verified batch rename is available"))?;
        refresh.extend(
            plans
                .iter()
                .filter_map(|plan| plan.path.parent().map(Path::to_path_buf)),
        );
        return crate::batch_rename::execute(plans, context);
    }
    if let Operation::GroupSelection {
        paths,
        folder,
        name,
    } = &op
    {
        use crate::operation_journal::{Event, Fingerprint, MovePlan, WirePath};
        use std::{fs::OpenOptions, os::windows::fs::OpenOptionsExt};
        let context = journal
            .as_ref()
            .ok_or_else(|| invalid("Grouping requires operation history"))?;
        if paths.is_empty() || !valid_name(name) {
            return Err(invalid("Select items and choose a valid folder name."));
        }
        let target = folder.join(name);
        let mut held = crate::safe_rename::stable_parent(folder)?;
        let mut plans = vec![];
        for path in paths {
            crate::operation_control::check().map_err(|e| invalid(&e))?;
            if path.parent() != Some(folder.as_path()) {
                return Err(invalid(
                    "New Folder with Selection requires items from the same open folder.",
                ));
            }
            let file = OpenOptions::new()
                .read(true)
                .share_mode(1 | 4)
                .custom_flags(0x02000000 | 0x00200000 | 0x00100000)
                .open(path)
                .map_err(|e| invalid(&e.to_string()))?;
            let expected = Fingerprint::read(&file).map_err(|e| invalid(&e))?;
            plans.push(MovePlan {
                path: path.clone(),
                target: target.join(
                    path.file_name()
                        .ok_or_else(|| invalid("An item has no name"))?,
                ),
                expected,
            });
            held.push(file);
        }
        refresh.push(folder.clone());
        context.append(Event::Before {
            job: context.job,
            action: "new_folder".into(),
            source: None,
            folder: Some(WirePath::new(folder)),
            name: Some(name.encode_utf16().collect()),
            proof: false,
        })?;
        let created = std::fs::create_dir(&target).map_err(|e| {
            invalid(&format!(
                "The folder could not be created; no selected items were moved: {e}"
            ))
        });
        context.append(Event::After {
            job: context.job,
            action: "new_folder".into(),
            source: None,
            target: created.as_ref().ok().map(|_| WirePath::new(&target)),
            parsing_name: None,
            hresult: created.as_ref().err().map_or(0, |e| e.code().0),
            identity: created.as_ref().ok().and_then(|_| Fingerprint::at(&target)),
        })?;
        created?;
        held.extend(crate::safe_rename::stable_parent(&target)?);
        for plan in plans {
            crate::operation_control::check().map_err(|e| invalid(&e))?;
            crate::safe_rename::move_guarded(&plan.path, &plan.target, context, &plan.expected)?;
        }
        refresh.push(target);
        return Ok(());
    }
    if let Operation::NewFile { folder, name } = &op {
        if !valid_name(name) {
            return Err(invalid("Choose a valid Windows file name."));
        }
        refresh.push(folder.clone());
        return std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(folder.join(name))
            .map(drop)
            .map_err(|e| {
                invalid(&if e.kind() == std::io::ErrorKind::AlreadyExists {
                    "An item with this name already exists.".to_string()
                } else {
                    e.to_string()
                })
            });
    }
    if let Operation::Organize { folder, groups } = &op {
        return organize(folder, groups, refresh, journal.as_ref());
    }
    if let Operation::UndoCopy { redo } = &op {
        let context = journal
            .as_ref()
            .ok_or_else(|| invalid("Undo requires history"))?;
        if *redo {
            let job = context
                .journal
                .lock()
                .map_err(|_| invalid("History lock failed"))?
                .snapshot
                .copy_redo_job(Some(context.job))
                .ok_or_else(|| invalid("No copied items are available to redo"))?;
            return put_back(job, owner, refresh, updates, journal);
        }
        let plan = context
            .journal
            .lock()
            .map_err(|_| invalid("History lock failed"))?
            .snapshot
            .copy_undo_plan(Some(context.job))
            .ok_or_else(|| invalid("No complete fresh-copy snapshot is available to undo"))?;
        return recycle_guarded(&plan.items, owner, refresh, updates, context);
    }
    if matches!(&op, Operation::RedoRecycle) {
        let context = journal
            .as_ref()
            .ok_or_else(|| invalid("Redo requires history"))?;
        let plan = context
            .journal
            .lock()
            .map_err(|_| invalid("History lock failed"))?
            .snapshot
            .recycle_redo_plan(Some(context.job))
            .ok_or_else(|| invalid("No unchanged restored items are available to recycle again"))?;
        return recycle_guarded(&plan.items, owner, refresh, updates, context);
    }
    if let Operation::PutBack { job } = &op {
        return put_back(*job, owner, refresh, updates, journal);
    }
    if let Operation::UndoMove { redo } = &op {
        let context = journal
            .as_ref()
            .ok_or_else(|| invalid("Undo requires operation history"))?;
        let (plans, created) = {
            let journal = context
                .journal
                .lock()
                .map_err(|_| invalid("Operation history lock failed"))?;
            let plans = journal
                .snapshot
                .move_plans(*redo, Some(context.job))
                .ok_or_else(|| invalid("No verified move is available to undo or redo."))?;
            let created = if *redo {
                vec![]
            } else {
                journal.snapshot.organize_created_folders(Some(context.job))
            };
            (plans, created)
        };
        if *redo {
            // Redoing an Organize recreates group folders its undo removed:
            // up to two missing levels (a year and its month) inside a folder
            // that still exists.
            for plan in &plans {
                if let Some(parent) = plan.target.parent()
                    && !crate::organize::exists(parent)
                {
                    if parent.parent().is_some_and(Path::is_dir) {
                        let _ = std::fs::create_dir(parent);
                    } else if let Some(year) = parent.parent()
                        && year.parent().is_some_and(Path::is_dir)
                        && !crate::organize::exists(year)
                    {
                        let _ = std::fs::create_dir(year).and_then(|_| std::fs::create_dir(parent));
                    }
                }
            }
        }
        // Check every root before authorizing the first mutation. Keep source
        // objects and all parent chains open through execution.
        use std::{fs::OpenOptions, os::windows::fs::OpenOptionsExt};
        let mut held = vec![];
        let mut volumes = vec![];
        for plan in &plans {
            crate::operation_control::check().map_err(|e| invalid(&e))?;
            held.extend(crate::safe_rename::stable_parent(
                plan.path
                    .parent()
                    .ok_or_else(|| invalid("Source parent is unavailable"))?,
            )?);
            let destination = crate::safe_rename::stable_parent(
                plan.target
                    .parent()
                    .ok_or_else(|| invalid("Original parent is unavailable"))?,
            )?;
            volumes.push(
                crate::operation_journal::Fingerprint::key(
                    destination
                        .last()
                        .ok_or_else(|| invalid("Original volume is unavailable"))?,
                )
                .map_err(|e| invalid(&e))?
                .0,
            );
            held.extend(destination);
            match std::fs::symlink_metadata(&plan.target) {
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                _ => {
                    return Err(invalid(
                        "An original destination exists or cannot be checked. No items were moved.",
                    ));
                }
            }
            let file = OpenOptions::new()
                .read(true)
                .share_mode(1 | 4)
                .custom_flags(0x02000000 | 0x00200000 | 0x00100000)
                .open(&plan.path)
                .map_err(|e| invalid(&e.to_string()))?;
            if !crate::operation_journal::Fingerprint::read(&file)
                .map_err(|e| invalid(&e))?
                .compatible(&plan.expected)
            {
                return Err(invalid("A moved item changed. No items were moved."));
            }
            held.push(file);
        }
        for (plan, volume) in plans.iter().zip(volumes) {
            crate::operation_control::check().map_err(|e| invalid(&e))?;
            refresh.extend(plan.path.parent().map(Path::to_path_buf));
            refresh.extend(plan.target.parent().map(Path::to_path_buf));
            if volume != plan.expected.volume {
                move_with_shell_guard(plan, owner, refresh, updates.clone(), context)?;
            } else {
                crate::safe_rename::move_guarded(
                    &plan.path,
                    &plan.target,
                    context,
                    &plan.expected,
                )?;
            }
        }
        drop(held);
        // Organize's own folders go only when empty; remove_dir never removes
        // a folder with anything in it.
        // Innermost first: "2024\05 May" goes before "2024".
        for folder in created.iter().rev() {
            let _ = std::fs::remove_dir(folder);
        }
        return Ok(());
    }

    if let Operation::UndoRename { redo } = &op {
        let context = journal
            .as_ref()
            .ok_or_else(|| invalid("Undo requires operation history"))?;
        let plan = context
            .journal
            .lock()
            .map_err(|_| invalid("Operation history lock failed"))?
            .snapshot
            .rename_plan(*redo, Some(context.job))
            .ok_or_else(|| invalid("No unchanged file rename is available to undo or redo."))?;
        if let Some(parent) = plan.path.parent() {
            refresh.push(parent.into());
        }
        return crate::safe_rename::rename_guarded(&plan.path, &plan.name, context, &plan.expected);
    }
    if let Operation::Rename { path, name } = &op {
        if let Some(parent) = path.parent() {
            refresh.push(parent.into());
        }
        return crate::safe_rename::rename(path, name, journal.as_ref());
    }
    match op {
        Operation::BatchRename { .. }
        | Operation::UndoBatchRename { .. }
        | Operation::UndoCopy { .. }
        | Operation::RedoRecycle
        | Operation::GroupSelection { .. }
        | Operation::NewFile { .. }
        | Operation::Organize { .. } => {
            unreachable!("Guarded inverses are handled before Shell queue setup")
        }
        Operation::ArchiveHistory => {
            unreachable!("History archive is handled by its owning worker")
        }
        Operation::UndoRename { .. } | Operation::UndoMove { .. } => {
            unreachable!("Undo rename is handled before Shell operations")
        }
        Operation::History | Operation::ReviewHistory => Ok(()),
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
            if let Some(updates) = updates {
                let targets = match &op {
                    Operation::Recycle { paths } | Operation::DeletePermanently { paths } => {
                        paths.clone()
                    }
                    _ => vec![],
                };
                let sink: IFileOperationProgressSink = crate::file_progress::OperationProgress {
                    updates,
                    targets,
                    journal,
                    move_sources: Default::default(),
                    copy_targets: Default::default(),
                    proofs: Default::default(),
                    evidence_roots: match &op {
                        Operation::Move { paths, .. }
                        | Operation::Copy { paths, .. }
                        | Operation::Recycle { paths } => paths.clone(),
                        _ => vec![],
                    },
                    required: Default::default(),
                }
                .into();
                files.Advise(&sink)?;
            }
            // The Shell owns its progress, error, access and cancellation dialogs.
            // Do not hide failures or turn one denied item into a silent abort.
            // Permanent deletion is reached only after the separate GPUI confirmation.
            let flags = if matches!(&op, Operation::DeletePermanently { .. }) {
                FOF_NOCONFIRMATION
            } else {
                FOFX_ADDUNDORECORD
                    | FOFX_RECYCLEONDELETE
                    | FOF_WANTNUKEWARNING
                    | FOF_NOCONFIRMMKDIR
                    | if matches!(&op, Operation::Recycle { .. }) {
                        FOF_NOCONFIRMATION
                    } else {
                        FILEOPERATION_FLAGS(0)
                    }
            };
            files.SetOperationFlags(flags)?;
            let _ = files.SetProgressMessage(match &op {
                Operation::DeletePermanently { .. } => w!("Deleting permanently..."),
                Operation::Recycle { .. } => w!("Moving to Recycle Bin..."),
                _ => w!("Updating files..."),
            });
            match op {
                Operation::Copy { paths, folder } => {
                    copy(&files, &paths, &folder, refresh)?;
                }
                Operation::PutBack { .. } => {
                    unreachable!("Put Back is handled before Shell queue setup")
                }
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
                Operation::Rename { .. } => {
                    unreachable!("native no-replace rename is handled before Shell operations")
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
                    if reply.finished {
                        return;
                    }
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
    fn permanent_delete_reports_confirmed_items_before_finishing() {
        let temp = tempfile::tempdir().unwrap();
        let keep = temp.path().join("Keep.txt");
        std::fs::write(&keep, "untouched").unwrap();
        let folder = temp.path().join("Owned folder");
        std::fs::create_dir(&folder).unwrap();
        std::fs::write(folder.join("Nested.txt"), "owned").unwrap();
        let file = temp.path().join("Résumé 🐈.txt");
        std::fs::write(&file, "owned").unwrap();
        let paths = vec![file, folder];
        let (worker, rx) = Worker::start();
        let began = std::time::Instant::now();
        worker
            .send(
                Operation::DeletePermanently {
                    paths: paths.clone(),
                },
                0,
            )
            .unwrap();
        let mut confirmed = std::collections::HashSet::new();
        let mut all_confirmed = std::collections::HashSet::new();
        let mut first = None;
        let mut progress = 0;
        loop {
            match rx.try_recv() {
                Ok(reply) => {
                    reply.result.unwrap();
                    if reply.progress.is_some() {
                        progress += 1;
                    }
                    for path in reply.removed {
                        assert!(!path.exists());
                        assert!(paths.iter().any(|p| {
                            seek_engine::paths::internal(p)
                                .eq_ignore_ascii_case(&seek_engine::paths::internal(&path))
                        }));
                        all_confirmed.insert(seek_engine::paths::internal(&path));
                        if !reply.finished {
                            first.get_or_insert_with(|| began.elapsed());
                            confirmed.insert(seek_engine::paths::internal(&path));
                        }
                    }
                    if reply.finished {
                        break;
                    }
                }
                Err(async_channel::TryRecvError::Empty) => {
                    assert!(began.elapsed().as_secs() < 30);
                    std::thread::sleep(std::time::Duration::from_millis(1));
                }
                Err(e) => panic!("{e}"),
            }
        }
        assert!(
            !confirmed.is_empty(),
            "file callbacks must reach the UI before final completion"
        );
        // Windows may report a directory's PostDelete before removing its
        // directory entry. The final existence check is authoritative there.
        assert_eq!(all_confirmed.len(), paths.len());
        assert_eq!(std::fs::read_to_string(&keep).unwrap(), "untouched");
        assert!(paths.iter().all(|p| !p.exists()));
        println!(
            "native delete: first confirmed {:.3} ms, finished {:.3} ms, {} progress callbacks",
            first.unwrap().as_secs_f64() * 1000.,
            began.elapsed().as_secs_f64() * 1000.,
            progress
        );
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

#[cfg(test)]
mod journal_tests {
    use super::*;
    fn finished(rx: &async_channel::Receiver<Reply>) -> Reply {
        let until = std::time::Instant::now() + std::time::Duration::from_secs(30);
        loop {
            match rx.try_recv() {
                Ok(reply) if reply.finished => return reply,
                Ok(_) => {}
                Err(async_channel::TryRecvError::Empty) => {
                    assert!(std::time::Instant::now() < until, "Operation timed out");
                    std::thread::sleep(std::time::Duration::from_millis(10));
                }
                Err(e) => panic!("{e}"),
            }
        }
    }
    #[test]
    fn journal_records_actual_callbacks_and_collision_then_reopens() {
        use crate::operation_journal::{Event, Journal};
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        let history = root.join("history.jsonl");
        let source = root.join("source.txt");
        std::fs::write(&source, b"owned original").unwrap();
        let existing = root.join("existing.txt");
        std::fs::write(&existing, b"keep existing").unwrap();
        let (worker, rx) = Worker::start_with_history(history.clone());
        worker
            .send(
                Operation::NewFolder {
                    folder: root.into(),
                    name: "Destination".into(),
                },
                0,
            )
            .unwrap();
        let reply = finished(&rx);
        reply.result.unwrap();
        assert!(reply.history.unwrap().unwrap().jobs[0].outcomes.iter().any(|event|matches!(event,Event::After{action,target:Some(target),hresult:0,..} if action=="new" && target.path().is_dir())));
        worker
            .send(
                Operation::Rename {
                    path: source.clone(),
                    name: "existing.txt".into(),
                },
                0,
            )
            .unwrap();
        let reply = finished(&rx);
        assert!(reply.result.is_err());
        assert_eq!(std::fs::read(&source).unwrap(), b"owned original");
        assert_eq!(std::fs::read(&existing).unwrap(), b"keep existing");
        assert!(
            reply.history.unwrap().unwrap().jobs[1]
                .outcomes
                .iter()
                .any(|event| matches!(event,Event::After{hresult,..} if *hresult<0))
        );
        worker
            .send(
                Operation::Rename {
                    path: source,
                    name: "renamed.txt".into(),
                },
                0,
            )
            .unwrap();
        finished(&rx).result.unwrap();
        worker
            .send(
                Operation::Move {
                    paths: vec![root.join("renamed.txt")],
                    folder: root.join("Destination"),
                },
                0,
            )
            .unwrap();
        let reply = finished(&rx);
        reply.result.unwrap();
        let snapshot = reply.history.unwrap().unwrap();
        assert_eq!(snapshot.jobs.len(), 4);
        assert!(snapshot.jobs.iter().all(|job| job.finished));
        assert!(snapshot.jobs[1].error.is_some());
        assert!(snapshot.jobs[3].outcomes.iter().any(|event|matches!(event,Event::After{action,target:Some(target),hresult,..} if action=="move" && crate::operation_journal::item_completed(*hresult) && target.path().exists())), "{:?}",snapshot.jobs[3].outcomes);
        assert_eq!(
            std::fs::read(root.join("Destination/renamed.txt")).unwrap(),
            b"owned original"
        );
        drop(worker);
        let until = std::time::Instant::now() + std::time::Duration::from_secs(5);
        let journal = loop {
            match Journal::open(&history) {
                Ok(journal) => break journal,
                Err(error) => {
                    assert!(std::time::Instant::now() < until, "{error}");
                    std::thread::sleep(std::time::Duration::from_millis(10));
                }
            }
        };
        assert_eq!(journal.snapshot.jobs, snapshot.jobs);
    }
    #[test]
    fn undo_redo_rename_preserve_bytes_and_refuse_changed_or_replaced_files() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        let original = root.join("original.txt");
        let renamed = root.join("renamed.txt");
        std::fs::write(&original, b"original").unwrap();
        let (worker, rx) = Worker::start_with_history(root.join("history.jsonl"));
        worker
            .send(
                Operation::Rename {
                    path: original.clone(),
                    name: "renamed.txt".into(),
                },
                0,
            )
            .unwrap();
        let reply = finished(&rx);
        reply.result.unwrap();
        assert!(
            reply
                .history
                .unwrap()
                .unwrap()
                .rename_plan(false, None)
                .is_some()
        );
        worker
            .send(Operation::UndoRename { redo: false }, 0)
            .unwrap();
        let reply = finished(&rx);
        reply.result.unwrap();
        assert_eq!(std::fs::read(&original).unwrap(), b"original");
        assert!(!renamed.exists());
        assert!(
            reply
                .history
                .unwrap()
                .unwrap()
                .rename_plan(true, None)
                .is_some()
        );
        worker
            .send(Operation::UndoRename { redo: true }, 0)
            .unwrap();
        finished(&rx).result.unwrap();
        assert_eq!(std::fs::read(&renamed).unwrap(), b"original");
        assert!(!original.exists());
        std::fs::write(&renamed, b"modified").unwrap();
        worker
            .send(Operation::UndoRename { redo: false }, 0)
            .unwrap();
        assert!(finished(&rx).result.is_err());
        assert_eq!(std::fs::read(&renamed).unwrap(), b"modified");
        assert!(!original.exists());
        // Same bytes on a different file identity must not pass as the renamed object.
        std::fs::rename(&renamed, root.join("moved-away.txt")).unwrap();
        std::fs::write(&renamed, b"original").unwrap();
        worker
            .send(Operation::UndoRename { redo: false }, 0)
            .unwrap();
        assert!(finished(&rx).result.is_err());
        assert_eq!(std::fs::read(&renamed).unwrap(), b"original");
        assert_eq!(
            std::fs::read(root.join("moved-away.txt")).unwrap(),
            b"modified"
        );
    }
    #[test]
    fn undo_collision_can_be_retried_without_overwriting_the_collision() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        let original = root.join("original.txt");
        std::fs::write(&original, b"original").unwrap();
        let (worker, rx) = Worker::start_with_history(root.join("history.jsonl"));
        worker
            .send(
                Operation::Rename {
                    path: original.clone(),
                    name: "renamed.txt".into(),
                },
                0,
            )
            .unwrap();
        finished(&rx).result.unwrap();
        std::fs::write(&original, b"collision").unwrap();
        worker
            .send(Operation::UndoRename { redo: false }, 0)
            .unwrap();
        let reply = finished(&rx);
        assert!(reply.result.is_err());
        assert_eq!(std::fs::read(&original).unwrap(), b"collision");
        assert_eq!(
            std::fs::read(root.join("renamed.txt")).unwrap(),
            b"original"
        );
        assert!(
            reply
                .history
                .unwrap()
                .unwrap()
                .rename_plan(false, None)
                .is_some()
        );
        std::fs::remove_file(&original).unwrap();
        worker
            .send(Operation::UndoRename { redo: false }, 0)
            .unwrap();
        finished(&rx).result.unwrap();
        assert_eq!(std::fs::read(&original).unwrap(), b"original");
    }
    #[test]
    fn rename_undo_survives_worker_restart() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        let original = root.join("original.txt");
        let history = root.join("history.jsonl");
        std::fs::write(&original, b"restart-safe").unwrap();
        let (worker, rx) = Worker::start_with_history(history.clone());
        worker
            .send(
                Operation::Rename {
                    path: original.clone(),
                    name: "renamed.txt".into(),
                },
                0,
            )
            .unwrap();
        finished(&rx).result.unwrap();
        drop(worker);
        let until = std::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            match crate::operation_journal::Journal::open(&history) {
                Ok(journal) => {
                    assert!(journal.snapshot.rename_plan(false, None).is_some());
                    break;
                }
                Err(error) => {
                    assert!(std::time::Instant::now() < until, "{error}");
                    std::thread::sleep(std::time::Duration::from_millis(10));
                }
            }
        }
        let (worker, rx) = Worker::start_with_history(history);
        worker
            .send(Operation::UndoRename { redo: false }, 0)
            .unwrap();
        finished(&rx).result.unwrap();
        assert_eq!(std::fs::read(original).unwrap(), b"restart-safe");
        assert!(!root.join("renamed.txt").exists());
    }
    #[test]
    fn unchanged_name_does_not_add_history_or_invalidate_undo() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        let original = root.join("original.txt");
        std::fs::write(&original, b"unchanged bytes").unwrap();
        let (worker, rx) = Worker::start_with_history(root.join("history.jsonl"));
        worker
            .send(
                Operation::Rename {
                    path: original,
                    name: "renamed.txt".into(),
                },
                0,
            )
            .unwrap();
        finished(&rx).result.unwrap();
        worker
            .send(
                Operation::Rename {
                    path: root.join("renamed.txt"),
                    name: "renamed.txt".into(),
                },
                0,
            )
            .unwrap();
        let reply = finished(&rx);
        reply.result.unwrap();
        let history = reply.history.unwrap().unwrap();
        assert_eq!(history.jobs.len(), 1);
        assert!(history.rename_plan(false, None).is_some());
    }
    #[test]
    fn unavailable_history_prevents_file_changes() {
        let temp = tempfile::tempdir().unwrap();
        let history = temp.path().join("not-a-file");
        std::fs::create_dir(&history).unwrap();
        let (worker, rx) = Worker::start_with_history(history);
        worker
            .send(
                Operation::NewFolder {
                    folder: temp.path().into(),
                    name: "Must not exist".into(),
                },
                0,
            )
            .unwrap();
        let reply = finished(&rx);
        assert!(reply.result.is_err());
        assert!(reply.history.unwrap().is_err());
        assert!(!temp.path().join("Must not exist").exists());
    }
    #[test]
    fn single_file_move_undo_redo_preserves_identity_and_refuses_collision_and_edits() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        let destination = root.join("destination");
        std::fs::create_dir(&destination).unwrap();
        let original = root.join("move.txt");
        let moved = destination.join("move.txt");
        std::fs::write(&original, b"original move bytes").unwrap();
        let identity = crate::operation_journal::Fingerprint::at(&original).unwrap();
        let (worker, rx) = Worker::start_with_history(root.join("history.jsonl"));
        worker
            .send(
                Operation::Move {
                    paths: vec![original.clone()],
                    folder: destination,
                },
                0,
            )
            .unwrap();
        let reply = finished(&rx);
        reply.result.unwrap();
        let snapshot = reply.history.unwrap().unwrap();
        assert!(snapshot.move_plan(false, None).is_some(), "{snapshot:?}");
        assert_eq!(
            crate::operation_journal::Fingerprint::at(&moved).unwrap(),
            identity
        );
        assert!(!original.exists());
        std::fs::write(&original, b"collision").unwrap();
        worker.send(Operation::UndoMove { redo: false }, 0).unwrap();
        let reply = finished(&rx);
        assert!(reply.result.is_err());
        assert_eq!(std::fs::read(&original).unwrap(), b"collision");
        assert_eq!(std::fs::read(&moved).unwrap(), b"original move bytes");
        assert!(
            reply
                .history
                .unwrap()
                .unwrap()
                .move_plan(false, None)
                .is_some()
        );
        std::fs::remove_file(&original).unwrap();
        worker.send(Operation::UndoMove { redo: false }, 0).unwrap();
        let reply = finished(&rx);
        reply.result.unwrap();
        assert!(
            reply
                .history
                .unwrap()
                .unwrap()
                .move_plan(true, None)
                .is_some()
        );
        assert_eq!(
            crate::operation_journal::Fingerprint::at(&original).unwrap(),
            identity
        );
        assert!(!moved.exists());
        worker.send(Operation::UndoMove { redo: true }, 0).unwrap();
        finished(&rx).result.unwrap();
        assert_eq!(
            crate::operation_journal::Fingerprint::at(&moved).unwrap(),
            identity
        );
        std::fs::write(&moved, b"changed after move").unwrap();
        worker.send(Operation::UndoMove { redo: false }, 0).unwrap();
        assert!(finished(&rx).result.is_err());
        assert_eq!(std::fs::read(&moved).unwrap(), b"changed after move");
        assert!(!original.exists());
        std::fs::rename(&moved, root.join("parked.txt")).unwrap();
        std::fs::write(&moved, b"original move bytes").unwrap();
        worker.send(Operation::UndoMove { redo: false }, 0).unwrap();
        assert!(finished(&rx).result.is_err());
        assert_eq!(std::fs::read(&moved).unwrap(), b"original move bytes");
        assert_eq!(
            std::fs::read(root.join("parked.txt")).unwrap(),
            b"changed after move"
        );
    }
    #[test]
    fn multi_file_undo_is_not_partial_and_a_directory_move_can_be_reversed() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        let destination = root.join("destination");
        std::fs::create_dir(&destination).unwrap();
        let a = root.join("a.txt");
        let b = root.join("b.txt");
        std::fs::write(&a, b"a").unwrap();
        std::fs::write(&b, b"b").unwrap();
        let (worker, rx) = Worker::start_with_history(root.join("history.jsonl"));
        worker
            .send(
                Operation::Move {
                    paths: vec![a, b],
                    folder: destination.clone(),
                },
                0,
            )
            .unwrap();
        let reply = finished(&rx);
        reply.result.unwrap();
        assert!(
            reply
                .history
                .unwrap()
                .unwrap()
                .move_plan(false, None)
                .is_none()
        );
        let folder = root.join("folder");
        std::fs::create_dir(&folder).unwrap();
        std::fs::write(folder.join("child.txt"), b"child").unwrap();
        worker
            .send(
                Operation::Move {
                    paths: vec![folder.clone()],
                    folder: destination.clone(),
                },
                0,
            )
            .unwrap();
        let reply = finished(&rx);
        reply.result.unwrap();
        assert!(
            reply
                .history
                .unwrap()
                .unwrap()
                .move_plan(false, None)
                .is_some()
        );
        worker.send(Operation::UndoMove { redo: false }, 0).unwrap();
        finished(&rx).result.unwrap();
        assert_eq!(std::fs::read(folder.join("child.txt")).unwrap(), b"child");
        assert!(!destination.join("folder").exists());
    }
    #[test]
    fn batch_rename_worker_restart_undo_redo_and_changed_source_guard() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        let a = root.join("a.txt");
        let b = root.join("b.txt");
        std::fs::write(&a, b"a").unwrap();
        std::fs::write(&b, b"b").unwrap();
        let history = root.join("history.jsonl");
        let (worker, rx) = Worker::start_with_history(history.clone());
        worker
            .send(
                Operation::BatchRename {
                    entries: vec![(a.clone(), "b.txt".into()), (b.clone(), "a.txt".into())],
                },
                0,
            )
            .unwrap();
        let reply = finished(&rx);
        reply.result.unwrap();
        assert!(
            reply
                .history
                .unwrap()
                .unwrap()
                .batch_rename_plan(false, None)
                .is_some()
        );
        drop(worker);
        drop(rx);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            match crate::operation_journal::Journal::open(&history) {
                Ok(journal) => {
                    assert!(journal.snapshot.batch_rename_plan(false, None).is_some());
                    break;
                }
                Err(error) => {
                    assert!(std::time::Instant::now() < deadline, "{error}");
                    std::thread::sleep(std::time::Duration::from_millis(10));
                }
            }
        }
        let (worker, rx) = Worker::start_with_history(history);
        std::fs::write(&b, b"changed").unwrap();
        worker
            .send(Operation::UndoBatchRename { redo: false }, 0)
            .unwrap();
        assert!(finished(&rx).result.is_err());
        assert_eq!(std::fs::read(&a).unwrap(), b"b");
        assert_eq!(std::fs::read(&b).unwrap(), b"changed");
        std::fs::write(&b, b"a").unwrap();
        worker
            .send(Operation::UndoBatchRename { redo: false }, 0)
            .unwrap();
        finished(&rx).result.unwrap();
        assert_eq!(std::fs::read(&a).unwrap(), b"a");
        assert_eq!(std::fs::read(&b).unwrap(), b"b");
        worker
            .send(Operation::UndoBatchRename { redo: true }, 0)
            .unwrap();
        finished(&rx).result.unwrap();
        assert_eq!(std::fs::read(&a).unwrap(), b"b");
        assert_eq!(std::fs::read(&b).unwrap(), b"a");
    }
    #[test]
    fn grouping_refuses_collisions_and_round_trips_selection_without_deleting_new_data() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        let a = root.join("a.txt");
        let b = root.join("b.txt");
        std::fs::write(&a, b"a").unwrap();
        std::fs::write(&b, b"b").unwrap();
        let (worker, rx) = Worker::start_with_history(root.join("history.jsonl"));
        let group = |name: &str, paths| Operation::GroupSelection {
            paths,
            folder: root.into(),
            name: name.into(),
        };
        std::fs::create_dir(root.join("taken")).unwrap();
        worker
            .send(group("taken", vec![a.clone(), b.clone()]), 0)
            .unwrap();
        assert!(finished(&rx).result.is_err());
        assert!(a.exists() && b.exists());
        worker
            .send(group("group", vec![a.clone(), root.join("missing.txt")]), 0)
            .unwrap();
        assert!(finished(&rx).result.is_err());
        assert!(a.exists() && !root.join("group").exists());
        worker
            .send(group("group", vec![a.clone(), b.clone()]), 0)
            .unwrap();
        let reply = finished(&rx);
        reply.result.unwrap();
        assert_eq!(
            reply
                .history
                .unwrap()
                .unwrap()
                .move_plans(false, None)
                .unwrap()
                .len(),
            2
        );
        std::fs::write(root.join("group/new.txt"), b"keep this").unwrap();
        worker.send(Operation::UndoMove { redo: false }, 0).unwrap();
        finished(&rx).result.unwrap();
        assert_eq!(std::fs::read(&a).unwrap(), b"a");
        assert_eq!(std::fs::read(&b).unwrap(), b"b");
        assert_eq!(
            std::fs::read(root.join("group/new.txt")).unwrap(),
            b"keep this"
        );
        worker.send(Operation::UndoMove { redo: true }, 0).unwrap();
        finished(&rx).result.unwrap();
        assert!(!a.exists() && !b.exists());
        assert_eq!(std::fs::read(root.join("group/a.txt")).unwrap(), b"a");
        assert_eq!(std::fs::read(root.join("group/b.txt")).unwrap(), b"b");
    }
    #[test]
    fn organize_keeps_both_skips_changed_files_and_undoes_after_restart() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("Downloads");
        std::fs::create_dir(&root).unwrap();
        let history = temp.path().join("history.jsonl");
        std::fs::write(root.join("a.pdf"), b"new a").unwrap();
        std::fs::write(root.join("b.png"), b"b").unwrap();
        std::fs::write(root.join("c.pdf"), b"c").unwrap();
        std::fs::write(root.join("keep.bin"), b"k").unwrap();
        std::fs::create_dir(root.join("Documents")).unwrap();
        std::fs::write(root.join("Documents/a.pdf"), b"old a").unwrap();
        let plan = crate::organize::propose(&root).unwrap();
        assert_eq!(plan.files(), 3);
        assert_eq!(plan.unplaced, 1);
        // c.pdf changes after the review, so it must stay where it is.
        std::fs::write(root.join("c.pdf"), b"changed after review").unwrap();
        let op = Operation::Organize {
            folder: root.clone(),
            groups: plan
                .groups
                .iter()
                .map(|g| (g.name.clone(), g.items.clone()))
                .collect(),
        };
        {
            let (worker, rx) = Worker::start_with_history(history.clone());
            worker.send(op, 0).unwrap();
            finished(&rx).result.unwrap();
        }
        assert_eq!(
            std::fs::read(root.join("Documents/a.pdf")).unwrap(),
            b"old a"
        );
        assert_eq!(
            std::fs::read(root.join("Documents/a (2).pdf")).unwrap(),
            b"new a"
        );
        assert_eq!(std::fs::read(root.join("Images/b.png")).unwrap(), b"b");
        assert_eq!(
            std::fs::read(root.join("c.pdf")).unwrap(),
            b"changed after review"
        );
        assert!(root.join("keep.bin").exists());
        // A new worker reads the saved journal, as after a restart.
        let (worker, rx) = Worker::start_with_history(history);
        worker.send(Operation::History, 0).unwrap();
        let plans = finished(&rx)
            .history
            .unwrap()
            .unwrap()
            .move_plans(false, None)
            .unwrap();
        assert_eq!(plans.len(), 2);
        worker.send(Operation::UndoMove { redo: false }, 0).unwrap();
        finished(&rx).result.unwrap();
        assert_eq!(std::fs::read(root.join("a.pdf")).unwrap(), b"new a");
        assert_eq!(std::fs::read(root.join("b.png")).unwrap(), b"b");
        assert_eq!(
            std::fs::read(root.join("Documents/a.pdf")).unwrap(),
            b"old a"
        );
        assert!(!root.join("Documents/a (2).pdf").exists());
        // The Images folder Organize made is gone; Documents existed and stays.
        assert!(!root.join("Images").exists());
        assert!(root.join("Documents").is_dir());
        worker.send(Operation::UndoMove { redo: true }, 0).unwrap();
        finished(&rx).result.unwrap();
        assert_eq!(std::fs::read(root.join("Images/b.png")).unwrap(), b"b");
        assert_eq!(
            std::fs::read(root.join("Documents/a (2).pdf")).unwrap(),
            b"new a"
        );
    }
    #[test]
    fn organize_by_date_makes_year_and_month_and_undo_removes_both() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("Camera");
        std::fs::create_dir(&root).unwrap();
        let history = temp.path().join("history.jsonl");
        std::fs::write(root.join("IMG_0001.jpg"), b"\xff\xd8\xff\xd9").unwrap();
        let plan =
            crate::organize::propose_by(&root, crate::organize::Mode::Date).unwrap();
        assert_eq!(plan.groups.len(), 1);
        let nested = plan.groups[0].name.clone();
        assert!(nested.contains('\\'), "{nested}");
        let op = Operation::Organize {
            folder: root.clone(),
            groups: plan
                .groups
                .iter()
                .map(|g| (g.name.clone(), g.items.clone()))
                .collect(),
        };
        let (worker, rx) = Worker::start_with_history(history);
        worker.send(op, 0).unwrap();
        finished(&rx).result.unwrap();
        assert!(root.join(&nested).join("IMG_0001.jpg").is_file());
        worker.send(Operation::UndoMove { redo: false }, 0).unwrap();
        finished(&rx).result.unwrap();
        assert!(root.join("IMG_0001.jpg").is_file());
        // Both the month and the year folder Organize made are gone.
        let year = nested.split('\\').next().unwrap();
        assert!(!root.join(year).exists());
        worker.send(Operation::UndoMove { redo: true }, 0).unwrap();
        finished(&rx).result.unwrap();
        assert!(root.join(&nested).join("IMG_0001.jpg").is_file());
    }
    #[test]
    fn organize_refuses_a_group_folder_that_is_a_file() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("Downloads");
        std::fs::create_dir(&root).unwrap();
        std::fs::write(root.join("a.png"), b"a").unwrap();
        let plan = crate::organize::propose(&root).unwrap();
        // "Images" appears as a file between review and run.
        std::fs::write(root.join("Images"), b"not a folder").unwrap();
        let (worker, rx) = Worker::start_with_history(temp.path().join("history.jsonl"));
        worker
            .send(
                Operation::Organize {
                    folder: root.clone(),
                    groups: plan
                        .groups
                        .iter()
                        .map(|g| (g.name.clone(), g.items.clone()))
                        .collect(),
                },
                0,
            )
            .unwrap();
        finished(&rx).result.unwrap();
        assert_eq!(std::fs::read(root.join("a.png")).unwrap(), b"a");
        assert_eq!(std::fs::read(root.join("Images")).unwrap(), b"not a folder");
    }
    #[test]
    fn bulk_move_checks_all_roots_before_undo_and_supports_redo() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        let destination = root.join("destination");
        std::fs::create_dir(&destination).unwrap();
        let a = root.join("a.txt");
        let b = root.join("b.txt");
        std::fs::write(&a, b"a").unwrap();
        std::fs::write(&b, b"b").unwrap();
        let (worker, rx) = Worker::start_with_history(root.join("history.jsonl"));
        worker
            .send(
                Operation::Move {
                    paths: vec![a.clone(), b.clone()],
                    folder: destination.clone(),
                },
                0,
            )
            .unwrap();
        let reply = finished(&rx);
        reply.result.unwrap();
        assert_eq!(
            reply
                .history
                .unwrap()
                .unwrap()
                .move_plans(false, None)
                .unwrap()
                .len(),
            2
        );
        // A collision on the last root must leave the first root untouched too.
        std::fs::write(&b, b"collision").unwrap();
        worker.send(Operation::UndoMove { redo: false }, 0).unwrap();
        assert!(finished(&rx).result.is_err());
        assert!(!a.exists());
        assert_eq!(std::fs::read(destination.join("a.txt")).unwrap(), b"a");
        assert_eq!(std::fs::read(&b).unwrap(), b"collision");
        std::fs::remove_file(&b).unwrap();
        // A changed last root likewise prevents partial undo.
        std::fs::write(destination.join("b.txt"), b"changed").unwrap();
        worker.send(Operation::UndoMove { redo: false }, 0).unwrap();
        assert!(finished(&rx).result.is_err());
        assert!(!a.exists());
        std::fs::write(destination.join("b.txt"), b"b").unwrap();
        worker.send(Operation::UndoMove { redo: false }, 0).unwrap();
        finished(&rx).result.unwrap();
        assert_eq!(std::fs::read(&a).unwrap(), b"a");
        assert_eq!(std::fs::read(&b).unwrap(), b"b");
        worker.send(Operation::UndoMove { redo: true }, 0).unwrap();
        finished(&rx).result.unwrap();
        assert!(!a.exists() && !b.exists());
        assert_eq!(std::fs::read(destination.join("a.txt")).unwrap(), b"a");
        assert_eq!(std::fs::read(destination.join("b.txt")).unwrap(), b"b");
    }
    #[test]
    fn move_undo_survives_restart_and_refuses_a_missing_original_parent() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        let source_folder = root.join("source");
        let destination = root.join("destination");
        std::fs::create_dir(&source_folder).unwrap();
        std::fs::create_dir(&destination).unwrap();
        let original = source_folder.join("resume.txt");
        let moved = destination.join("resume.txt");
        std::fs::write(&original, b"restart move bytes").unwrap();
        let history = root.join("history.jsonl");
        let (worker, rx) = Worker::start_with_history(history.clone());
        worker
            .send(
                Operation::Move {
                    paths: vec![original.clone()],
                    folder: destination,
                },
                0,
            )
            .unwrap();
        finished(&rx).result.unwrap();
        drop(worker);
        drop(rx);
        let until = std::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            match crate::operation_journal::Journal::open(&history) {
                Ok(journal) => {
                    assert!(journal.snapshot.move_plan(false, None).is_some());
                    break;
                }
                Err(error) => {
                    assert!(std::time::Instant::now() < until, "{error}");
                    std::thread::sleep(std::time::Duration::from_millis(10));
                }
            }
        }
        let (worker, rx) = Worker::start_with_history(history);
        std::fs::rename(&source_folder, root.join("parked-folder")).unwrap();
        worker.send(Operation::UndoMove { redo: false }, 0).unwrap();
        assert!(finished(&rx).result.is_err());
        assert_eq!(std::fs::read(&moved).unwrap(), b"restart move bytes");
        assert!(!original.exists());
        std::fs::rename(root.join("parked-folder"), &source_folder).unwrap();
        worker.send(Operation::UndoMove { redo: false }, 0).unwrap();
        let reply = finished(&rx);
        assert!(
            reply.result.is_ok(),
            "reply={:?}; history={:?}",
            reply.result,
            reply.history
        );
        assert!(
            original.exists(),
            "original missing after retry: {:?}; directory {:?}",
            root,
            std::fs::read_dir(root)
                .unwrap()
                .map(|e| e.unwrap().path())
                .collect::<Vec<_>>()
        );
        assert_eq!(std::fs::read(&original).unwrap(), b"restart move bytes");
        assert!(!moved.exists());
    }
    #[test]
    fn put_back_restores_the_recorded_item_and_removes_it_from_the_bin() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        let original = root.join("Restore this exact item.txt");
        std::fs::write(&original, b"owned recycle round trip").unwrap();
        let (worker, rx) = Worker::start_with_history(root.join("history.jsonl"));
        worker
            .send(
                Operation::Recycle {
                    paths: vec![original.clone()],
                },
                0,
            )
            .unwrap();
        let reply = finished(&rx);
        reply.result.unwrap();
        let history = reply.history.unwrap().unwrap();
        let job = history
            .latest_put_back()
            .expect("recycle should be restorable");
        let plan = history.put_back_plan(job).unwrap();
        assert_eq!(plan.len(), 1);
        assert_eq!(plan[0].original, original);
        let recycled = plan[0].recycled.clone();
        assert!(recycled.exists());
        worker.send(Operation::PutBack { job }, 0).unwrap();
        let reply = finished(&rx);
        assert!(
            reply.result.is_ok(),
            "result={:?}; history={:?}",
            reply.result,
            reply.history
        );
        assert_eq!(
            std::fs::read(&original).unwrap(),
            b"owned recycle round trip"
        );
        assert!(!recycled.exists());
        unsafe {
            CoInitializeEx(None, COINIT_APARTMENTTHREADED).ok().unwrap();
        }
        let remains = recycled_sources(&[recycled]);
        unsafe {
            CoUninitialize();
        }
        assert!(
            remains.unwrap().is_empty(),
            "The restored object must no longer be offered by the Recycle Bin namespace"
        );
        let history = reply.history.unwrap().unwrap();
        assert!(history.put_back_plan(job).is_none());
        assert!(history.recycle_undo_job().is_none());
        worker.send(Operation::PutBack { job }, 0).unwrap();
        assert!(finished(&rx).result.is_err());
        assert_eq!(
            std::fs::read(original).unwrap(),
            b"owned recycle round trip"
        );
    }

    #[test]
    fn put_back_distinguishes_identical_original_names_and_refuses_collision() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        let original = root.join("same original name.txt");
        let (worker, rx) = Worker::start_with_history(root.join("history.jsonl"));
        std::fs::write(&original, b"first recycled version").unwrap();
        worker
            .send(
                Operation::Recycle {
                    paths: vec![original.clone()],
                },
                0,
            )
            .unwrap();
        let reply = finished(&rx);
        reply.result.unwrap();
        let history = reply.history.unwrap().unwrap();
        let first = history.latest_put_back().unwrap();
        let first_path = history.put_back_plan(first).unwrap()[0].recycled.clone();
        std::fs::write(&original, b"second recycled version").unwrap();
        worker
            .send(
                Operation::Recycle {
                    paths: vec![original.clone()],
                },
                0,
            )
            .unwrap();
        let reply = finished(&rx);
        reply.result.unwrap();
        let history = reply.history.unwrap().unwrap();
        let second = history.latest_put_back().unwrap();
        let second_path = history.put_back_plan(second).unwrap()[0].recycled.clone();
        assert_ne!(first_path, second_path);
        worker.send(Operation::PutBack { job: first }, 0).unwrap();
        finished(&rx).result.unwrap();
        assert_eq!(std::fs::read(&original).unwrap(), b"first recycled version");
        assert_eq!(
            std::fs::read(&second_path).unwrap(),
            b"second recycled version"
        );
        worker.send(Operation::PutBack { job: second }, 0).unwrap();
        let reply = finished(&rx);
        assert!(reply.result.unwrap_err().contains("already exists"));
        assert_eq!(std::fs::read(&original).unwrap(), b"first recycled version");
        assert_eq!(
            std::fs::read(&second_path).unwrap(),
            b"second recycled version"
        );
        std::fs::rename(&original, root.join("first restored version.txt")).unwrap();
        worker.send(Operation::PutBack { job: second }, 0).unwrap();
        finished(&rx).result.unwrap();
        assert_eq!(
            std::fs::read(&original).unwrap(),
            b"second recycled version"
        );
        assert!(!second_path.exists());
    }
    #[test]
    fn put_back_batch_preflight_preserves_every_item_when_one_target_conflicts() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        let a = root.join("a.txt");
        let b = root.join("b.txt");
        std::fs::write(&a, b"a original").unwrap();
        std::fs::write(&b, b"b original").unwrap();
        let (worker, rx) = Worker::start_with_history(root.join("history.jsonl"));
        worker
            .send(
                Operation::Recycle {
                    paths: vec![a.clone(), b.clone()],
                },
                0,
            )
            .unwrap();
        let reply = finished(&rx);
        reply.result.unwrap();
        let history = reply.history.unwrap().unwrap();
        let job = history.latest_put_back().unwrap();
        let plan = history.put_back_plan(job).unwrap();
        assert_eq!(plan.len(), 2);
        std::fs::write(&b, b"new target").unwrap();
        worker.send(Operation::PutBack { job }, 0).unwrap();
        assert!(finished(&rx).result.is_err());
        assert!(!a.exists());
        assert_eq!(std::fs::read(&b).unwrap(), b"new target");
        for entry in &plan {
            assert!(entry.recycled.exists());
        }
        std::fs::rename(&b, root.join("preserved collision.txt")).unwrap();
        worker.send(Operation::PutBack { job }, 0).unwrap();
        finished(&rx).result.unwrap();
        assert_eq!(std::fs::read(&a).unwrap(), b"a original");
        assert_eq!(std::fs::read(&b).unwrap(), b"b original");
        for entry in &plan {
            assert!(!entry.recycled.exists());
        }
    }
    #[test]
    fn put_back_survives_restart_and_refuses_missing_parent_and_changed_content() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        let folder = root.join("original parent");
        std::fs::create_dir(&folder).unwrap();
        let original = folder.join("resume.txt");
        std::fs::write(&original, b"recycle restart").unwrap();
        let history_path = root.join("history.jsonl");
        let (worker, rx) = Worker::start_with_history(history_path.clone());
        worker
            .send(
                Operation::Recycle {
                    paths: vec![original.clone()],
                },
                0,
            )
            .unwrap();
        let reply = finished(&rx);
        reply.result.unwrap();
        let history = reply.history.unwrap().unwrap();
        let job = history.latest_put_back().unwrap();
        let recycled = history.put_back_plan(job).unwrap()[0].recycled.clone();
        drop(worker);
        drop(rx);
        let until = std::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            match crate::operation_journal::Journal::open(&history_path) {
                Ok(journal) => {
                    assert!(journal.snapshot.put_back_plan(job).is_some());
                    break;
                }
                Err(error) => {
                    assert!(std::time::Instant::now() < until, "{error}");
                    std::thread::sleep(std::time::Duration::from_millis(10));
                }
            }
        }
        let (worker, rx) = Worker::start_with_history(history_path);
        std::fs::rename(&folder, root.join("parked parent")).unwrap();
        worker.send(Operation::PutBack { job }, 0).unwrap();
        assert!(finished(&rx).result.is_err());
        assert!(recycled.exists());
        std::fs::rename(root.join("parked parent"), &folder).unwrap();
        std::fs::write(&recycled, b"changed recycled bytes").unwrap();
        worker.send(Operation::PutBack { job }, 0).unwrap();
        assert!(finished(&rx).result.unwrap_err().contains("changed"));
        assert!(!original.exists());
        assert_eq!(std::fs::read(&recycled).unwrap(), b"changed recycled bytes");
        std::fs::write(&recycled, b"recycle restart").unwrap();
        worker.send(Operation::PutBack { job }, 0).unwrap();
        finished(&rx).result.unwrap();
        assert_eq!(std::fs::read(&original).unwrap(), b"recycle restart");
        assert!(!recycled.exists());
    }
    #[test]
    fn history_archive_runs_on_the_worker_without_mutating_user_files() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        let original = root.join("original.txt");
        let renamed = root.join("renamed.txt");
        std::fs::write(&original, b"preserved user file").unwrap();
        let journal_path = root.join("history.jsonl");
        let (worker, rx) = Worker::start_with_history(journal_path.clone());
        worker
            .send(
                Operation::Rename {
                    path: original.clone(),
                    name: "renamed.txt".into(),
                },
                0,
            )
            .unwrap();
        let reply = finished(&rx);
        reply.result.unwrap();
        assert!(
            reply
                .history
                .unwrap()
                .unwrap()
                .rename_plan(false, None)
                .is_some()
        );
        let evidence = std::fs::read(&journal_path).unwrap();
        worker.send(Operation::ArchiveHistory, 0).unwrap();
        let reply = finished(&rx);
        reply.result.unwrap();
        let snapshot = reply.history.unwrap().unwrap();
        assert!(snapshot.jobs.is_empty());
        assert_eq!(snapshot.archived_file, Some(journal_path.clone()));
        assert!(snapshot.rename_plan(false, None).is_none());
        assert!(std::fs::read(&journal_path).unwrap().starts_with(&evidence));
        assert_eq!(std::fs::read(&renamed).unwrap(), b"preserved user file");
        assert!(!original.exists());
        worker
            .send(
                Operation::Rename {
                    path: renamed.clone(),
                    name: "new history.txt".into(),
                },
                0,
            )
            .unwrap();
        let reply = finished(&rx);
        reply.result.unwrap();
        let snapshot = reply.history.unwrap().unwrap();
        assert_eq!(snapshot.jobs.len(), 1);
        assert_eq!(snapshot.jobs[0].id, 2);
        worker
            .send(Operation::UndoRename { redo: false }, 0)
            .unwrap();
        finished(&rx).result.unwrap();
        assert_eq!(std::fs::read(&renamed).unwrap(), b"preserved user file");
        worker.send(Operation::History, 0).unwrap();
        let reply = finished(&rx);
        reply.result.unwrap();
        assert_eq!(reply.history.unwrap().unwrap().jobs.len(), 2);
        assert!(std::fs::read(&journal_path).unwrap().starts_with(&evidence));
    }
    #[test]
    fn directory_rename_undo_refuses_changed_children_and_preserves_streams() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        let original = root.join("folder");
        std::fs::create_dir(&original).unwrap();
        let child = original.join("child.txt");
        std::fs::write(&child, b"original").unwrap();
        std::fs::write(format!("{}:notes", child.display()), b"named data").unwrap();
        let (worker, rx) = Worker::start_with_history(root.join("history.jsonl"));
        worker
            .send(
                Operation::Rename {
                    path: original.clone(),
                    name: "renamed".into(),
                },
                0,
            )
            .unwrap();
        let reply = finished(&rx);
        reply.result.unwrap();
        assert!(
            reply
                .history
                .unwrap()
                .unwrap()
                .rename_plan(false, None)
                .is_some()
        );
        let renamed = root.join("renamed");
        std::fs::write(renamed.join("child.txt"), b"modified").unwrap();
        worker
            .send(Operation::UndoRename { redo: false }, 0)
            .unwrap();
        assert!(finished(&rx).result.unwrap_err().contains("changed"));
        assert!(!original.exists());
        assert_eq!(
            std::fs::read(renamed.join("child.txt")).unwrap(),
            b"modified"
        );
        std::fs::write(renamed.join("child.txt"), b"original").unwrap();
        worker
            .send(Operation::UndoRename { redo: false }, 0)
            .unwrap();
        finished(&rx).result.unwrap();
        assert_eq!(
            std::fs::read(format!("{}:notes", original.join("child.txt").display())).unwrap(),
            b"named data"
        );
        worker
            .send(Operation::UndoRename { redo: true }, 0)
            .unwrap();
        finished(&rx).result.unwrap();
        assert!(renamed.is_dir());
    }
    #[test]
    fn directory_and_large_file_put_back_preserves_nested_data() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        let folder = root.join("folder");
        std::fs::create_dir(&folder).unwrap();
        std::fs::create_dir(folder.join("nested")).unwrap();
        let data = folder.join("nested/large.bin");
        std::fs::File::create(&data)
            .unwrap()
            .set_len(17 * 1024 * 1024)
            .unwrap();
        std::fs::write(format!("{}:notes", data.display()), b"nested stream").unwrap();
        let before = crate::operation_journal::Fingerprint::at(&folder).unwrap();
        let (worker, rx) = Worker::start_with_history(root.join("history.jsonl"));
        worker
            .send(
                Operation::Recycle {
                    paths: vec![folder.clone()],
                },
                0,
            )
            .unwrap();
        let reply = finished(&rx);
        reply.result.unwrap();
        let snapshot = reply.history.unwrap().unwrap();
        let job = snapshot
            .latest_put_back()
            .expect("directory recycle must retain complete evidence");
        worker.send(Operation::PutBack { job }, 0).unwrap();
        let reply = finished(&rx);
        assert!(reply.result.is_ok(), "{:?}", reply.result);
        assert_eq!(
            crate::operation_journal::Fingerprint::at(&folder).unwrap(),
            before
        );
        assert_eq!(
            std::fs::read(format!("{}:notes", data.display())).unwrap(),
            b"nested stream"
        );
    }
    #[test]
    fn queued_cancellation_stops_before_mutation_and_next_request_can_run() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        let original = root.join("original.txt");
        std::fs::write(&original, b"unchanged").unwrap();
        let (worker, rx) = Worker::start_with_history(root.join("history.jsonl"));
        let token = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        worker
            .tokens
            .lock()
            .unwrap()
            .push(std::sync::Arc::downgrade(&token));
        worker
            .tx
            .as_ref()
            .unwrap()
            .try_send((
                Operation::Rename {
                    path: original.clone(),
                    name: "cancelled.txt".into(),
                },
                0,
                token,
            ))
            .unwrap();
        worker.cancel();
        unsafe {
            windows_sys::Win32::UI::WindowsAndMessaging::PostThreadMessageW(
                worker.thread,
                FILE_OPERATION_MESSAGE,
                0,
                0,
            );
        }
        let reply = finished(&rx);
        assert!(reply.result.unwrap_err().contains("cancelled"));
        assert!(reply.history.unwrap().unwrap().jobs.is_empty());
        assert!(original.exists());
        assert!(!root.join("cancelled.txt").exists());
        worker
            .send(
                Operation::Rename {
                    path: original,
                    name: "completed.txt".into(),
                },
                0,
            )
            .unwrap();
        finished(&rx).result.unwrap();
        assert_eq!(
            std::fs::read(root.join("completed.txt")).unwrap(),
            b"unchanged"
        );
    }
    #[test]
    fn copy_undo_redo_restores_the_exact_version_without_recopying_changed_source() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        let source = root.join("source.txt");
        let dest = root.join("destination");
        std::fs::create_dir(&dest).unwrap();
        std::fs::write(&source, b"copied version").unwrap();
        std::fs::write(format!("{}:notes", source.display()), b"copied stream").unwrap();
        let (worker, rx) = Worker::start_with_history(root.join("history.jsonl"));
        worker
            .send(
                Operation::Copy {
                    paths: vec![source.clone()],
                    folder: dest.clone(),
                },
                0,
            )
            .unwrap();
        let reply = finished(&rx);
        reply.result.unwrap();
        let snapshot = reply.history.unwrap().unwrap();
        assert!(snapshot.copy_undo_plan(None).is_some(), "{snapshot:?}");
        let copied = dest.join("source.txt");
        let evidence = crate::operation_journal::Fingerprint::at(&copied).unwrap();
        worker.send(Operation::UndoCopy { redo: false }, 0).unwrap();
        let reply = finished(&rx);
        reply.result.unwrap();
        assert!(!copied.exists());
        assert!(
            reply
                .history
                .unwrap()
                .unwrap()
                .copy_redo_job(None)
                .is_some()
        );
        std::fs::write(&source, b"changed source").unwrap();
        worker.send(Operation::UndoCopy { redo: true }, 0).unwrap();
        let reply = finished(&rx);
        assert!(reply.result.is_ok(), "{:?}", reply.result);
        assert_eq!(
            crate::operation_journal::Fingerprint::at(&copied).unwrap(),
            evidence
        );
        assert_eq!(std::fs::read(&source).unwrap(), b"changed source");
        assert_eq!(
            std::fs::read(format!("{}:notes", copied.display())).unwrap(),
            b"copied stream"
        );
        assert!(
            reply
                .history
                .unwrap()
                .unwrap()
                .copy_undo_plan(None)
                .is_some()
        );
        std::fs::write(&copied, b"user modified copy").unwrap();
        worker.send(Operation::UndoCopy { redo: false }, 0).unwrap();
        assert!(finished(&rx).result.unwrap_err().contains("changed"));
        assert_eq!(std::fs::read(copied).unwrap(), b"user modified copy");
    }
    #[test]
    fn copied_directories_and_multiple_roots_undo_without_touching_originals() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        let folder = root.join("folder");
        let dest = root.join("destination");
        std::fs::create_dir(&folder).unwrap();
        std::fs::create_dir(&dest).unwrap();
        std::fs::create_dir(folder.join("nested")).unwrap();
        std::fs::write(folder.join("nested/child.txt"), b"child bytes").unwrap();
        let separate = root.join("separate.txt");
        std::fs::write(&separate, b"separate bytes").unwrap();
        let (worker, rx) = Worker::start_with_history(root.join("history.jsonl"));
        worker
            .send(
                Operation::Copy {
                    paths: vec![folder.clone(), separate.clone()],
                    folder: dest.clone(),
                },
                0,
            )
            .unwrap();
        let reply = finished(&rx);
        reply.result.unwrap();
        let snapshot = reply.history.unwrap().unwrap();
        let plan = snapshot
            .copy_undo_plan(None)
            .unwrap_or_else(|| panic!("complete directory copy proof: {snapshot:?}"));
        assert_eq!(plan.items.len(), 2);
        worker.send(Operation::UndoCopy { redo: false }, 0).unwrap();
        let reply = finished(&rx);
        reply.result.unwrap();
        assert!(!dest.join("folder").exists());
        assert!(!dest.join("separate.txt").exists());
        assert_eq!(
            std::fs::read(folder.join("nested/child.txt")).unwrap(),
            b"child bytes"
        );
        assert!(separate.exists());
        worker.send(Operation::UndoCopy { redo: true }, 0).unwrap();
        let reply = finished(&rx);
        reply.result.unwrap();
        assert_eq!(
            std::fs::read(dest.join("folder/nested/child.txt")).unwrap(),
            b"child bytes"
        );
        assert!(dest.join("separate.txt").exists());
    }
    #[test]
    fn redo_recycling_refuses_changes_then_returns_the_same_item_to_the_bin() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        let original = root.join("recycled.txt");
        std::fs::write(&original, b"recycle original").unwrap();
        let (worker, rx) = Worker::start_with_history(root.join("history.jsonl"));
        worker
            .send(
                Operation::Recycle {
                    paths: vec![original.clone()],
                },
                0,
            )
            .unwrap();
        let reply = finished(&rx);
        reply.result.unwrap();
        let job = reply.history.unwrap().unwrap().latest_put_back().unwrap();
        worker.send(Operation::PutBack { job }, 0).unwrap();
        let reply = finished(&rx);
        reply.result.unwrap();
        assert!(
            reply
                .history
                .unwrap()
                .unwrap()
                .recycle_redo_plan(None)
                .is_some()
        );
        std::fs::write(&original, b"changed recycle").unwrap();
        worker.send(Operation::RedoRecycle, 0).unwrap();
        assert!(finished(&rx).result.is_err());
        assert!(original.exists());
        std::fs::write(&original, b"recycle original").unwrap();
        worker.send(Operation::RedoRecycle, 0).unwrap();
        let reply = finished(&rx);
        reply.result.unwrap();
        assert!(!original.exists());
        let job = reply.history.unwrap().unwrap().recycle_undo_job().unwrap();
        worker.send(Operation::PutBack { job }, 0).unwrap();
        finished(&rx).result.unwrap();
        assert_eq!(std::fs::read(original).unwrap(), b"recycle original");
    }
}
