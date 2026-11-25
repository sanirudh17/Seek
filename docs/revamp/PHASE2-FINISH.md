# Phase two completion run

The user has requested completion of all phase-two items before phase three, without intermediate approval pauses. Do not end work merely because another narrow increment passes. Keep the isolated worktree, pinned native stack, approved Figtree/Hugeicons/glass presentation and version 1.0.2. No publication.

Completion batches, in order:

1. Finish 2.5: general file/folder/stream snapshots, cancellation, multi-item/copy/cross-volume undo and redo, recycle redo, reconciliation, and New Folder with Selection.
2. Implement 2.6: replace/number batch rename preview, conflict handling, skip/apply and rollback/error paths.
3. Implement 2.7–2.8: search chips/saved/recent queries/five context lines, favorites/recents/capacity, thumbnails and drag feedback/hover/cancel.
4. Implement 2.9–2.10: argv-based saved commands with explicit invocation, dependent preview/command/keyboard settings and complete option coverage.
5. Close 2.1–2.4 acceptance gaps and 2.11 documentation/shortcut sheet, then run the complete phase-two regression and native GUI checkpoint with owned data. Do not equate source-only checks with GUI verification. Report host limitations precisely.

Only after those gates may phase three begin. The planning estimate given to the user is several focused development/test sessions, likely days rather than a small polish pass; this is not a promised deadline.

Copy/bulk continuation: copy undo and exact-version redo, redo recycling, actual restored-path capture, deferred whole-directory copy/move proofs, multi-root move undo/redo with complete preflight, and legacy checksum compatibility are implemented. 154 tests and strict Clippy pass; see `2.5-COPY-BULK-UNDO.md`. Real cross-volume acceptance is unavailable on this single-volume host. Reconciliation and New Folder with Selection remain next in 2.5; keep working through the remaining batches.

Grouping and explicit read-only interrupted reconciliation are now implemented; 156 tests and strict Clippy pass. Grouping undo returns selected items and retains the created folder, as stated in the name sheet. Next is 2.6 batch rename, with final operation/native UI acceptance deferred to the combined checkpoint. Do not stop at this increment.

2.6 batch rename is implemented with asynchronous replace/number preview, explicit conflict skipping, complete preflight, no-replace staging, guarded undo/redo and best-effort rollback. 162 tests, strict Clippy and build pass; see `2.6-BATCH-RENAME.md`. Continue with 2.7 search upgrades, then the remaining batches; full UI acceptance remains pending.

2.7 saved/recent mode-aware query persistence and five-line content evidence are implemented; 164 app tests, 68 engine tests, strict lint and build pass. See `2.7-SEARCH.md`. Continue 2.8 interactions, 2.9 saved commands, 2.10 dependent settings, 2.11 docs/shortcuts and final acceptance. No small review launch is needed.

2.8 favorites shortcuts/hints, background local capacity/recent evidence, valid-drop feedback and cancellable delayed list-folder expansion are implemented; 166 app tests, strict lint and build pass. See `2.8-INTERACTIONS.md`. Continue saved commands/settings/docs and final native acceptance rather than ending here.

2.9 Saved commands settings, explicit context/palette invocation, selection guards and direct argv expansion are implemented; 170 app tests and strict lint pass. See `2.9-SAVED-COMMANDS.md`. The exact-argv native helper passes Unicode/metacharacter checks. Next: complete dependent Previews and Keyboard shortcuts settings, shared shortcut sheet/docs, then full native/idle acceptance. Preserve the approved native material/animation implementation.

2.10 dependent settings are implemented (eleven functional pages), including live automatic-preview/thumbnail controls with default-on behavior and a shared keyboard reference. 170 tests, strict lint and build pass; see `2.10-SETTINGS-FINISH.md`. Before final acceptance, close the 2.1 original-image-pixel gap (current 100% still uses bounded preview pixels), complete 2.11 generated shortcut/docs alignment, and run native/idle verification. Do not mark phase two complete until those gates are actually resolved or explicitly reported as unverified host limits.

Current implementation detail: new operation fingerprints cover local regular files without the old 16 MiB cutoff, directory topology/child identities, and named data streams. Normalized content fingerprints support copy/cross-volume validation while identity fingerprints detect replacements. Legacy primary-stream-only records retain their old validation semantics. Links/offline data are refused; directory traversal has a stack-depth guard. Cancellation is scoped to each queued request and checked while hashing, enumerating directories and at native pre-operation/progress callbacks. Stop does not roll back already completed items. Native tests exercise directory rename/move inverse operations and directory Put Back with a large file and named stream. Snapshot/cancellation foundation: 149 tests passed, strict Clippy passed; broader operation work remains open.

Final implementation checkpoint (November 7): all numbered phase-two code/documentation scope is delivered, including bounded original-image pixels and final user menu/group refinements. App suite 173 passed/four ignored, engine 68 passed/one ignored; strict lint and build pass. See PHASE2-COMPLETION.md. The latest explicit instruction prohibits further app launches, so final native visual/idle gates are recorded as not rerun. Do not launch an approval session or claim these unexecuted gates passed. Phase-three implementation has not started.
