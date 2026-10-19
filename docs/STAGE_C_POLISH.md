# Stage C performance follow-up and preview/scroll refinement

2026-10-20 (checks run October 19-20). The measured Stage C requirements now pass on this machine and the owned one-million-file fixture. No packaging, dependency updates, vendor changes or production Seek profile testing. This follow-up also includes the user's requested slimmer rounded scrollbar and removal of the low-resolution preview flash.

## Changes and diagnosis

The full table was being rebuilt for unrelated search/chrome paints. An application-level `ResultPane` now reuses unchanged GPUI layout/paint and observes table notifications to invalidate on data, selection, scrolling and media changes. Crucially, the previous cached-table experiment failed because GPUI's cached prepaint does not replay accessibility nodes. The existing `Window::is_a11y_active()` API lets this implementation retain the original uncached table whenever accessibility is active. Native UI Automation row lookup, selection and tab restoration pass. No framework patch is needed.

Opt-in `--profile-ui` diagnostics separate input handling, result application, row sorting/cell rendering and shell rendering. Partial debug profiling was diagnostic only, not release-speed evidence. Final measurements below run without that profiling flag. The no-UIA benchmark uses a native click to focus the search field and asserts accessibility remains inactive; the accessibility run explicitly activates UI Automation and records the active flag. Focus loss before or after any measured search aborts the run.

The memory comparison found a separate problem: the shared media LRU retained large previews and extracted details after the selection changed. These now belong only to the current selection; retired images leave both the CPU cache and GPUI's image store. Closing the info panel frees them, and late worker replies are still rejected by the existing epoch guard. Small list/tile icons retain their bounded LRU. The new Rust test verifies retention, removal, accounting and preservation of small icons.

The preview flash was an enlarged small association icon displayed while the full shell image was pending. Large previews now reserve their final bounds and show `Loading preview...` until the already-decoded full image arrives. An association-icon fallback remains available after a real provider failure. List rows retain their existing icons. Selection keys and stale-reply guards prevent another file's preview appearing.

Shared GPUI scrollbar styling now uses a 5-DIP thumb, 2.5-DIP radius, and a 7-DIP rounded hover/drag thumb, within a 12-DIP track. Both orientations use the same theme style; the wider input target remains. No Windows scrollbar or idle animation was added. Existing native GPUI menus keep their scrollbar-free presentation.

Tabs are explicit, not automatic: Ctrl+T duplicates the current folder/search; Ctrl+Enter opens the selected folder or current search in another tab. Typing edits the active tab. Ctrl+Tab / Ctrl+Shift+Tab switch, Ctrl+W closes, and closing the last tab quits. Session state and the shared index/worker model are unchanged.

## Release measurements

All UI samples use the same owned million-file dataset, three warmups and 30 measured native final-character-to-DirectX-Present samples per query, foreground throughout, and 200 correctly matching rows. Present return excludes physical DWM scanout.

| Query | Old release, same no-UIA harness p95 | New ordinary path p95 | New accessibility path p95 |
| --- | ---: | ---: | ---: |
| atlas | 14.374 ms | 11.243 ms | 14.427 ms |
| orbit | 13.549 ms | 11.707 ms | 15.188 ms |
| pixel | 14.182 ms | 10.727 ms | 14.132 ms |

Both new paths are below 16 ms in these samples. Ordinary-path worker p95: 2.466 / 2.447 / 2.054 ms; accessibility-path worker p95: 2.590 / 2.245 / 2.419 ms. The old no-UIA control predates the accessibility diagnostic, so its flag is unavailable; it uses native focus without UIA calls. The older committed accessibility-enabled measurements were 18.551 / 19.704 / 19.231 ms. The quieter same-session control is also faster than those historical numbers, so do not attribute the entire historical improvement to this patch. Focus-interrupted and incomplete debug attempts are excluded, rather than mixed into passing reports.

Five warm-process launches to first cached-results Present: **232.139 / 226.658 / 222.233 / 227.962 / 228.927 ms**, median **227.962 ms**, below 300 ms. Historical Stage C median was 253.232 ms; system cache/load conditions can affect this comparison.

The separately approved one-off elevated diagnostic ran valid Microsoft-signed [RAMMap](https://learn.microsoft.com/en-us/sysinternals/downloads/rammap), emptying system working-set/file-cache pages and the standby list (`-Es`, `-Et`, both exit 0). Standby counters fell from **3,040,878,592 to 3,371,008 bytes**. The parent harness checks that it is unelevated and launches Seek itself with an ordinary-user token and owned temporary profile. Only the finite purge helper is elevated. **One cold Windows file-cache sample: 303.664 ms to cached results, 3,403.720 ms to fully up to date.** This is a verified Windows file-cache purge, not a fresh boot, cold SSD/controller cache, or proof every shared OS page was evicted. Memory measurements below were taken before purging, so working-set trimming is not presented as an app memory saving.

| Controlled native session point | Old private commit | New private commit |
| --- | ---: | ---: |
| One tab, warm selected preview | 122.176 MiB | 98.523 MiB |
| Three tabs, warm selected preview | 144.395 MiB | 111.043 MiB |
| Three tabs, after 12 preview selections | 218.062 MiB | 115.219 MiB |
| Back to one tab | 220.027 MiB | 116.699 MiB |

Identical 240-file owned fixture and action sequence, original versus final release. The new media cache stays at one large preview / one detail entry; after 12 selections it holds 4,884,627 payload bytes, including small icons. Closing the info panel is separately verified to retain zero large previews/details. These are process snapshots with native allocator/driver caches and action history, not isolated per-tab allocation or a long-duration leak proof. Memory does not necessarily return to its initial snapshot after closing tabs.

Three-tab foreground idle: **20.0476425 seconds, 0 measured CPU ms, 0 redraws**, no input/focus change. Private commit at that point: 116.188 MiB. No polling, background process, startup task or artificial working-set trim was added to Seek.

## Verification, artifacts and limits

- 49 app Rust tests, strict all-target/all-feature Clippy, normal-feature compilation, formatting and release build pass.
- Ten final-release tab/memory/idle checks pass: independent queries, 120 scoped files equal an independent listing, list/grid viewport restoration, content mode/query/scope restoration, inactive-preview retention, Cancel preserving all 240 disposable files, inactive/active/final-tab close and quiet idle.
- Six final-release preview checks pass: PNG A to PNG B to text to PNG A, full 768-edge shell images, panel close releasing preview/details, and Light preview. Dark/Light screenshots show undecorated full-size images, rounded narrow scrollbar and centered glass modal labels at 125% SDR. No real file deletion.
- Three native screen captures without UI Automation confirm the cached ordinary path visibly replaces atlas rows with orbit and then pixel rows; accessibility remains inactive in each capture. These are paint-correctness checks, not another timing sample.
- No high-speed video of the sub-frame transition, physical HDR/mixed-DPI, full-C memory benchmark, other-machine check or long-duration leak proof. No broad Stage A/B suites were repeated.

Release: **19,876,864 bytes**, SHA256 `3AAB07193CB67876BD71BEA6FC26C69229B034468A52655FF7C46BC824EA39F8`.

Evidence: [summary](benchmarks/c-polish-summary.json), [ordinary UI](benchmarks/c-polish-ui-inactive.json), [accessibility UI](benchmarks/c-polish-ui-accessibility.json), [old control](benchmarks/c-polish-ui-before-inactive.json), [warm launch](benchmarks/c-polish-warm-startup.json), [cold launch and purge receipt](benchmarks/c-polish-cold-startup.json), [memory before](benchmarks/c-polish-memory-before.json), [memory/idle after](benchmarks/c-polish-memory-after.json), [preview checks](benchmarks/c-polish-preview.json).

Reproduce using `scripts/bench-window.ps1 -Executable <release> -Files 1000000 -BroadOnly -Walker`, with `-NoUiAutomation` for the ordinary path; `scripts/check-stage-c.ps1 -Executable <release> -MemorySnapshots -IdleSeconds 20`; `scripts/check-preview-scroll.ps1`; and `scripts/measure-startup.ps1 -Files 1000000 -Runs 5 -IdleSeconds 0 -Walker`. Cold reproduction additionally requires explicit authorization and `-Runs 1 -ColdFileCache -CacheTool <verified RAMMap64.exe>`; it deliberately requests one UAC prompt. All profiles/fixtures are owned test data.

Next: user visual review of the thin draggable scrollbar, rapidly changing PNG/text previews, and explicit tab creation/switch/close. Stage C's measured acceptance is now satisfied on this setup; no packaging is authorized.
