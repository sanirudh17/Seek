//! Window-thread shell drag handoff. Internal moves remain GPUI typed drops.
//! Only COPY is offered externally; the source is never deleted by Seek.
use std::{path::PathBuf, sync::Arc};
use windows::{
    Win32::{
        Foundation::*,
        System::{
            Com::{CoTaskMemFree, IDataObject},
            Ole::*,
            SystemServices::MODIFIERKEYS_FLAGS,
        },
        UI::{
            Input::KeyboardAndMouse::{GetAsyncKeyState, VK_LBUTTON},
            Shell::*,
        },
    },
    core::{BOOL, HRESULT, PCWSTR, implement},
};

#[implement(IDropSource)]
struct Source;
impl IDropSource_Impl for Source_Impl {
    fn QueryContinueDrag(&self, escape: BOOL, keys: MODIFIERKEYS_FLAGS) -> HRESULT {
        if escape.as_bool() {
            DRAGDROP_S_CANCEL
        } else if keys.0 & 1 == 0 {
            DRAGDROP_S_DROP
        } else {
            HRESULT(0)
        }
    }
    fn GiveFeedback(&self, _: DROPEFFECT) -> HRESULT {
        DRAGDROP_S_USEDEFAULTCURSORS
    }
}
/// Called on the owning window thread, after GPUI has relinquished its typed drag.
pub fn start(paths: Arc<Vec<PathBuf>>) -> windows::core::Result<()> {
    if paths.is_empty() || unsafe { GetAsyncKeyState(VK_LBUTTON.0 as i32) } >= 0 {
        return Ok(());
    }
    unsafe {
        OleInitialize(None)?;
        struct OleGuard;
        impl Drop for OleGuard {
            fn drop(&mut self) {
                unsafe {
                    OleUninitialize();
                }
            }
        }
        let _ole = OleGuard;
        let mut ids = Vec::new();
        for path in paths.iter() {
            use std::os::windows::ffi::OsStrExt;
            let wide: Vec<_> = path.as_os_str().encode_wide().chain(Some(0)).collect();
            let mut id = std::ptr::null_mut();
            if let Err(error) = SHParseDisplayName(PCWSTR(wide.as_ptr()), None, &mut id, 0, None) {
                for id in ids {
                    CoTaskMemFree(Some(id));
                }
                return Err(error);
            }
            ids.push(id.cast());
        }
        let pointers: Vec<_> = ids
            .iter()
            .map(|p| p.cast::<Common::ITEMIDLIST>() as *const _)
            .collect();
        let array = SHCreateShellItemArrayFromIDLists(&pointers);
        for id in ids {
            CoTaskMemFree(Some(id));
        }
        let data: IDataObject = array?.BindToHandler(None, &BHID_DataObject)?;
        let source: IDropSource = Source.into();
        let mut effect = DROPEFFECT_NONE;
        #[cfg(feature = "benchmarks")]
        crate::metrics::interaction("ole_start", serde_json::json!({"files":paths.len()}));
        let result = DoDragDrop(&data, &source, DROPEFFECT_COPY, &mut effect);
        #[cfg(feature = "benchmarks")]
        crate::metrics::interaction(
            "ole_result",
            serde_json::json!({"hresult":result.0,"effect":effect.0}),
        );
        result.ok()
    }
}
