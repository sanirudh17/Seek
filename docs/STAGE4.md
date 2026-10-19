# Stage 4: completed implementation and available-hardware validation

Updated 2026-10-16. The foreground-idle blocker is resolved on the final release. All available automated/SDR checks pass. **HDR appearance and physical mixed-DPI monitor movement remain unverified because that hardware is unavailable.** The measured startup/search limits below are retained explicitly; this is not a universal sub-300 ms launch or sub-16 ms search claim.

## Final changes

- View and action buttons share the same 14% accent wash for pressed/selected states. Hidden items, sort, Info and Settings use one shared component style. Active icons retain readable neutral foregrounds, including pale custom accents. Info highlights only with a visible inspector. No other layout or icon redesign.
- Settings visibility/location selectors remain left aligned, other button labels centered, and default-view changes apply immediately. System/Light/Dark, whole-window glass and persisted locations/exclusions remain intact.
- Startup begins the existing index worker while native UI/artwork initialize; redundant pre-window theme setup is removed. The normal window still receives its saved theme before its first view. Native artwork and single-instance behavior are preserved. First indexing and deferred MFT/content/semantic work are unchanged.
- Saved-index structural validation, atomic snapshots/cursors, bounded event coalescing/caches, bulk-update snapshot isolation, direct indexed children and stable scroll anchoring from the earlier Stage 4 fixes remain in place. [Navigation implementation details](benchmarks/stage4-navigation-report.md).

GPUI Kit 0.7.1, GPUI `279fe070bb389b79652e52065b2f001edcc0b11b`, the patched Windows crate and the installed helper are unchanged. No installation, service update or administrative action was needed.

## Correctness and UI

**83 Rust tests pass**, one existing benchmark ignored. Formatting, all-target/all-feature Clippy with `-D warnings`, normal-feature checking, debug workspace and full optimized workspace build pass. The final build took 460.481 s. [Source checks](benchmarks/stage4-closeout-checks.json), [release build](benchmarks/stage4-closeout-release-build.json), [exact final commands and hashes](benchmarks/stage4-closeout-release-commands.json).

**30 focused release checks pass:** toolbar/color/DPI 9, current-app view/theme/contrast 6, Settings/persistence/relaunch 7, expansion/live-scroll/alignment 5 and real Known Folder navigation/recovery 3. Pixel comparisons record both colors and permit at most 2/255 per channel for raster rounding; Settings is sampled while pressed, before its modal shades the toolbar. Bounds reject overlaps across the tested renderer transitions. Final captures were reviewed inline in both themes; this is not a new independent-review claim. [Toolbar/display evidence](benchmarks/stage4-closeout-toolbar.json), [UI](benchmarks/stage4-closeout-ui.json), [Settings](benchmarks/stage4-closeout-settings.json), [navigation](benchmarks/stage4-closeout-navigation.json).

Fresh independent million-file verification matches **1,001,001 entries**, with zero missing/extra/stale/denied entries. Each stress source applies **49,288 live operations from four writers**, then 20 closed-app operations; first/live/closed/damaged-header checkpoints all match independent truth. Helper lifecycle covers Unicode, emoji, unpaired UTF-16, long paths, hard links and restart replay, with **10/10** top-five targets. These headless correctness checks ran while release compilation was active; their latency is under that load: walker: query p95 0.0833 ms, checkpoint 9.812 s; helper: query p95 0.1215 ms, checkpoint 33.955 s. The helper checkpoint remains slower under a burst. [Million-file truth](benchmarks/stage4-closeout-million-truth.json), [walker](benchmarks/stage4-closeout-stress-walker.json), [helper](benchmarks/stage4-closeout-stress-helper.json), [lifecycle](benchmarks/stage4-closeout-helper-lifecycle.json), [commands](benchmarks/stage4-closeout-core-commands.json).

## Search, navigation and launch

Milliseconds, median / p95; 3 warmups and 30 measured samples per query. Backend includes parsing, a fixture-folder scope and materialized paths. UI searches All locations and can reuse typing caches; these workloads differ and cannot be subtracted to infer UI overhead. UI measures native final-character receipt through DirectX Present return, with correctness assertions; physical scanout is excluded. Existing datasets were reused. [10k](benchmarks/stage4-closeout-bench-10000.json), [1M](benchmarks/stage4-closeout-bench-1000000.json).

| Query | 10k backend | 10k UI | 1M backend | 1M UI |
| --- | ---: | ---: | ---: | ---: |
| Exact atlas | 0.0045 / 0.0051 | 5.280 / 9.328 | 0.0036 / 0.0041 | 5.434 / 10.395 |
| Exact orbit | 0.0024 / 0.0026 | 7.821 / 11.396 | 0.0054 / 0.0057 | 5.697 / 11.545 |
| Exact pixel | 0.0031 / 0.0033 | 7.359 / 11.511 | 0.0032 / 0.0054 | 7.259 / 11.477 |
| Broad atlas (200) | 0.2636 / 0.7280 | 13.778 / 16.896 | 12.6292 / 13.4262 | 14.645 / 17.805 |
| Typo altas | 0.0019 / 0.0019 | 6.566 / 9.743 | 0.0033 / 0.0037 | 7.168 / 11.230 |
| Prefix | 0.0018 / 0.0019 | 4.799 / 10.283 | 0.0023 / 0.0038 | 6.245 / 9.589 |
| Extension filter | 0.0024 / 0.0041 | 6.397 / 10.331 | 0.0090 / 0.0096 | 6.972 / 11.006 |

Broad-query UI p95 exceeds 16 ms in both datasets; the scoped backend p95 stays below 16 ms in these samples. Saved launch medians from five new processes are **321.207 ms (10k)** and **317.699 ms (1M)**, slightly above the earlier approximate 300 ms goal. The quiet same-machine previous-release 10k median is **333.885 ms**. These sequential, warm-cache samples are not a cold-cache or broad hardware distribution. [Previous release](benchmarks/stage4-closeout-startup-before-quiet.json), [10k](benchmarks/stage4-closeout-startup-10000.json), [1M](benchmarks/stage4-closeout-startup-1000000.json). The earlier diagnostic taken during compilation is not used for that comparison.

Twelve real Downloads/Desktop/Documents/Pictures/Videos interactions all pass the 100 ms guard, maximum **32.935 ms**, during recovery of a disposable copy of the real saved index. The scrolled view stays on the same content through live additions and deletion above it; 2,049 direct children expand from the index. [Real navigation](benchmarks/stage4-closeout-real-navigation.json). Three additional copied-profile C: launches and complete exits pass; cached presentation and recovery remain separate. These use a user-restricted in-memory metrics pipe, so the trace does not index itself. Real user files are read-only; this is not a new independent whole-C: truth audit. [Real-drive launch](benchmarks/stage4-closeout-drive-startup.json).

| Dataset | Private working set MiB | Private commit MiB | Mapped index MiB | Close ms / exit 0 |
| --- | ---: | ---: | ---: | ---: |
| 10,000 | 33.98 | 87.22 | 0.73 | 211.193 |
| 1,000,000 | 44.35 | 100.03 | 72.65 | 158.952 |

These are process snapshots, not a long-duration leak proof. The ordinary real-profile app is reopened and responding: [launch record](benchmarks/stage4-closeout-normal-launch.json).

## Sustained foreground idle

Both final release datasets pass at least 20 uninterrupted quiet foreground seconds with **zero measured process CPU time and zero presentations**. Raw intervals and all interrupted/background segments are retained. Qualification uses unchanged native activity, applied generation/status and foreground identity; CPU/redraws are outcomes, never filters. Focus is requested once and never locked; no global input blocking or Windows policy change. Windows CPU counter resolution applies. The harness uses 250 ms observation intervals; the app does not poll at rest.

| Dataset | Longest quiet foreground seconds | CPU ms | Presentations | Private working set MiB |
| --- | ---: | ---: | ---: | ---: |
| 10,000 | 30.132 | 0 | 0 | 36.15 |
| 1,000,000 | 30.024 | 0 | 0 | 39.04 |

[10k raw sample](benchmarks/stage4-closeout-idle-10000.json), [1M raw sample](benchmarks/stage4-closeout-idle-1000000.json). Prior focus-contaminated results remain historical evidence and are not substituted for these passes.

## Display coverage and limits

The attached 1920x1080 SDR panel reports **120 DPI / 125%**, Windows HDR **Not Supported**, advanced color disabled. A second GPU reports no active display mode. [Windows dxdiag inventory](benchmarks/stage4-closeout-display-inventory.json). Actual laptop captures cover both themes. Disposable test windows also receive **100/125/150/200% WM_DPICHANGED** transitions, with separate nonoverlapping toolbar bounds and rendered-size assertions in both themes. Windows display settings are untouched. These are renderer/native-handler simulations, not physical mixed-monitor DPI or HDR certification. The method follows [Microsoft's WM_DPICHANGED contract](https://learn.microsoft.com/en-us/windows/win32/hidpi/wm-dpichanged); Windows [Advanced Color documentation](https://learn.microsoft.com/en-us/windows/win32/direct3darticles/high-dynamic-range) distinguishes SDR advanced color from HDR.

HDR appearance requires an HDR-capable attached display. Physical mixed-DPI movement, other Windows/GPU devices, arbitrary wallpapers/Mica fallback and physical power-loss durability remain untested. No screenshots or SDR tests are described as HDR verification.

Failed harness reports are retained: exact RGB equality initially rejected a one-level raster difference; sampling Settings after its modal opened compared different composited backgrounds; PowerShell's hashtable property sort incorrectly ordered bounds. The final checks sample the actual pressed state and sort numeric coordinates explicitly. The idle harness also now sorts numeric segment durations explicitly and fails with a nonzero exit when no qualifying segment exists. Earlier focus/input-interrupted samples, including the first 1M attempt, remain separate from the final uninterrupted passes. The first optimized build could not replace the still-running app; after graceful close, the unchanged source rebuilt successfully. These failed attempts are not passes.

## Reproduce

Close the running Seek window before rebuilding on Windows. Use the pinned Rust 1.96.0 MSVC / VS 2022 / SDK 26100 toolchain:

```powershell
cargo test --workspace --all-features --locked --offline
cargo clippy --workspace --all-targets --all-features --locked --offline -- -D warnings
cargo build --workspace --release --all-features --locked --offline
./scripts/check-stage4-toolbar.ps1
./scripts/check-stage4-navigation.ps1
./scripts/check-stage4-navigation.ps1 -RealProfile
./scripts/measure-quiet-idle.ps1 -Files 10000 -Seconds 30
./scripts/measure-quiet-idle.ps1 -Files 1000000 -Seconds 30
```

The exact query, stress, startup, Settings and real-drive commands are in the linked ledgers. The idle check needs foreground focus throughout its qualified segment. Benchmark instrumentation is opt-in through `--metrics`; ordinary builds may omit `--all-features`.

Check by eye: matched view/action selection fills, readable icons with pale accents, both themes, left-aligned visibility selector and stable scrolling. Full HDR and physical mixed-DPI checks require suitable hardware. No Later ideas were started.

Final app SHA256: `8ad37a41061f218e4c5a39a2cefe47fdfa38d8cad61643835cecb2f6c111ff31`. [Previous Stage 4 baseline](STAGE4-BASELINE.md), [machine-readable current summary](benchmarks/stage4-summary.json).
