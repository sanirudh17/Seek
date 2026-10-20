# Post-release fixes: source verification, build held

2026-10-20. The user explicitly withheld permission to build the app until the fixes were ready. Only Rust test-harness executables and type-check artifacts were compiled. Both normal debug/release executable hashes remain unchanged. No installer, version change, release upload, framework update or production-profile test.

## Causes and implementation

- **Gibberish:** previews decoded non-BOM data with lossy UTF-8. Previews and plain-text content search now share strict UTF-8, UTF-16/32 BOM handling, conservative BOMless UTF-16 detection and Windows ANSI conversion. The 8-KiB preview handles a cut multi-byte character without inserting replacement glyphs. Mixed binary/control data reports that text preview is unavailable. It cannot reconstruct corrupted source text or reliably infer every BOMless encoding. The original AMD driver log from the screenshots was not available at the checked location; representative owned Unicode/binary logs were tested. PDF/Word extraction remains unchanged.
- **Suggestion position:** a fixed window coordinate remained valid only in the restored window. Suggestions now belong to the actual relative search-field container; resizing and centering use the same layout. The type-token popup retains its measured anchor and closes on resize.
- **Tabs and icons:** activation used to clear the rows and query again, while folder Search requests invalidated their metadata cache. Tabs now keep immutable shared rendered-row snapshots, preserve selection/scroll and revalidate asynchronously. Snapshots are invalidated by relevant preference/scope changes, bounded to eight views / 24 MiB estimated row storage (8 MiB each); evicted views reload. This is not a process-memory cap. Direct folder metadata reads are independent of index write locks, have at most two concurrent finite workers, and prioritize the requested folder. Revision tokens reject obsolete in-flight reads. List icons have a separate event-driven STA from large previews; folder rows use the existing shared artwork, and the small-image cache holds up to 512 entries within its unchanged 32-MiB budget. Shell/cache misses still require asynchronous loading.
- **Deletion:** the app waited for batch completion, then refreshed from an index/cache that could still contain the deleted row. Successful native item callbacks now prune active and saved views; a final filesystem check covers provider callbacks that precede removal of a directory entry. Failed/cancelled targets remain visible. Backend suppression prevents stale index entries returning during reconciliation and allows same-path recreated files. Available callbacks are coalesced before sorting/repainting. Windows progress, error and access dialogs are enabled; the recycle-only guard remains. An optional SetProgressMessage call returned E_NOTIMPL in the first test and is now nonfatal. No destructive test touched real files.
- **Selection and input:** Shift ranges use an original anchor; Ctrl+click toggles individual items, Ctrl+arrows moves only the caret, and Shift+arrows follows list/grid geometry. The last Ctrl-deselected row cannot be deleted merely because it retains the caret. Confirmed deletion preserves surviving selection, or selects the adjacent row. Permanent-delete default focus now goes to **Delete permanently**, as explicitly requested; Enter/Space confirms that focused choice, arrows/Tab toggle, Escape cancels, and captured selections are still consumed once. Rename/new-folder button arrows preserve ordinary text-field navigation. Other focused buttons use the existing focus chain; native menu arrows remain native. Empty full-window overlays are omitted; activation/resize invalidates table hit regions, and otherwise-unhandled result-pane wheel events scroll without requiring a click.
- **Motion and glass:** bounded 140-ms insertion, 120-ms selected-tab wash and 100-ms closing-slot contraction use pinned GPUI animation. Content never waits for motion. Windows client-area animation preference is respected. The supplied gpui-motion library uses a different GPUI adapter stack, so it was evaluated without adding it or updating the framework. On supported Windows 11, compositor-owned Desktop Acrylic replaces legacy accent blur; unsupported systems retain the existing fallback. No second extended frame or native caption is introduced. Actual transition appearance remains unverified.

## Measured checks

[Machine-readable measurements](benchmarks/post-release-source-fixes.json). **65 distinct Rust checks passed**: a 62-pass full app suite (one unrelated live-release network test ignored), followed by three new targeted native-material, real-file decoder and deletion/recreation tests. A selection test was rerun after its argument refactor and bounds correction. Strict all-target/all-feature Clippy, normal-feature type checking, formatting and diff checks pass. The source UI detector emitted no findings for the Rust targets; it cannot certify native rendering.

| Check | Observed result | Limit |
| --- | --- | --- |
| Owned native deletion, file + nested folder | First confirmed file 344.188 ms; batch 447.516 ms; 7 progress callbacks | Test-profile worker timing under parallel test load, not row paint or a large-operation benchmark |
| Owned 400-file folder metadata | Initial read 4.745 ms; warm access p95 4.387 ms, 100 accesses | Every access equals independent directory truth; excludes UI |
| 12,000-row snapshot handoff | p95 0.000100 ms, 100 handoffs; shared row pointer verified | Handle cloning only, excludes capture/layout/paint; not a tab latency claim |
| DWM material | Set/read/remove passed on a hidden owned native window | Does not verify maximize/restore animation |
| Deletion/recreation | Confirmed deleted rows stay absent; same-path new file becomes visible, matching independent listing | Disposable backend fixture |
| Encoding | BOMless LE/BE, UTF-8, ANSI fixtures, symbols and bounded multibyte tail pass; mixed binary rejected | Original screenshot driver log not checked |

No new normal app executable was built. Existing SHA-256:

- Debug: `992D0CC39E8E1BCB3ABBF0BB6303DD2496A7DFDE90418DCA4E3D02FE5A1F1732`.
- Release: `696AA55FB716447DA69F6184E41ED12CCDC86405B662FB718A7EA9BEEA183C79`.

## Next step after build approval

Build one test version with a separate owned profile, then perform one targeted desktop pass: restored/maximized suggestions with sidebar open/closed; tab create/switch/close with saved scroll and shell icons; glass startup/maximize/restore/taskbar transitions in Light/Dark; Shift/Ctrl selection and wheel scrolling after focus changes; dialog default/arrow/Enter/Escape/backdrop behavior; large disposable recycle/permanent-delete progress and cancel. Measure actual input-to-paint, memory and idle CPU there. These checks remain pending; source/API tests cannot close them. Do not publish or replace the installed app without authorization.

## References

- [Windows operation flags](https://learn.microsoft.com/en-us/windows/win32/api/shobjidl_core/nf-shobjidl_core-ifileoperation-setoperationflags): ordinary Shell progress/error dialogs must not be suppressed.
- [DWM system backdrop types](https://learn.microsoft.com/en-us/windows/win32/api/dwmapi/ne-dwmapi-dwm_systembackdrop_type): Desktop Acrylic system material and supported Windows versions.
- [Supplied gpui-motion source](https://github.com/1bye/gpui-motion): evaluated, no dependency added. The linked X post was unavailable to the browser.
