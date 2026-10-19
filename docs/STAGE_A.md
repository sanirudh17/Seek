# Seek v2 Stage A - daily-use basics

Closed on 2026-10-16 at the user's request to stop repeated testing. Stage B is next; no Stage B or C implementation or packaging is included. The existing evidence below is retained, including failed harness attempts. Closure does not turn the remaining performance and hardware limits into passes.

## Delivered

- Cached launch with favorite-folder catch-up first: Downloads, Desktop, Documents, Pictures, Videos, Music, then recently changed folders and the remainder at low priority. Helper installations replay the journal; ordinary installations retain the correct full relist. First indexing and catch-up show real counts or percentages when a total is known, and completion is explicit.
- Type filters (Any, Folder, Document, Image, Movie, Audio, Code, Archive), a removable search chip and typing suggestions. JPEG has one suggestion matching both jpg and jpeg. Name suggestions search filenames; content search is deferred to B.
- GPUI row and toolbar menus: Open, Open with, Reveal, Copy, Cut, Paste, Copy path/name/quoted path, Rename/F2, Recycle Bin, New folder and Properties. All items returns from Largest to the previous query, sort and view.
- Windows file clipboard, including verified paste into Explorer; focus-aware file/text shortcuts and Ctrl+Shift+C for paths. Rename, folder creation and recycling update the visible list and normal tracking. Mutations use Windows IFileOperation on a worker, collisions ask, and deletes are constrained to the Recycle Bin. The worker blocks on messages instead of polling. Open with and Properties launch Windows dialogs from GPUI menu items.
- GPUI typed internal moves with a drag preview, to folder rows and sidebar locations. Cross-application OLE drag is not part of A.
- Indexed largest files/folders, scoped or drive-wide, top 200. Sizes at or above 2 GiB inherit the existing format's downward rounding of less than 2 MiB per file; approximate values are disclosed. Folder totals sum indexed logical sizes, not allocated disk space.
- Requested finish: compact Type menu/chip, no duplicate JPEG suggestion, direction-aware submenu arrows, aligned breadcrumbs, in-window rename sheet, roomier menus without separators or visible scrollbars, readable custom accents, centered Hidden/Settings controls and Appearance navigation. Light and Dark menus use neutral translucent surfaces. Selected image previews use source-resolution artwork without the shell's dark photo frame; folders request large native artwork. Gallery preview requests now settle.

## Real C: startup measurements

These are saved-profile, warm-cache measurements on this user's real C: drive, using disposable profile copies. Cached and priority times run from process launch to the instrumented native presentation return, not physical scanout. OS caches were not flushed. Before and after runs were sequential against a changing drive, not a controlled paired distribution. Startup reports predate the final UI-only refinements; indexing source is unchanged.

| Sample | Cached results | Priority folders current and presented | Fully caught up |
| --- | ---: | ---: | ---: |
| Before, helper, one run | 354.284 ms | Not separately measured | 2,833.058 ms |
| Before, no helper, one run | 300.705 ms | Not separately measured | 57,720.724 ms |
| After, helper recovery | 319.002 ms | 355.612 ms | 32,651.469 ms |
| After, valid helper replay 1 | 347.198 ms | 372.940 ms | 2,291.370 ms |
| After, valid helper replay 2 | 315.987 ms | 315.987 ms | 2,142.705 ms |
| After, no helper, one quiet run | 541.018 ms | 880.811 ms | 140,523.752 ms |

Sources: [before helper](benchmarks/v2-stage-a-before-helper.json), [before walker](benchmarks/v2-stage-a-before-walker.json), [after helper](benchmarks/v2-stage-a-after-helper.json), [after walker](benchmarks/v2-stage-a-after-walker.json).

Priority folders meet the approximate one-second target in these samples. **Full no-helper catch-up remains slow and is worse than the earlier 57.7 s sample.** It is a known remaining performance problem, not a claimed speedup. The later run processed 305,908 directories / 3,397,919 entries: read/preparation including snapshot forks took 126,823.952 ms, application 12,676.772 ms and planning 461.692 ms. Private working set was 59,031,552 bytes, excluding separately mapped index pages.

The normal helper samples spent about 1,841-1,843 ms saving the atomic snapshot/cursor, 53-59 ms loading, 110-246 ms reading and 28-58 ms applying. Atomic persistence was preserved. The first after-helper sample required one journal-budget reconciliation and is reported separately rather than discarded. Helper private working sets across the three runs were 52,854,784 / 60,104,704 / 57,847,808 bytes.

Profiling exposed unnecessary whole-overlay forks for unchanged parents with large descendant subtrees. The fix avoids these for a small unchanged direct-child diff, while retaining safe forks for deletion, retyping and recursive scans; a dedicated 600-entry regression covers base and overlay cases. An earlier recovery diagnostic was stopped at 376.354 s, with 692.344 CPU seconds and 3,014,709,248 private committed bytes. It has no completed truth/latency verdict and cannot establish an exact before/after recovery speedup. A separate baseline startup timeout came from the sampler's ring retention and is retained as a harness failure, not an application launch measurement.

## Correctness and daily-use validation

Final release SHA256: `607503F85EA19235D2BD73D916B36A2598CE41452E0862841481511F6178704E` (17,784,832 bytes).

- **91 Rust tests passed**, one existing benchmark ignored. Formatting, strict all-target/all-feature Clippy, normal-feature workspace check and optimized all-feature workspace build passed. The optimized build took 7 min 36 s.
- **30/30 release desktop checks passed**, including actual Explorer clipboard paste with matching bytes, Windows collision Skip preserving both files, copy/cut/paste, immediate rename/new folder, restored-window rename bounds, both internal drop targets, recycling, text/file focus behavior, Type suggestions/chip, return from Largest, previews settling, light menus and process exit. [Final UI report](benchmarks/v2-stage-a-ui.json), [Explorer clipboard report](benchmarks/v2-stage-a-explorer-clipboard.json). Browser upload/chat paste is not independently verified.
- Before and after synthetic lifecycle runs each used 1,001 generated files, four writers, **4,932 live operations and 20 closed-app operations**. Both helper and walker had **zero missing, extra, stale or denied entries** at first/live/closed/corrupt-history checkpoints. Closed restart catch-up: helper 26.728 ms before / 29.144 ms after; walker 10.577 ms before / 13.864 ms after. These tiny fixture times show correctness, not a speedup. [Final helper](benchmarks/v2-stage-a-final-closed-helper.json), [final walker](benchmarks/v2-stage-a-final-closed-walker.json), [before helper](benchmarks/v2-stage-a-before-closed-helper.json), [before walker](benchmarks/v2-stage-a-before-closed-walker.json).
- Real-C copied-profile closed-change tests independently listed an owned Downloads subtree after renames, moves, edits, parent rename and newly created descendants. **11 expected entries, zero missing/extra/stale/denied**, with helper and full walker. Helper catch-up 1,887.352 ms, priority engine work 126.680 ms; walker 182,765.685 ms under concurrent compilation, priority engine work 287.259 ms. The rest of C: was not independently ground-truth certified. Denied directories remain visibly counted. [Fixture metadata](benchmarks/v2-stage-a-drive-closed.json), [helper truth](benchmarks/v2-stage-a-drive-closed-helper.json), [walker truth](benchmarks/v2-stage-a-drive-closed-walker.json).
- Native preview regression: a real 1,200 x 800 bitmap produces a 768 x 512 preview, with every pixel checked for correct BGRA red and no dark frame; folder artwork is at least 256 px. Existing BGRA behavior is preserved. The desktop preview fixture is a real image; other media extensions in menu/filter fixtures are not proof of media decoding.
- Actual native Properties window and the Windows OpenWith.exe launcher were observed. Choices within every possible Open with dialog were not exhaustively tested.

Earlier failed UI/harness reports remain alongside the final passing reports. They are not counted as acceptance passes. The independent final visual review accepted the scoped popup legibility and toolbar/settings alignment fixes.

## Largest view measurements

Existing fixture: **1,001,008 entries**, top 200, three warm-ups and 30 samples, index-only timed queries with no filesystem scan. Largest files median **48.806 ms**, p95 **57.750 ms**; largest folders median **5.774 ms**, p95 **6.479 ms**. Compilation was running concurrently. These are specific Largest-mode measurements, not Stage C general-search guarantees. [Files](benchmarks/v2-stage-a-largest-million-files.json), [folders](benchmarks/v2-stage-a-largest-million-folders.json).

## Limits and next step

Fresh sustained foreground idle sampling was **not rerun**, because the user explicitly asked to stop repeated testing. The v1 zero-CPU/redraw measurements in PROGRESS.md are historical, not certification of this Stage A artifact. The short Gallery diagnostic is not a qualified idle pass.

Appearance was checked on the available 125% SDR display. Physical HDR, mixed-DPI movement and true cold-cache launch remain unverified. Popup translucency uses GPUI's existing whole-window acrylic plus translucent neutral tints; this pinned backend provides no separate within-window backdrop-blur primitive.

GPUI stays at `279fe070bb389b79652e52065b2f001edcc0b11b`, Kit at 0.7.1, and the patched Windows crate stays pinned. The local Kit menu patch adds optional row height, scrollbar visibility and anchor-aware submenu arrows; provenance is in [SEEK-PATCHES.md](../vendor/gpui-component/SEEK-PATCHES.md). The installed helper is unchanged (SHA256 `151539B785DBAF56E80AC8F403BCF383988A48762FE8A13B47F21652700D9560`). The desktop remains ordinary-user, with no tray, global hotkey or autostart; closing quits.

**Next: Stage B in the next authorized run.** Its OLE drag experiment and bounded content search have not started. Stage C and packaging remain outside scope. No additional tests are being run for this closeout.

## Reproduction commands (already run; not rerun at closeout)

```powershell
cargo fmt --all -- --check
cargo test --workspace --all-features --locked --offline
cargo clippy --workspace --all-targets --all-features --locked --offline -- -D warnings
cargo check --workspace --locked --offline
cargo build --workspace --release --all-features --locked --offline
./scripts/check-v2-stage-a.ps1
./scripts/check-v2-stage-a-drive-closed.ps1
```

The desktop script includes the separate clipboard/Explorer check. File-operation tests create owned disposable fixtures. The real-drive script copies the saved profile and mutates only its uniquely named Downloads fixture; it does not modify the ordinary profile or the Windows journal. Startup measurements use `scripts/measure-drive-startup.ps1`; raw JSONs retain exact sample metadata.
