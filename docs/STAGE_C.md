# Seek v2 Stage C

**2026-10-20 follow-up:** the remaining measured UI, memory and cold-cache checks are now completed. See [Stage C polish](STAGE_C_POLISH.md) for the verified final artifact and results. The partial acceptance and cache limitations below describe the earlier release, retained as historical evidence.

Scope: session tabs, measured speed polish and the requested application button-label alignment. No packaging or framework dependency updates. Testing uses owned temporary profiles, never the production Seek profile.

## Tabs

One index/search worker and one active result table are shared. Inactive tabs retain only their query, folder scope, view, sort, expansion paths, selection, navigation history and list/grid pixel offsets. Opening or switching tabs cancels the previous content search through its existing generation guard; stale replies cannot overwrite the active tab. Reactivating a tab queries current data rather than retaining stale result rows. Content tabs restore their viewport after the authoritative scan finishes. No tab timers or persistent background work are added.

Ctrl+T duplicates the current folder/search into a new tab. Ctrl+Enter opens the selected folder in a new tab (or duplicates the search). Native row/sidebar menus offer **Open folder in new tab**; the toolbar actions menu offers **Open search in new tab**. Ctrl+Tab and Ctrl+Shift+Tab cycle; the close button or Ctrl+W closes a tab. Closing the final tab closes the window and quits. Tabs last for the current session; no disk session history was requested or added.

## Button alignment

Remaining application text buttons use the same full-height, centered 20-DIP text line box as Settings, including the permanent-delete Cancel/red action, rename/new-folder actions, content Cancel/unreadable list and All items. Icon-only controls retain their existing layout. The compact translucent delete confirmation, focus defaults and file-operation dispatch stay intact. No vendor source is changed.

## Speed work and verification

Windows association/Favorites artwork no longer blocks initial window creation. Bundled icons display first; the same finite startup worker delivers shell artwork asynchronously. The results update path avoids allocating a set of cloned paths when there is no multiple selection, tree deduplication borrows path keys, and local-date formatting is shared within a result batch (512 timestamps maximum; nothing retained between requests). A focused 603-row cached/direct comparison verifies identical labels and the bound.

The native benchmark harness now selects input-timed frames rather than accidentally accepting a later status-only update. Its broad-query mode checks 200 correctly matching rows per query. Failed attempts are excluded from measured reports. Before: one-million-file native UI p95 atlas 17.994 ms, orbit 18.327 ms, pixel 17.365 ms (30 measured samples per query after three warmups), five warm-process launches median 317.951 ms. 48 app tests, strict all-target/all-feature Clippy, normal-feature compilation and eight targeted native debug checks pass. Verified release: five warm launches median 253.232 ms (247.177-255.715 ms), below 300 ms. Broad-query worker p95 is 2.998 / 3.544 / 3.184 ms (atlas/orbit/pixel), but native input-to-present p95 is 18.551 / 19.704 / 19.231 ms, above 16 ms. This is not a full-UI speed target pass. The input-to-apply median was 6.754 / 6.844 / 8.209 ms and apply-to-present median 6.515 / 6.986 / 7.350 ms. These boundaries do not isolate GPU/compositor time, so no rendering/driver cause is claimed. [Release UI](benchmarks/stage-c-after-ui.json), [release startup](benchmarks/stage-c-after-startup.json).

A focused cached-table experiment was rejected and removed: after tab/content/grid switching a visible Alpha row appeared in the captured tree but the native UI Automation lookup could not find it. Correctness and accessibility take priority over a speculative latency gain. The final release retains the original table rendering. No vendor or framework changes.

**Performance acceptance is partial:** warm launch and worker search are below their targets in these samples; the full native input-to-present p95 remains above 16 ms. A true cold-system-cache launch is unverified. Stage C must not be reported as passing every performance requirement. The next step is focused main-thread/frame profiling for the UI tail and a controlled cold-cache run; packaging is not approved.

True cold-system-cache launch is not claimed from first-process or copied-index measurements. This ordinary-user token has no cache-purge privilege; a controlled cold-system run remains a separate validation requirement if it cannot be performed in this session.

## Final verification and artifact

48 app tests, strict all-target/all-feature Clippy, normal-feature compile, formatting and optimized build pass. Nine final-release native checks pass using only owned temporary data: two independent searches, a 120-file scoped listing equal to independent filesystem truth, list/grid viewport restoration, content mode/query/scope, modal Cancel preserving all 240 files, inactive/active/final-tab close. Final Light/Dark screenshots were inspected at 125% scaling. No actual permanent-deletion retest was needed for label layout.

Three-tab quiet foreground idle: **20.0034696 seconds, 0 measured CPU ms, 0 redraws**, no input/focus loss. Private commit **125,759,488 bytes (119.934 MiB)**; working set **127,176,704 bytes (121.285 MiB)** in this small owned fixture. Post-search one-million-file private commit **105 MiB** (before 110.301 MiB). These are snapshots/bounded samples, not a full-drive memory or long-duration leak claim. [Native evidence](benchmarks/stage-c-interactions.json), [summary](benchmarks/stage-c-summary.json).

Verified release: **19,855,872 bytes**, SHA256 `4F9AAE190E1132C93F250B34C98226C53AA97D66906B960358BFB61F9E208D1A`. The cached-table experiment was never included in this executable; source was restored to the verified rendering path before final release checks. No packaging or production-profile test operations.

Reproduce the targeted native checks with `scripts/check-stage-c.ps1 -Executable target/x86_64-pc-windows-msvc/release/seek.exe -IdleSeconds 20`. It creates its own disposable profile; avoid input during the bounded idle sample. Broad-query measurements use `scripts/bench-window.ps1 -Executable target/x86_64-pc-windows-msvc/release/seek.exe -Files 1000000 -Runs 30 -Walker -BroadOnly`; the existing owned million-file benchmark dataset is required. Warm launches use `scripts/measure-startup.ps1` with `-Files 1000000 -Runs 5 -IdleSeconds 0 -Walker`. None of these are true cold-system-cache measurements.

Check by eye: duplicate a folder/search with Ctrl+T or the native menu, use Ctrl+Tab / Ctrl+Shift+Tab, return to a scrolled List/Icons tab, close with the cross or Ctrl+W, and inspect the centered Cancel/red-delete and rename/new-folder labels in both themes. Physical HDR/mixed-DPI remains unverified.
