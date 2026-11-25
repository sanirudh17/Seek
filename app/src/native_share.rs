//! Per-window Windows Share sheet. Resolve storage items off the UI thread.
use std::path::PathBuf;
use windows::{
    ApplicationModel::DataTransfer::*,
    Foundation::TypedEventHandler,
    Storage::{IStorageItem, StorageFile, StorageFolder},
    Win32::{Foundation::HWND, UI::Shell::IDataTransferManagerInterop},
    core::{HSTRING, Interface},
};
pub struct Session {
    manager: DataTransferManager,
    token: i64,
}
impl Drop for Session {
    fn drop(&mut self) {
        let _ = self.manager.RemoveDataRequested(self.token);
    }
}
impl Session {
    pub fn show(paths: Vec<PathBuf>, owner: HWND) -> windows::core::Result<Self> {
        let interop = windows::core::factory::<DataTransferManager, IDataTransferManagerInterop>()?;
        let manager: DataTransferManager = unsafe { interop.GetForWindow(owner)? };
        let token = manager.DataRequested(&TypedEventHandler::new(
            move |_, args: windows::core::Ref<DataRequestedEventArgs>| {
                let request = args
                    .as_ref()
                    .ok_or_else(windows::core::Error::from_thread)?
                    .Request()?;
                request
                    .Data()?
                    .Properties()?
                    .SetTitle(&HSTRING::from("Share from Seek"))?;
                let deferral = request.GetDeferral()?;
                let paths = paths.clone();
                std::thread::spawn(move || {
                    let result = (|| -> windows::core::Result<()> {
                        unsafe {
                            windows::Win32::System::WinRT::RoInitialize(
                                windows::Win32::System::WinRT::RO_INIT_MULTITHREADED,
                            )?;
                        }
                        struct Runtime;
                        impl Drop for Runtime {
                            fn drop(&mut self) {
                                unsafe {
                                    windows::Win32::System::WinRT::RoUninitialize();
                                }
                            }
                        }
                        let _runtime = Runtime;
                        let mut items = Vec::<IStorageItem>::new();
                        for path in paths {
                            let meta = std::fs::symlink_metadata(&path)
                                .map_err(|_| windows::core::Error::from_thread())?;
                            let name = HSTRING::from(path.as_os_str());
                            if meta.is_dir() {
                                items.push(
                                    StorageFolder::GetFolderFromPathAsync(&name)?
                                        .join()?
                                        .cast()?,
                                );
                            } else {
                                items.push(
                                    StorageFile::GetFileFromPathAsync(&name)?.join()?.cast()?,
                                );
                            }
                        }
                        let items: windows_collections::IIterable<IStorageItem> =
                            items.into_iter().map(Some).collect::<Vec<_>>().into();
                        request.Data()?.SetStorageItems(&items, true)
                    })();
                    if let Err(error) = result {
                        let _ = request.FailWithDisplayText(&HSTRING::from(format!(
                            "Cannot prepare these files for sharing: {error}"
                        )));
                    }
                    let _ = deferral.Complete();
                });
                Ok(())
            },
        ))?;
        let session = Self { manager, token };
        unsafe {
            interop.ShowShareUIForWindow(owner)?;
        }
        Ok(session)
    }
}
