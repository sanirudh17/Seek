//! Windows completion events, delivered from the file-operation STA to the UI.
#![allow(non_snake_case)]
use windows::{
    Win32::{System::Com::CoTaskMemFree, UI::Shell::*},
    core::{HRESULT, PCWSTR, Ref, Result},
};
#[windows::core::implement(IFileOperationProgressSink)]
pub struct OperationProgress {
    pub updates: async_channel::Sender<crate::file_ops::Reply>,
    pub targets: Vec<std::path::PathBuf>,
    pub journal: Option<crate::operation_journal::Context>,
    pub evidence_roots: Vec<std::path::PathBuf>,
    pub required:
        std::collections::HashMap<std::path::PathBuf, crate::operation_journal::Fingerprint>,
    pub proofs: std::sync::Mutex<Vec<DeferredProof>>,
    pub copy_targets:
        std::sync::Mutex<std::collections::HashMap<std::path::PathBuf, (std::path::PathBuf, bool)>>,
    pub move_sources: std::sync::Mutex<
        std::collections::HashMap<std::path::PathBuf, crate::operation_journal::Fingerprint>,
    >,
}
pub struct DeferredProof {
    action: String,
    source: crate::operation_journal::WirePath,
    target: Option<crate::operation_journal::WirePath>,
    before: crate::operation_journal::Fingerprint,
    target_key: Option<(u64, [u8; 16])>,
    fresh: bool,
}
fn target_key(target: Option<&IShellItem>) -> Option<(u64, [u8; 16])> {
    use std::os::windows::fs::OpenOptionsExt;
    let file = std::fs::OpenOptions::new()
        .access_mode(0x80)
        .share_mode(7)
        .custom_flags(0x02000000 | 0x00200000 | 0x00100000)
        .open(path(target)?.path())
        .ok()?;
    crate::operation_journal::Fingerprint::key(&file).ok()
}
fn display_name(item: Option<&IShellItem>, kind: SIGDN) -> Option<Vec<u16>> {
    let raw = unsafe { item?.GetDisplayName(kind) }.ok()?;
    let value = unsafe { raw.as_wide().to_vec() };
    unsafe {
        CoTaskMemFree(Some(raw.0.cast()));
    }
    Some(value)
}
pub(crate) fn path(item: Option<&IShellItem>) -> Option<crate::operation_journal::WirePath> {
    display_name(item, SIGDN_FILESYSPATH).map(crate::operation_journal::WirePath)
}
impl OperationProgress {
    fn before(
        &self,
        action: &str,
        source: Option<&IShellItem>,
        folder: Option<&IShellItem>,
        name: Option<&PCWSTR>,
    ) -> Result<()> {
        crate::operation_control::check()
            .map_err(|e| windows::core::Error::new(windows::Win32::Foundation::E_ABORT, e))?;
        if let Some(source) = path(source)
            && let Some((_, expected)) = self.required.iter().find(|(path, _)| {
                seek_engine::paths::cmp_ci(
                    &seek_engine::paths::internal(path),
                    &seek_engine::paths::internal(&source.path()),
                )
                .is_eq()
            })
            && !crate::operation_journal::Fingerprint::at(&source.path())
                .as_ref()
                .is_some_and(|current| current.compatible(expected))
        {
            return Err(windows::core::Error::new(
                windows::Win32::Foundation::E_ABORT,
                "The item changed before the operation. Its inverse was stopped.",
            ));
        }
        if matches!(action, "move" | "delete" | "copy")
            && self.journal.is_some()
            && let Some(source) = path(source)
            && self.evidence_roots.iter().any(|root| {
                seek_engine::paths::cmp_ci(
                    &seek_engine::paths::internal(root),
                    &seek_engine::paths::internal(&source.path()),
                )
                .is_eq()
            })
            && let Some(identity) = crate::operation_journal::Fingerprint::at(&source.path())
        {
            self.move_sources
                .lock()
                .map_err(|_| {
                    windows::core::Error::from_hresult(windows::Win32::Foundation::E_FAIL)
                })?
                .insert(source.path(), identity);
        }
        crate::operation_control::check()
            .map_err(|e| windows::core::Error::new(windows::Win32::Foundation::E_ABORT, e))?;
        if let Some(journal) = &self.journal {
            journal.append(crate::operation_journal::Event::Before {
                job: journal.job,
                action: action.into(),
                source: path(source),
                folder: path(folder),
                name: name
                    .filter(|name| !name.is_null())
                    .map(|name| unsafe { name.as_wide().to_vec() }),
                proof: matches!(action, "copy" | "move")
                    && path(source).is_some_and(|source| {
                        self.move_sources
                            .lock()
                            .ok()
                            .and_then(|sources| {
                                sources
                                    .get(&source.path())
                                    .map(|identity| identity.directory)
                            })
                            .unwrap_or(false)
                    }),
            })?;
        }
        Ok(())
    }
    fn after(
        &self,
        action: &str,
        source: Option<&IShellItem>,
        target: Option<&IShellItem>,
        hresult: HRESULT,
    ) -> Result<()> {
        let identity = if matches!(action, "move" | "delete" | "copy") {
            path(source)
                .and_then(|source| self.move_sources.lock().ok()?.remove(&source.path()))
                .and_then(|before| {
                    let fresh_copy = if action == "copy" {
                        let prediction = path(source).and_then(|source| {
                            self.copy_targets.lock().ok()?.remove(&source.path())
                        });
                        prediction.is_some_and(|(predicted, existed)| {
                            !existed
                                || path(target).is_some_and(|actual| {
                                    !seek_engine::paths::cmp_ci(
                                        &seek_engine::paths::internal(&predicted),
                                        &seek_engine::paths::internal(&actual.path()),
                                    )
                                    .is_eq()
                                })
                        })
                    } else {
                        true
                    };
                    if before.directory && matches!(action, "copy" | "move") {
                        let source = path(source)?;
                        self.proofs.lock().ok()?.push(DeferredProof {
                            action: action.into(),
                            source,
                            target: path(target),
                            target_key: target_key(target),
                            before,
                            fresh: fresh_copy
                                && crate::operation_journal::item_completed(hresult.0),
                        });
                        return None;
                    }
                    let after = crate::operation_journal::Fingerprint::at(&path(target)?.path())?;
                    let valid = if action == "copy" || action == "move" {
                        before.directory == after.directory
                            && before.bytes == after.bytes
                            && before.content_hash.is_some()
                            && before.content_hash == after.content_hash
                    } else {
                        before == after
                    };
                    let fresh_copy = fresh_copy
                        && (action != "copy"
                            || before.volume != after.volume
                            || before.id != after.id);
                    (crate::operation_journal::item_completed(hresult.0) && valid && fresh_copy)
                        .then_some(after)
                })
        } else {
            None
        };
        if let Some(journal) = &self.journal {
            journal.append(crate::operation_journal::Event::After {
                job: journal.job,
                action: action.into(),
                source: path(source),
                target: path(target),
                parsing_name: display_name(target, SIGDN_DESKTOPABSOLUTEPARSING),
                hresult: hresult.0,
                identity,
            })?;
        }
        Ok(())
    }
}
impl IFileOperationProgressSink_Impl for OperationProgress_Impl {
    fn StartOperations(&self) -> Result<()> {
        Ok(())
    }
    fn FinishOperations(&self, _hr: HRESULT) -> Result<()> {
        let proofs =
            std::mem::take(&mut *self.proofs.lock().map_err(|_| {
                windows::core::Error::from_hresult(windows::Win32::Foundation::E_FAIL)
            })?);
        if let Some(journal) = &self.journal {
            for proof in proofs {
                let identity = proof
                    .target
                    .as_ref()
                    .and_then(|target| crate::operation_journal::Fingerprint::at(&target.path()))
                    .filter(|after| {
                        proof.fresh
                            && Some((after.volume, after.id)) == proof.target_key
                            && after.directory == proof.before.directory
                            && after.bytes == proof.before.bytes
                            && after.content_hash.is_some()
                            && after.content_hash == proof.before.content_hash
                    });
                journal.append(crate::operation_journal::Event::Proof {
                    job: journal.job,
                    action: proof.action,
                    source: Some(proof.source),
                    target: proof.target,
                    identity,
                })?;
            }
        }
        Ok(())
    }
    fn PreRenameItem(&self, _: u32, source: Ref<IShellItem>, name: &PCWSTR) -> Result<()> {
        self.before("rename", source.as_ref(), None, Some(name))
    }
    fn PostRenameItem(
        &self,
        _: u32,
        source: Ref<IShellItem>,
        _: &PCWSTR,
        hr: HRESULT,
        target: Ref<IShellItem>,
    ) -> Result<()> {
        self.after("rename", source.as_ref(), target.as_ref(), hr)
    }
    fn PreMoveItem(
        &self,
        _: u32,
        source: Ref<IShellItem>,
        folder: Ref<IShellItem>,
        name: &PCWSTR,
    ) -> Result<()> {
        self.before("move", source.as_ref(), folder.as_ref(), Some(name))
    }
    fn PostMoveItem(
        &self,
        _: u32,
        source: Ref<IShellItem>,
        _: Ref<IShellItem>,
        _: &PCWSTR,
        hr: HRESULT,
        target: Ref<IShellItem>,
    ) -> Result<()> {
        self.after("move", source.as_ref(), target.as_ref(), hr)
    }
    fn PreCopyItem(
        &self,
        flags: u32,
        source: Ref<IShellItem>,
        folder: Ref<IShellItem>,
        name: &PCWSTR,
    ) -> Result<()> {
        if let (Some(source), Some(folder)) = (path(source.as_ref()), path(folder.as_ref())) {
            use std::os::windows::ffi::OsStringExt;
            let source = source.path();
            let name = if name.is_null() || unsafe { name.as_wide() }.is_empty() {
                source.file_name().unwrap_or_default().to_owned()
            } else {
                std::ffi::OsString::from_wide(unsafe { name.as_wide() })
            };
            let predicted = folder.path().join(name);
            let existed = flags & TSF_OVERWRITE_EXIST.0 as u32 != 0
                || !std::fs::symlink_metadata(&predicted)
                    .is_err_and(|e| e.kind() == std::io::ErrorKind::NotFound);
            if let Ok(mut targets) = self.copy_targets.lock() {
                targets.insert(source, (predicted, existed));
            }
        }
        self.before("copy", source.as_ref(), folder.as_ref(), Some(name))
    }
    fn PostCopyItem(
        &self,
        flags: u32,
        source: Ref<IShellItem>,
        _: Ref<IShellItem>,
        _: &PCWSTR,
        hr: HRESULT,
        target: Ref<IShellItem>,
    ) -> Result<()> {
        if flags & TSF_OVERWRITE_EXIST.0 as u32 != 0
            && let Some(source) = path(source.as_ref())
            && let Ok(mut copies) = self.copy_targets.lock()
        {
            copies.remove(&source.path());
        }
        self.after("copy", source.as_ref(), target.as_ref(), hr)
    }
    fn PreDeleteItem(&self, _: u32, source: Ref<IShellItem>) -> Result<()> {
        self.before("delete", source.as_ref(), None, None)
    }
    fn PostDeleteItem(
        &self,
        _: u32,
        item: Ref<IShellItem>,
        _hr: HRESULT,
        target: Ref<IShellItem>,
    ) -> Result<()> {
        self.after("delete", item.as_ref(), target.as_ref(), _hr)?;
        if _hr.is_err() {
            return Ok(());
        }
        if let Some(item) = item.as_ref() {
            use std::os::windows::ffi::OsStringExt;
            if let Ok(raw) = unsafe { item.GetDisplayName(SIGDN_FILESYSPATH) } {
                let path = unsafe {
                    std::path::PathBuf::from(std::ffi::OsString::from_wide(raw.as_wide()))
                };
                unsafe {
                    CoTaskMemFree(Some(raw.0.cast()));
                }
                let key = seek_engine::paths::internal(&path);
                if let Some(target) = self.targets.iter().find(|p| {
                    seek_engine::paths::cmp_ci(&seek_engine::paths::internal(p), &key).is_eq()
                }) && std::fs::symlink_metadata(target)
                    .is_err_and(|e| e.kind() == std::io::ErrorKind::NotFound)
                {
                    let _ = self.updates.try_send(crate::file_ops::Reply {
                        result: Ok(()),
                        refresh: vec![],
                        removed: vec![target.clone()],
                        finished: false,
                        progress: None,
                        history: None,
                    });
                }
            }
        }
        Ok(())
    }
    fn PreNewItem(&self, _: u32, folder: Ref<IShellItem>, name: &PCWSTR) -> Result<()> {
        self.before("new", None, folder.as_ref(), Some(name))
    }
    fn PostNewItem(
        &self,
        _: u32,
        _: Ref<IShellItem>,
        _: &PCWSTR,
        _: &PCWSTR,
        _: u32,
        hr: HRESULT,
        target: Ref<IShellItem>,
    ) -> Result<()> {
        self.after("new", None, target.as_ref(), hr)
    }
    fn UpdateProgress(&self, total: u32, done: u32) -> Result<()> {
        crate::operation_control::check()
            .map_err(|e| windows::core::Error::new(windows::Win32::Foundation::E_ABORT, e))?;
        let _ = self.updates.try_send(crate::file_ops::Reply {
            result: Ok(()),
            refresh: vec![],
            removed: vec![],
            finished: false,
            progress: Some((done, total)),
            history: None,
        });
        Ok(())
    }
    fn ResetTimer(&self) -> Result<()> {
        Ok(())
    }
    fn PauseTimer(&self) -> Result<()> {
        Ok(())
    }
    fn ResumeTimer(&self) -> Result<()> {
        Ok(())
    }
}
