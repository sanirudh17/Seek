# Stage 2 final measurements — 2026-10-12

**All 24 real-driver checks pass.** The final administrator controller updated
only Seek's protected helper and ran all file/app verification with the
ordinary desktop token. Read-only real Downloads **500**, Documents **565**,
Desktop **24**: zero missing/extra/stale/denied individually and in the C:
index. First/live/closed fixture: **3,015 entries**, zero discrepancies each,
**10/10** top-five accuracy. Closed fixture catch-up **26.770 ms**;
closed deleted-parent replay **19.680 ms**, no reset/full reconciliation.
Invalidated Downloads/changed-fixture history recovers in **21.632 / 18.425 ms**.

| Measurement | Result |
| --- | ---: |
| First whole-C: helper index | 21,932.127 ms |
| First whole-C: forced walker index | 22,767.712 ms |
| Helper entries / mapped index bytes | 3,016,024 / 174,237,516 |
| Metadata / ordinary namespace authorization | 8,578.463 / 9,277.049 ms |
| Cached C: results presentation | 277.791 ms |
| C: USN catch-up / correct presentation | 2,731.649 / 2,805.871 ms |
| App private working set / private commit | 45.70 / 89.31 MiB |
| Window close / process exit | 110.675 ms / 0 |
| Helper CPU after client disconnect | 0 ms over 3,146.962 ms |
| Helper idle private working set / commit | 8.97 / 14.28 MiB |

The first helper index and saved app launch have **0 NTFS fallbacks, journal
resets or conservative reconciliations**. Cached results retain their visible
catching-up warning until synchronization finishes. Windows denied/skipped
locations: first helper **189**, walker **185**, app snapshot **191**.
The independent correctness checks cover the Known Folders and fixtures,
not every protected/system entry on C:. The drive changes between runs.

`cargo test --workspace --all-features --locked`: **56 passed**, one unchanged
explicit pipe benchmark ignored; combined formatting/test command **63.674 s**.
All-target/all-feature Clippy with `-D warnings`, default workspace check and
formatting pass (**11.375 / 3.608 / 4.702 s**).
`cargo build --workspace --release --all-features --locked` passes in
**328.785 s**. Final script **16:15:50–16:17:45 UTC**, exit 0.

The four native range scanners reduce metadata enumeration; ordinary access
checks remain mandatory. Deleted historical parents recover from bounded
cross-batch journal records. Signaling close before pipe cancellation fixes an
observed shutdown fallback: old cached CLI **40.487 s total** despite a
**2.506 s** catch-up, new focused check **16.226 ms shutdown / 4.942 s total**.
That focused check ran during compilation and is not a launch benchmark.

These sequential samples used existing OS caches and do not establish
fsearch parity, a cold-cache distribution, sustained foreground idle or heavy
churn correctness. Unchanged Stage 1 query/UI benchmarks below were retained.
GPUI Kit remains tested **0.7.1**; compatible **0.7.3** upgrade starts Stage 3.
Raw commands/numbers: [final results](stage2-final-results.json),
[short summary](stage2-summary.json), [source checks](stage2-ranged-scan-checks.json),
[tests](stage2-ranged-scan-tests.json), [release build](stage2-ranged-scan-release-build.json).
# Stage 1 measurements — 2026-10-11

**Historical checkpoint: Stage 1 foundation closeout.** Focused debug/release GUI
checks pass. Product acceptance still needs cold-cache, sustained quiet
foreground-idle, broad-query and stress/device verification. Retained saved-index
launch medians meet 300 ms on the Windows process-creation clock; one 1M launch
took 315.502 ms. The engine's current GitHub head matches our pin; Kit 0.7.1 is
the verified latest stable version. The documented Windows patch parks its
VSync thread on per-window frame demand.
See [backend provenance](../../vendor/gpui-windows/SEEK-PATCHES.md). No Stage 2 work
or external upstream messages were sent.

## Focused closeout, without repeat benchmarks

`cargo test --workspace --all-features --locked`: **12 passed**, including the
3,015-entry first/live/stopped engine lifecycle test and two new Unicode/typo
highlight tests. All-targets/all-features Clippy with `-D warnings`, formatting
and the default workspace build passed. The four commands took **29.207 s**
together; raw outcomes are in `stage1-closeout-validation.json`. Final
`cargo build -p seek --release --features benchmarks --locked` passed in
**279.147 s**, recorded in `stage1-closeout-release-build.json`.

`scripts/check-stage1.ps1` passed **8 focused checks**: filtered chip creation,
chip removal, Name sort, arrow/two-step Escape handling, live filesystem
changes, GUI-process closed changes/relaunch, geometry restoration and full
process exit. The check exposed a real table Escape binding conflict, fixed by
capturing the kit's Escape/Cancel actions. Window settings I/O remains off the
UI thread, with a small atomic save awaited through GPUI's quit hook.

The debug run's independent ground truth had **6 entries, zero missing/extra/
stale** in each phase; catch-up **2.251 ms**, closes **55.578 / 56.233 ms**, exit 0.
The final release run had the same zero-error counts, catch-up **1.192 ms** and
closes **56.452 / 63.821 ms**, exit 0. Its additional shell checks verified
Enter opened the exact test folder in Explorer and Ctrl+Enter selected the
exact file at the expected Explorer location. Both runs restored a
**1200×780 physical-pixel** window exactly. Raw reports:
`stage1-focused-check.json` and `stage1-release-shell-check.json`.

Command for the release run:
`scripts/check-stage1.ps1 -Executable target/x86_64-pc-windows-msvc/release/seek.exe
-ShellActions -Output docs/benchmarks/stage1-release-shell-check.json`.
The initial UI Automation lookup needed a bounded wait for provider readiness;
the harness now waits for initialization and query readback. Earlier harness
failures were corrected before reporting the passing run. No successful
benchmark was rerun in this closeout. The performance table below is retained
from the font-fix checkpoint, preceding the final highlight/geometry fixes.

The current upstream [fsearch README](https://github.com/noahdunnagan/fsearch)
publishes **0.13 ms median name search on an M4 Max with 8.3M files**. Seek's
retained backend exact-query measurements (~0.003–0.004 ms) and GUI timings
cannot establish parity: different hardware, smaller generated dataset,
three unique exact queries, and different timing boundaries. The published
3.0 ms typing figure sums all filename keystrokes; it is not GUI paint latency.

One final attributed idle trial (`stage1-closeout-idle.json`) recorded **0 ms
process CPU over 20.234 s**, two presentations and six native activity/focus
messages. Foreground was lost on **68/78** checks, so the report explicitly
sets `quiet_foreground_sample=false`; **sustained quiet foreground idle remains
uncertified**. No retry followed. Private working set was **27.00 MiB**, close
**48.877 ms**, exit 0. Its one saved/warm 10k launch presented 200 up-to-date rows
in **201.586 ms** from kernel process creation. This is a single functional/idle
run, not a new launch distribution or cold-cache claim.

## Font and startup investigation

`upstream-check.json` records the current fsearch-crossplatform head, already
vendored: `16eaebd89b8db161e4a5cac961f746565b878686`, merged 2026-10-10 at
21:49:20 UTC. `kit-version-check.json` records latest stable GPUI Kit 0.7.1.

Startup phase instrumentation found about 108–121 ms in initial DirectWrite
setup and 139–153 ms between shell construction and window-open return.
DirectWrite now uses the Windows font cache instead of forcing an immediate
refresh. Refresh after a failed family lookup remains. The shell/spike now use
the installed `Segoe UI Variable Text` family; `Segoe UI Variable` was missing
and caused a failed lookup. Corresponding measured phase intervals fell to
about 5–7 ms and 7–15 ms. This keeps the existing GPUI renderer and engine.

The old launch clock began **before PowerShell's Start-Process**, including its
setup cost. The new harnesses retain that clock separately and use the actual
Windows process-creation timestamp for application launch. Do not directly
compare the two launch clocks. Process-entry intervals use the same monotonic
clock before and after the fixes; the 10k median fell from 435.021 to 222.271 ms.
The first rebuilt executable sample was 830.102 ms on the old harness clock,
207.820 ms after entry. The pre-entry interval was not attributed to a specific
cause; neither Defender nor PowerShell alone was established as its cause.

| Latest release measurement | 10,000 files | 1,000,000 files |
|---|---:|---:|
| Kernel process creation → cached results, five-run median | 255.539 ms | 243.910 ms |
| Same five-run range | 240.755–260.562 ms | 233.455–315.502 ms |
| Native query → Present median range, 30 samples/query | 6.132–6.918 ms | 6.058–6.582 ms |
| Worst query p95 | 11.169 ms | 10.129 ms |
| Private working set after queries | 26.32 MiB | 38.51 MiB |
| Total working set / private commit | 61.64 / 70.41 MiB | 139.10 / 84.01 MiB |
| UI-benchmark first cached presentation | 240.632 ms | 252.073 ms |
| UI-benchmark up-to-date presentation | 240.632 ms | 1,181.985 ms |
| Engine walker catch-up interval | 16.260 ms | 1,129.162 ms |
| Close → process exit, code 0 | 110.452 ms | 86.485 ms |

Commands: `scripts/measure-startup.ps1 -Runs 5 -IdleSeconds 0 -Files 10000
-Output docs/benchmarks/startup-kernel-clock-10000.json`, repeated with one
million files; `scripts/bench-window.ps1 -Executable
target/x86_64-pc-windows-msvc/release/seek.exe -Files 10000 -Runs 30 -Output
docs/benchmarks/ui-font-fix-10000.json`, repeated with one million files.
Build with `cargo build -p seek --release --features benchmarks` (diagnostic
build 4m 53s; optimized rebuild 4m 42s). Both release executables ran and closed.

Each startup contains 200 cached rows; the 1M status says `catching_up=true`.
These are saved-index launches **without flushing the OS cache**. Query samples
use three exact filename queries, three warmups and 30 measured runs each;
each sample verifies the expected single result. Native WM_CHAR receipt →
application update → successful DirectX Present return excludes physical DWM
scanout and harness waiting. Backend source is unchanged: previous opt-level 3
medians remain 0.00390–0.00410 ms / 0.00290–0.00305 ms. No broad/fuzzy GUI
latency or fsearch parity claim is made.

## Latest correctness, spike and validation

`seek-cli self-test --output docs/benchmarks/correctness-font-fix.json` ran the
current debug CLI: **3,015 indexed and ground-truth entries**, **0 missing,
extra or stale** after first index, live changes and stopped-engine/relaunch.
Live verification 148.828 ms; walker relaunch catch-up 13.656 ms; accuracy
**10/10 within top five**. The fixture includes Unicode, emoji, actual unpaired
UTF-16, hard links, a junction loop, long paths and case-only renames. This is
engine lifecycle verification, not a complete GUI-process relaunch test.

Twenty-second quiet foreground idle remains **unverified**. A 10k trial lost
foreground on every check. A 1M trial stayed foreground but received typing and
query generation changes: 3,312.5 ms CPU and 359 presentations, so it is not an
idle sample. Further attempts lost foreground with one redraw and 31.25 / 15.625
ms CPU. Raw JSONs explicitly mark invalid trials; preserve them, rather than
averaging them as idle. The default-feature debug shell recorded zero CPU over
1.003 seconds with foreground endpoints true, private working set 27.17 MiB,
and close 74.656 ms, exit 0. That short sample does not certify sustained idle.

The latest release spike's Acrylic API returned success, and the accessibility
tree contains the input and row nodes. Its screenshot was occluded by another
window, so the latest glass/image appearance was not reverified. Its one-second
foreground-endpoint sample used 437.5 ms CPU without input attribution;
retain that conflicting result. Close took 65.876 ms, exit 0. Earlier unobscured
glass/image observations remain in the historical record below. No browser
actions were taken and no screenshot was committed.

`font-fix-validation.json`: workspace build **22.39s**, all-features tests
**18.90s (10 passed)**, all-targets/all-features Clippy with `-D warnings`
**11.62s**, formatting **4.49s**; each exited 0. Standalone frame scheduler
**2 passed**. The ignored report wrapper had a null-log parsing issue for silent
commands; its handling was fixed, and child exit codes/timings were checked.

Stage 1 stays open: capture a continuously foreground, input-free 20-second
sample with input/frame attribution; profile redraws if still present. Then
measure OS-cache-cold launch and the 315.502 ms outlier, fix queued-event catch-up
status and finish GUI lifecycle/interaction, highlighting and geometry work.
The NTFS/helper stage has not started.

## Previous continuation results (before the font fix)

Three exact queries in each dataset, three warmups and 30 measured samples per
query. UI samples begin when the last character's native WM_CHAR is received and
end after the matching results have been applied and DirectX Present returns.
They include worker/channel/UI work, exclude harness wait and physical DWM
scanout, and validate the expected one-row result on every run. The UI caps
results at 200 even though the reference query spells `limit:500`.

| Files | Backend median range, opt 3 | Backend worst p95 | UI median range | UI worst p95 | Private working set after queries |
|---:|---:|---:|---:|---:|---:|
| 10,000 | 0.00390–0.00410 ms | 0.00450 ms | 4.907–6.296 ms | 10.789 ms | 28.07 MiB |
| 1,000,000 | 0.00290–0.00305 ms | 0.00500 ms | 5.779–6.192 ms | 9.730 ms | 40.87 MiB |

UI commands: `scripts/bench-window.ps1 -Executable
target/x86_64-pc-windows-msvc/release/seek.exe -Files 10000 -Runs 30 -Output ...`,
and repeat with `-Files 1000000`. Build the executable with
`cargo build -p seek --release --features benchmarks` (5m 16s for the first
continuation build). `seek-cli bench --ui-executable PATH` integrates this
harness into the backend report. `benchmarks` and `--metrics` are explicit opt-in
diagnostics; production builds omit the observer and file-output worker.

The integrated `seek-cli bench --files 10000 --runs 30 --ui-executable
target/x86_64-pc-windows-msvc/release/seek.exe` also completed with exit 0;
`cli-bench-integrated.json` contains its backend and foreground UI results. Its
cached launch took 541.547 ms, confirming the launch limit remains unmet after
the final release rebuild. Use the dedicated runs in the table for comparison.

| Launch/shutdown measurement | 10k | 1M |
|---|---:|---:|
| Process launch → first cached results presentation | 474.070 ms | 572.675 ms |
| Process launch → up-to-date result presentation | 474.070 ms | 1,232.471 ms |
| Process entry → same presentation | 394.184 ms | 1,145.712 ms |
| Engine's walker catch-up interval | 11.631 ms | 1,120.736 ms |
| Window close → process exit | 77.088 ms | 80.510 ms |
| Total working set / private commit | 63.56 / 72.21 MiB | 142.58 / 85.07 MiB |

**Launch fails even with the OS cache warm.** These are first launches in their
individual measurements, not OS-cache-cold trials. The table waits for an
up-to-date presentation. First cached presentation was extracted from the original
trace of each same run; the 1M record contains 200 results and `catching_up=true`,
separate from later reconciliation completion. The 1M index file is 72.65 MiB; process working
set includes resident mapped pages. No fsearch parity claim is made.

Current backend raw files: `10000-current-opt3.json`, `1000000-current-opt3.json`.
Opt-level `s` raw files end in `-current-opts.json`: median ranges
0.00300–0.00450 ms (10k), 0.00340–0.00790 ms (1M), worst p95
0.00460 / 0.00950 ms. The CLI is 2,297,344 bytes with opt 3 and 1,839,104 with
`s`. Retain opt 3 because the larger workload slowed with `s`. Runs were separate
and non-randomized; generation/existing-file checks and launch reconciliation
are reported separately from warm queries.

`correctness-extended.json`: **3,015 indexed / ground-truth entries and zero
missing, extra or stale entries** at first index, live changes and stopped-engine
relaunch. Added hard links, a junction loop, a real unpaired-surrogate filename,
case-only live/closed renames and path-scoped accuracy. Live verification interval
83.012 ms, walker catch-up 3.233 ms; **10/10 top-five queries**. This small accuracy
set is not upstream's typo-rate experiment. Full GUI lifecycle/open/reveal tests
remain incomplete.

Patched debug spike: 0 ms CPU over 11.726 seconds; private working set 34.32 MiB.
Debug Mica: 0 ms over 20.003 seconds, 34.16 MiB. Release repeat samples reached
0 ms over 20.019 and 20.005 seconds, around 29 MiB, but their explicit foreground
flags were false. One earlier release spike recorded 140.625 ms over 20.012
seconds without focus/input attribution. Preserve these limits: **foreground
zero-idle cost is not yet certified**. Native input and table scrolling were
observed with the demand patch. Acrylic API acceptance is captured in
`spike-demand-release-foreground.json.glass.log`; explicit Mica and unsupported
fallback observations are recorded only where tested. `spike-demand-mica-api.json.glass.log`
confirms requested/actual MicaBackdrop API success, 0 ms CPU over 20.015 seconds
and process exit code 0 (background sample). API success and visible
appearance are separate checks.

Fresh million-file scan interrupted by close: 58.909 ms to exit, code 0,
`close-during-index-debug.json`. The UI no longer joins a crawler/checkpoint.
Atomic saved snapshots plus launch reconciliation preserve the last complete
baseline. This measurement does not establish every shutdown race.

Extended engine/CLI tests: 10 passed. Standalone condition-variable scheduler
tests: 2 passed, including 1,000 request/wait exchanges. Final workspace build,
test, Clippy and formatting results are in `continuation-validation.json`.
Next work stays Stage 1: profile initialization/cached-results startup, separate
it from catch-up, certify foreground idle and finish the remaining UI/correctness
checks. Keep GPUI; do not advance to the helper stage yet.

## Earlier checkpoint, before the demand patch (historical)

The remaining sections preserve the original checkpoint's measurements and
limitations. Statements about unimplemented frame demand or unmeasured UI/memory
were true at that earlier checkpoint; the continuation results above supersede
them.

Machine: Windows 11 Home build 26300, AMD Ryzen 7 7445HS, 12 logical processors, C: NTFS. Rust 1.96.0 MSVC; Visual Studio 2022 Build Tools; SDK 10.0.26100.0. Kit 0.7.1, commit `1ccf011620da24d633f7cd6d3c11ca2e97798dcf`; official Zed `279fe070bb389b79652e52065b2f001edcc0b11b`, dated 2026-10-04. Kit 0.7.1 was verified as the latest stable release on crates.io on this date.

The initial older component/pin combination failed with 44 API compatibility errors. Aligning current kit dependencies with the source revision declared by its GPUI snapshot, and fixing its macro crate-name lookup, resolved compilation. The current Windows backend is a separate `gpui_platform` dependency. The release spike build completed in 12m 22s. Local logs are in ignored `.references`; raw measurements below are committed.

## GPUI gate

The real window rendered the kit input, one PNG and a 100,000-row lazy table. A captured view showed rows around 578 while scrolling, with roughly 20 visible rows; this demonstrates virtualization/scrolling, not a measured scroll latency. Acrylic was visibly translucent after clearing the root's opaque theme background. Native caption buttons were visible. Explicit Mica selection is implemented; Mica appearance and automatic Acrylic → Mica → solid fallback were not verified.

| Measurement | Result | Evidence |
|---|---:|---|
| Debug spike private working set | 32.79 MiB | `spike-debug.json` |
| Release spike private working set | 60.48 MiB | `spike-release.json` |
| Static-caret debug spike private working set | 32.45 MiB | `spike-static-caret-debug.json` |
| Debug shell with 10,000-file index, private working set | 29.99 MiB | `shell-memory-close-debug.json` |
| Same shell total working set / private commit | 90.46 / 73.61 MiB | `shell-memory-close-debug.json` |
| Spike close | Process exited, exit code 0 | All completed spike JSONs |
| Tested shell close | Process exited within 10-second wait | `shell-memory-close-debug.json`; exit code unavailable for the attached process |
| Quiet static-caret shell CPU | 609.375 ms over 20,010.137 ms | `shell-idle-debug.json` |

The last CPU sample is **3.045% of one core**, or about **0.254% of all 12 logical processors**, and fails zero idle cost. No automation input was sent during that sample. Earlier spike samples included different focus/interaction conditions and are retained as raw evidence, not treated as valid zero-idle measurements.

`scripts/measure-spike.ps1` reads `Win32_PerfRawData_PerfProc_Process.WorkingSetPrivate`, samples `Process.TotalProcessorTime`, sends WM_CLOSE and waits for process exit. Historical JSONs call the window-handle timing `input_idle_ms`; this was a labeling error, not a first-paint or input-idle measurement. The script now calls it `window_handle_ms`. These historical measurements were 1.51–2.08 seconds and do **not** establish the 300 ms usable-window target.

The pinned Windows `begin_vsync_thread` calls `RedrawWindow(..., RDW_INVALIDATE)` for every window each VSync cycle, regardless of application changes. Current upstream main was read as well and still has this behavior. This is a framework-level conflict with the event-only frame requirement; it does not prove that every request redraws the GPU scene. Source: [pinned implementation](https://github.com/zed-industries/zed/blob/279fe070bb389b79652e52065b2f001edcc0b11b/crates/gpui_windows/src/platform.rs#L433).

## Backend filename benchmark

Actual files generated on NTFS; three unique exact filename queries, a folder scope, `limit:500`, three warmups and 30 measured runs per query. Every run checked that the expected single filename was returned. Timings include parse, search and result path materialization. They exclude IPC, input dispatch, rendering and painting. The FinderSearch reference includes IPC, so these numbers do not establish parity with fsearch or end-to-end latency.

| Files | Query | Median ms | p95 ms |
|---:|---|---:|---:|
| 10,000 | `'atlas-notes-00000.txt` | 0.00690 | 0.00760 |
| 10,000 | `'orbit-budget-00001.txt` | 0.00675 | 0.01460 |
| 10,000 | `'pixel-design-00002.txt` | 0.00680 | 0.00720 |
| 1,000,000 | `'atlas-notes-00000.txt` | 0.00490 | 0.00590 |
| 1,000,000 | `'orbit-budget-00001.txt` | 0.00660 | 0.02530 |
| 1,000,000 | `'pixel-design-00002.txt` | 0.00480 | 0.00560 |

| Dataset | Generation | First index / initial persistence | Index entries | Index bytes | Bytes/entry |
|---|---:|---:|---:|---:|---:|
| 10,000 files | 6.051 s | 27.234 ms | 10,018 | 763,008 | 76.16 |
| 1,000,000 files | 618.587 s | 3.213 s | 1,001,008 | 76,176,256 | 76.10 |
| Full C: walker | Existing drive | 38.847 s | 2,868,114 | 169,610,112 | 59.14 |

The full-drive measurement included the generated million-file fixture. It reported **183 skipped locations**; no full-drive ground-truth comparison was performed. Its CLI checkpoint adds work after the initial ready time; the table reports the engine's recorded first-index interval, not the entire command wall time. Helper timings are unmeasured and belong to Stage 2.

Index entry counts include synthetic root/ancestor entries; verification counts only actual entries in the selected fixture.

Raw data: `10000-opt3.json`, `1000000-opt3.json`, `drive-walker.json`. These opt-level 3 runs were taken before the final directory-attribute/cancellation/persistence refinements. They are baseline measurements, not a final performance certification. There are no painted end-to-end numbers, 1M-loaded process-memory measurement, or opt-level `s` comparison because work stopped at the GPUI gate.

## Correctness

Command: `seek-cli self-test --output docs/benchmarks/correctness-current.json`, MSVC release CLI. Independent ground truth uses `std::fs::read_dir` and Windows metadata; the index uses FindFirstFileExW and its live overlay.

| Phase | Ground truth / indexed | Missing | Extra | Stale |
|---|---:|---:|---:|---:|
| First index | 3,012 / 3,012 | 0 | 0 | 0 |
| Live create/rename/move/delete/modify | 3,012 / 3,012 | 0 | 0 | 0 |
| Stopped-engine changes, saved-index relaunch | 3,012 / 3,012 | 0 | 0 | 0 |

Live reconciliation: **210.377 ms** including repeated ground-truth verification in the test harness. Walker relaunch catch-up: **5.484 ms**. No helper was installed. First-index ready time was 21.132 ms in the earlier run; the committed current JSON contains its own first-index timing. The two run files remain distinct.

Accuracy: **7/7 within top five, 100%**. Exact/partial names, one missing letter (`invoce`), swapped letters (`invocie`), mixed ASCII case, Unicode/emoji and a long-path target. These two typo examples do not reproduce the upstream 98% experiment; no percentage comparison is made. Automated tests additionally cover parser modes, extension/size/content-search rejection, Unicode casing, exact/prefix ranking protection, newest-first top 200, hidden-view filtering without exclusion, cancellation, reversible UTF-16/WTF-8 and ordinal path comparison.

The stopped-engine test is not a complete GUI process relaunch test. Hard-link, junction, case-only rename, inaccessible subtree, overflow/heavy-change and process-exit-during-indexing tests remain unfinished. Known limitations and the exact next action are in PROGRESS.md.

Final commands: `cargo test --workspace` passed **9 tests, 0 failures**; `cargo build --workspace` passed in **4.65 s**; `cargo clippy --workspace --all-targets -- -D warnings` passed in **1.76 s**. Final debug `seek-cli verify` again reported zero missing/extra/stale entries (`verify-final.json`). The rebuilt debug shell opened on the 10,000-file dataset and WM_CLOSE exited with code 0 (`shell-final-run.json`). These checks do not change the failed idle gate.
