# Vendored engine provenance

Source: https://github.com/threehymns/fsearch-crossplatform

Pinned commit: `16eaebd89b8db161e4a5cac961f746565b878686`.

Checked the repository's `main` head again on 2026-10-11: it matches this pin.
The latest merge landed at 2026-10-10 21:49:20 UTC, so the recent speed-lead
update is already included. Evidence: `docs/benchmarks/upstream-check.json`.

`src/index.rs`, `src/query.rs` and `src/live.rs` started as verbatim copies of this commit. Windows-specific changes are described in this repository's commits. `upstream/engine.rs`, `upstream/walk.rs` and `upstream/fsevents_linux.rs` preserve the unmodified platform-driver reference; they are not compiled on Windows.

Keep LICENSE with redistribution: MIT, copyright 2026 Noah Dunnagan and threehymns. No FinderSearch code is used.

Real-data audit fix (2026-10-11): typo word-start masks and the existing
one-edit matcher now include underscores and hyphens alongside spaces.
`lambanual` previously missed `CS23532_CN_Labmanual_2026-27.pdf`. Only
candidate distinct names are scanned; the bitmap/matcher architecture remains.
Index magic advances to `SKIDX002`, forcing older snapshots to rebuild rather
than silently retaining incomplete word-start masks. Old benchmark timings
predate this change; no new latency claim is inferred.

Stage 2 adds Windows-only `ntfs`, `kernel`, `helper` and helper synchronization
modules around the existing index/live/query core. Journal cursors are stored
in an optional atomic index footer; portable flat-section offsets and subtree
layout remain intact. Raw-record fixups remain strict; the separate checked
FSCTL parser is a candidate correction awaiting a real-driver retest after
updating the earlier installed service binary. No new NTFS performance claim.
