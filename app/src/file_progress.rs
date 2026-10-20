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
}
impl IFileOperationProgressSink_Impl for OperationProgress_Impl {
    fn StartOperations(&self) -> Result<()> {
        Ok(())
    }
    fn FinishOperations(&self, _hr: HRESULT) -> Result<()> {
        Ok(())
    }
    fn PreRenameItem(&self, _: u32, _: Ref<IShellItem>, _: &PCWSTR) -> Result<()> {
        Ok(())
    }
    fn PostRenameItem(
        &self,
        _: u32,
        _: Ref<IShellItem>,
        _: &PCWSTR,
        _hr: HRESULT,
        _: Ref<IShellItem>,
    ) -> Result<()> {
        Ok(())
    }
    fn PreMoveItem(
        &self,
        _: u32,
        _: Ref<IShellItem>,
        _: Ref<IShellItem>,
        _: &PCWSTR,
    ) -> Result<()> {
        Ok(())
    }
    fn PostMoveItem(
        &self,
        _: u32,
        _: Ref<IShellItem>,
        _: Ref<IShellItem>,
        _: &PCWSTR,
        _hr: HRESULT,
        _: Ref<IShellItem>,
    ) -> Result<()> {
        Ok(())
    }
    fn PreCopyItem(
        &self,
        _: u32,
        _: Ref<IShellItem>,
        _: Ref<IShellItem>,
        _: &PCWSTR,
    ) -> Result<()> {
        Ok(())
    }
    fn PostCopyItem(
        &self,
        _: u32,
        _: Ref<IShellItem>,
        _: Ref<IShellItem>,
        _: &PCWSTR,
        _hr: HRESULT,
        _: Ref<IShellItem>,
    ) -> Result<()> {
        Ok(())
    }
    fn PreDeleteItem(&self, _: u32, _: Ref<IShellItem>) -> Result<()> {
        Ok(())
    }
    fn PostDeleteItem(
        &self,
        _: u32,
        item: Ref<IShellItem>,
        _hr: HRESULT,
        _: Ref<IShellItem>,
    ) -> Result<()> {
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
                    });
                }
            }
        }
        Ok(())
    }
    fn PreNewItem(&self, _: u32, _: Ref<IShellItem>, _: &PCWSTR) -> Result<()> {
        Ok(())
    }
    fn PostNewItem(
        &self,
        _: u32,
        _: Ref<IShellItem>,
        _: &PCWSTR,
        _: &PCWSTR,
        _: u32,
        _hr: HRESULT,
        _: Ref<IShellItem>,
    ) -> Result<()> {
        Ok(())
    }
    fn UpdateProgress(&self, total: u32, done: u32) -> Result<()> {
        let _ = self.updates.try_send(crate::file_ops::Reply {
            result: Ok(()),
            refresh: vec![],
            removed: vec![],
            finished: false,
            progress: Some((done, total)),
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
