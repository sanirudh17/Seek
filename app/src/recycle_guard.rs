//! Veto a Shell delete that is not explicitly marked for recycling.
#![allow(non_snake_case)]
use windows::{
    Win32::{Foundation::E_ABORT, UI::Shell::*},
    core::{HRESULT, PCWSTR, Ref, Result},
};
#[windows::core::implement(IFileOperationProgressSink)]
pub struct RecycleGuard;
impl IFileOperationProgressSink_Impl for RecycleGuard_Impl {
    fn StartOperations(&self) -> Result<()> {
        Ok(())
    }
    fn FinishOperations(&self, hr: HRESULT) -> Result<()> {
        hr.ok()
    }
    fn PreRenameItem(&self, _: u32, _: Ref<IShellItem>, _: &PCWSTR) -> Result<()> {
        Ok(())
    }
    fn PostRenameItem(
        &self,
        _: u32,
        _: Ref<IShellItem>,
        _: &PCWSTR,
        hr: HRESULT,
        _: Ref<IShellItem>,
    ) -> Result<()> {
        hr.ok()
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
        hr: HRESULT,
        _: Ref<IShellItem>,
    ) -> Result<()> {
        hr.ok()
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
        hr: HRESULT,
        _: Ref<IShellItem>,
    ) -> Result<()> {
        hr.ok()
    }
    fn PreDeleteItem(&self, flags: u32, _: Ref<IShellItem>) -> Result<()> {
        if flags & TSF_DELETE_RECYCLE_IF_POSSIBLE.0 as u32 == 0 {
            Err(windows::core::Error::new(
                E_ABORT,
                "Move to Recycle Bin cannot permanently delete files. This item cannot be recycled.",
            ))
        } else {
            Ok(())
        }
    }
    fn PostDeleteItem(
        &self,
        _: u32,
        _: Ref<IShellItem>,
        hr: HRESULT,
        recycled: Ref<IShellItem>,
    ) -> Result<()> {
        hr.ok()?;
        if recycled.is_none() {
            Err(windows::core::Error::new(
                E_ABORT,
                "Windows did not confirm a Recycle Bin item.",
            ))
        } else {
            Ok(())
        }
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
        hr: HRESULT,
        _: Ref<IShellItem>,
    ) -> Result<()> {
        hr.ok()
    }
    fn UpdateProgress(&self, _: u32, _: u32) -> Result<()> {
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
