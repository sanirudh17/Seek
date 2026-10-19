# Seek drag-and-drop follow-up

Authorized 2026-10-17 after Stage B: retry external drag-out and investigate the reported internal drop doing nothing. No Stage C or packaging.

## Change

The old benchmark-only prototype polled the pointer on a separate OLE thread. The fresh before sample reached `DoDragDrop` but did not copy to Explorer or complete before test shutdown. Its failure did not establish that external dragging was impossible.

The app now keeps GPUI's typed drag for internal destinations. A root drag-move handler detects the pointer leaving the source viewport, snapshots the selected paths, ends the typed drag and releases capture on the owning window thread. A deferred callback starts the Windows OLE loop on that same thread, after the Seek entity callback has returned. Shell item PIDLs supply the shell `IDataObject`. Only Copy is offered externally; Seek never deletes a source after an external drop. Escape cancels. The transient COM initialization and PIDLs are balanced on error paths. No polling worker or timer remains, and normal builds include the feature without an experiment flag.

Internal moves still use the existing worker-thread `IFileOperation` path and its conflict dialogs. Supported internal targets are folder rows/tiles and sidebar locations. Dragging to another arbitrary position in a sorted list does not manually reorder it. This follow-up verified List folder-row and sidebar moves; the reported no-op was not reproduced on those targets in the owned fixture.

There is no GPUI, Kit or patched Windows crate update or source patch. The existing framework already exposes typed drag-move events and `stop_active_drag`, sufficient for an app-level Windows handoff.

## References

The user-supplied [Files layout implementation](https://github.com/files-community/Files/blob/main/src/Files.App/Views/Layouts/BaseLayoutPage.cs) prepares a shell ID-list payload or storage items in its UI framework's drag-start handler. That informed the choice of a shell data object; no Files source was imported. Seek uses Rust/GPUI with Win32 OLE rather than WinUI's DataPackage API.

Microsoft documents the native loop and source contract in [DoDragDrop](https://learn.microsoft.com/en-us/windows/win32/api/ole2/nf-ole2-dodragdrop) and the source-thread capture operation in [ReleaseCapture](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-releasecapture).

## Validation method and limits

`scripts/check-drag-followup.ps1` creates unique owned files and uses fresh native UIA bounds with real held-pointer input. Explorer success requires a destination file with the same SHA256. Browser checks use a separate Chromium profile and `scripts/drag-upload-fixture.py`, bound only to loopback. Both a standard `<input type="file">` and an HTML DataTransfer drop target read the actual file bytes and report SHA256. No file is uploaded to an external service. Multi-selection checks both file hashes; Escape must not deliver another drop.

These checks establish native file transfer to Explorer and Chromium's standard web file interfaces. They do not certify every website, chat app, elevated target, virtual file provider or touch/pen input. External transfer is Copy only. After handing off outside Seek, returning the pointer to Seek does not restore the original typed internal drag; cancel and start again for an internal move. Stage A's unrelated correctness and idle suites are retained and are not repeated here.

## Final results

**7/7 targeted release interaction checks pass**, plus clean exit with code 0 in all three test instances:

- List folder-row move and sidebar move: both destination/source filesystem states checked.
- Explorer external Copy: destination bytes match SHA256 and source preserved.
- Chromium native file input and HTML DataTransfer drop area: both read actual bytes matching SHA256.
- Multi-selection: both selected files received with matching hashes.
- Escape cancellation: no extra file-delivery event; native result `DRAGDROP_S_CANCEL`, effect None. Successful external drops return `DRAGDROP_S_DROP`, effect Copy.

These final runs do **not** pass `--experiment-drag-out`. [Browser raw evidence](benchmarks/v2-drag-browser-release.json), [Explorer](benchmarks/v2-drag-explorer-release.json), [internal](benchmarks/v2-drag-internal-release.json), [summary](benchmarks/v2-drag-summary.json). The browser report includes the actual file-name/size/hash records. Earlier debug and before reports retain their distinct executable hashes.

Optimized build passes in **3 min 53 s**. Release executable: **17,970,176 bytes**, SHA256 `6ABFC929F433F250F9F0C7DF240A97E6A80DAEB1353F819A0FBD886392802F88`. Strict all-target/all-feature app Clippy, normal-feature check, formatting and diff checks pass. The normal-feature check caught diagnostics calls that needed conditional compilation; fixed before this final release. An earlier optimized build could not replace the still-running prior executable; that app was closed normally and the final build succeeded. These setup/build failures are not counted as passing checks.

The app is reopened with its normal user profile after validation. No real files were moved by the test beyond the uniquely owned fixtures. No broad indexing, idle or performance suite was repeated.
