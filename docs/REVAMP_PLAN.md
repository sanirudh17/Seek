# Seek revamp execution and verification ledger

Branch: `revamp/glass-quicklook-panes`. Worktree: `../Seek-revamp-glass-panes`. Base: `4453afd` (published 1.0.2). All development and fixtures stay in this worktree; reference clones beside it are read-only. No push, tag, publication or version bump is authorized. One commit per numbered implementation item. An item is verified only after build, run and the checks below; source-only work stays unverified.

## Binding direction

Operate/Read native Windows UI, Rust/GPUI with existing pins and vendored crates. Low visual variance, moderate transient motion, medium-high density. Monodrive/Monocode structure, Zeron frosted surfaces, Tilde interaction references. Translate principles to GPUI; no HTML/CSS/GSAP. File correctness outranks speed, memory and polish. Dark-only glass, opaque Light, Windows high-contrast/disabled-transparency fallback. PDF appearance stays; Word remains text. PowerPoint becomes details-only. Do not modify Carbon; report the possibly shared startup flash.

Impeccable and Taste installed project-locally. The comprehensive user brief supplies discovery answers: record shape before each major area, critique after, audit/polish/document at completion. The skills' web/mobile platform classifiers do not represent native Windows, so apply native GPUI constraints from this brief instead of generating web output.

## Required order and acceptance

| Item | Work | Verification gate | State |
| --- | --- | --- | --- |
| 1.1 | ZIP updater separator and packaging safety | Published ZIP extraction, slash/backslash/mixed and malicious archives, finished ZIP structure check | Built/run; compatibility verified; reported incident unreplicated. See 1.1-UPDATER.md |
| 1.2 | Observable/recoverable preview host | Rotating logs, handshake mismatch, cold/normal deadlines, retry/cancel, hidden self-test, host timings | Built/run; 77 tests, 20 UI checks/theme; production cause unverified. See 1.2-PREVIEW.md |
| 1.3 | Hidden startup/backdrop/first-frame reveal | Native first-frame ordering, dark/opaque startup captures, fade respects animation preference | Native ordering/launch checks passed; visual and idle acceptance outstanding (1.3-STARTUP.md) |
| 1.4 | Glass tokens/background/controls | Bright-background legibility, transparency-off and high contrast, before/after measurements | Implemented partially; final accessibility, transient and measurement gates pending |
| 1.5 | Remove partial PowerPoint preview | All presentation extensions details-only, PDF unchanged, Word text retained | Implemented and native checked; association launch/enlarged handoff unverified - see revamp/1.5-PRESENTATIONS.md |
| 1.6 | Enter folder search results | Double click/Enter/current tab, Ctrl+Enter/new tab, back/forward | Routing implemented; live acceptance pending |
| 1.7 | Restrained event-driven motion | All named transitions, animation-off, no idle render loop, idle CPU measurement | Tabs/icons/alignment partial; broader motion and controlled idle pending |
| 2.1 | Quick Look | Focus-safe Space/Esc, mouse/menu, file/page keys, image zoom/Fit/100%, supported/details-only types | Foundation implemented; GUI and full-resolution zoom acceptance pending - see revamp/2.1-QUICK-LOOK-BRIEF.md |
| 2.2 | Inspector | Atomic content handoff, multiple-selection summary, asynchronous totals, correct metadata/actions | Metadata/selection totals/rest/cancellation/actions implemented; 91 tests and strict lint pass. GUI/idle/mutation acceptance and operation-dependent grouping remain pending; see revamp/2.2-SELECTION-INSPECTOR.md |
| 2.3 | Code/Markdown and safe text editing | Syntax/line numbers/find, Markdown/tasks, atomic save/conflict/encoding/EOL/BOM/undo failure paths | Implemented with Auto-save/backup settings; 104 tests and owned save/undo review passed. Latest UI/keyboard/idle acceptance pending; see revamp/2.3-TEXT-READER.md |
| 2.4 | Command palette | Ctrl+K, sidebar Jump, pages/commands, keyboard focus and navigation | Implemented; native launch/open observed, full routing/focus/idle acceptance pending; see revamp/2.4-COMMAND-PALETTE.md |
| 2.5 | Operations/undo | Crash-safe journal, recycle Put Back, move/copy/redo/conflicts/progress, no overwrite rename | Partial: durable journal/archival, cancellation, file/folder/stream snapshots, rename and multi-root move undo/redo, copy undo/exact-version redo and recycle Put Back/redo implemented; 154 tests, strict lint and build pass. Cross-volume source implemented but genuine second-volume acceptance unavailable; read-only reconciliation and New Folder with Selection implemented; final acceptance pending; see revamp/2.5-COPY-BULK-UNDO.md |
| 2.6 | Batch rename | Live replace/number preview, conflicts, skip/apply, rollback/error coverage | Implemented; 162 tests, strict lint and build pass. Combined native UI/idle acceptance pending; see revamp/2.6-BATCH-RENAME.md |
| 2.7 | Search upgrades | Existing engine regression checks, filter chips/saved/recent, five matching lines/truncation | Implemented; 164 app and 68 engine tests, strict lint and build pass. Final native routing/visual acceptance pending; see revamp/2.7-SEARCH.md |
| 2.8 | Smaller interaction items | Favorites shortcuts/hints, recents, capacity, thumbnails, drag feedback/hover/cancel | Implemented; 166 tests, strict lint and build pass. Existing thumbnail providers preserved; final native image/drag/idle acceptance pending; see revamp/2.8-INTERACTIONS.md |
| 2.9 | Saved commands | Safe argv placeholder expansion and selection, context/palette execution | Implemented; 170 tests, strict lint and native exact-argv helper pass. Final GUI acceptance pending; see revamp/2.9-SAVED-COMMANDS.md |
| 2.10 | Settings redesign | Every existing option retained, new pages/search/live controls, keyboard and contrast | Eleven functional pages implemented, including Previews, Saved commands and Keyboard shortcuts; 170 tests, strict lint and build pass. Final native acceptance pending; see revamp/2.10-SETTINGS-FINISH.md |
| 2.11 | Documentation/shortcuts sheet | README/PRODUCT/DESIGN/changelog/license alignment and in-app keyboard sheet | Implemented; generated 31-entry shortcut sheet checks clean. See revamp/PHASE2-COMPLETION.md for final implementation and coverage record |
| 3.1 | Split panes | Independent tabs/path/filter, dividers/file/tab DnD, persistence, Alt+F5 | Pending |
| 3.2 | Drag-to-split animation | Edge zones, accent preview, ~150ms, leave/Esc cancellation, drop settle and motion-off | Pending |
| 3.3 | Workspaces | Named independent layouts persist and restore safely | Pending |
| 3.4 | Sidebar/pinned/live groups | User group DnD, pinning, live saved-query updates | Pending |
| 3.5 | Column view | Fourth view, keyboard/path navigation, selection correctness | Pending |
| 3.6 | Status/shelf/docked document | Persistent statuses, staging shelf, pinned code/Markdown document beside browsing | Pending |
| 4 | Optional organization review | Mandatory review before changes, failure paths; only after required items | Not started |

## Phase-two checkpoint, November 7

All 2.1-2.11 implementation scope is delivered. Final app suite: 173 passed, four ignored; engine regression: 68 passed, one ignored; strict lint and locked/offline build pass. Latest review adds searchable palette groups, focused menus, aligned command arguments, real process results, compact history, keyboard spacing and a clean wider sidebar. [Completion evidence and coverage boundaries](revamp/PHASE2-COMPLETION.md) supersede the historical per-item partial counts above. The user explicitly requested no further app launch after final grouping changes: final rendered/idle acceptance is not claimed. Genuine cross-volume and HDR/mixed-DPI coverage remain unavailable. Phase three has not started; implementation completion must not be relabelled fully verified acceptance.

## Measurement policy

Retain unchanged 1.0.2 baseline privately; use owned profiles/fixture files, not the production index. Measure startup, idle CPU, binary size, selected PDF first host and repeat-page times before/after major changes. Distinguish process/worker timing from painted UI and uncontrolled cache from true cold-system-cache results. Record hardware/platform limitations explicitly; do not infer verification from a successful build or a screenshot filename.
