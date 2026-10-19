# Task 1: sidebar population

Completed 2026-10-18, before starting Task 2. All app tests used separate temporary data folders. The real Seek data folder was not used by any test. GPUI, Kit and the patched Windows crate are unchanged.

## Evidence and cause

Two actual before-fix reproductions use the previous release with isolated data:

1. Search contents successfully, then click a sidebar location: the input clears but **Content mode remains enabled**, so the backend receives an empty content query instead of a folder browse. [Before report](benchmarks/task1-sidebar-before.json).
2. In normal mode, click Downloads while the test index covers only an owned fixture: **zero rows despite independently listed Downloads files**. The backend only used `engine.children` after the global `ready` flag; missing directory coverage and genuinely empty directories both became an empty vector. [Before report](benchmarks/task1-sidebar-unindexed-before.json).

The old first-index path also published its namespace only after the full walk/build. Code inspection confirmed that the same global readiness gate prevented earlier sidebar directory data from appearing. Redirected paths were resolved through the Known Folder API already, but a duplicate resolver guessed `USERPROFILE/<name>` after API failure. That fallback was not a valid redirected-location guarantee.

## Fix

Sidebar navigation exits Content and Largest modes, clears stale rows, and immediately shows **Indexing this folder...**. Finite metadata workers enumerate complete direct children independently of global indexing. Downloads, Desktop, Documents, Pictures, Videos and Music are prefetched in that order. Demand reads do not block the UI or wait for the full-drive index. A bounded metadata cache retains at most eight folders / 32,000 entries across cached folders (one larger active folder is kept complete, never truncated). Ordinary index notifications take over once a directory is published. Navigation and Seek file operations invalidate fallback snapshots.

One shared resolver now uses **SHGetKnownFolderPath for the current ordinary-user token**, including OneDrive redirection. Missing/unresolvable folders are omitted, without guessed profile paths. Production source/default settings contain no fixed username or per-user absolute path; the new harness resolves its driver through LOCALAPPDATA. Historical reports and generic parser test fixtures are not runtime defaults.

Completed-empty and failed reads have distinct states: **This folder has no visible items.** or **Folder unavailable: ...**. Hidden/system settings and exclusions are applied consistently. No top-200 truncation is used for folder browsing. Metadata enumeration never opens document content streams.

## Verification

- **3 new Rust/integration checks pass.** They cover complete direct-child truth, case/trailing/extended-prefix normalization, empty versus missing, an unindexed folder, an owned redirected junction, a non-C letter and an unfinished first global index.
- A fresh backend with 10,000 owned subdirectories delivered the selected folder's **2 complete rows in 7.403 ms while `index_ready=false`**. This is a debug in-process measurement, not a universal latency promise.
- A no-settings/no-index UI run compares each current Windows Known Folder with independent `Get-ChildItem` output: **Downloads 539, Desktop 18, Documents 133, Pictures 15, Videos 7, Music 1**, all **zero missing/extra visible entries**. Desktop, Documents and Pictures resolved into this user's OneDrive; Music resolved to a separate redirected folder rather than a guessed profile path. The location root also matched **11/11** entries. [Raw folder matrix](benchmarks/task1-sidebar-cold.json).
- The non-C UI drive location matched **4/4** independently listed entries through an owned temporary **Z:** SUBST alias, removed after the test. Only a physical C: volume is available, so a separate physical disk is not certified. [Non-C evidence](benchmarks/task1-sidebar-non-c.json).
- UI empty-folder, leaving Content mode, leaving Largest mode and closing checks pass. [State checks](benchmarks/task1-sidebar-empty-modes.json).
- Prefetch metadata reads for the six real Known Folders took **0.143-3.354 ms each** in the retained debug sample. These measure enumeration, not complete click-to-paint latency. The UI run's index happened to finish before automated clicks; the separate backend test explicitly verifies publication before global readiness.
- Strict app all-target/all-feature Clippy, normal-feature check and formatting pass. Earlier harness startup/empty-label assertions and a mixed-slash junction command were corrected; failed artifacts are not acceptance passes.

No broad Stage A suite or real-profile launch was repeated. Physical HDR/mixed-DPI and a second physical volume remain unverified. Next: Task 2 only. [Machine-readable summary](benchmarks/task1-sidebar-summary.json).
