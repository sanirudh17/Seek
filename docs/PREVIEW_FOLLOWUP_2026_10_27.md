> Historical implementation report. Earlier build holds are superseded by the approved [1.0.2 release](RELEASE_1_0_2.md).

# Preview and navigation follow-up — awaiting app-build approval

The changes below are source and test-harness verified. The normal debug/release app, installed program and release assets were not rebuilt or replaced. No version bump, installer, publication or framework update was performed.

## Preview request lifetime

The document worker previously enforced its four-second timeout while waiting for the host, then called a reader thread's unbounded `join()`/`read_to_end()`. If another process retained the inherited stdout write handle, the host could exit successfully while the reader waited indefinitely for EOF. That prevented subsequent preview requests from completing. An owned descendant-process regression reproduces the problematic handle lifetime; the new reader returns the completed host result without waiting for that descendant to exit.

The worker now reads only available pipe bytes on its existing worker thread, checks cancellation/deadline during collection, and drains the host's final bytes after exit. There is no reader thread to join. Key/generation snapshots are taken together. UI preview requests are also reconciled with restored/programmatic selections. An event-driven six-second UI guard ends a stranded loading state with a Retry action; its task is cancelled when a reply arrives. Existing selected-item, 32 MiB file, four-second host and 256 MiB host-memory boundaries remain. The output cap is 16 MiB to allow the larger raster.

The exact production incident was not reproduced in a fresh temporary-profile GUI run: the already-installed app rendered the fixture PDF. Its existing native host also rendered the user-selected screenshot PDF (15 pages) in 672 ms. This narrows the incident but does not prove that inherited stdout was its only trigger. The source regression and deadline gap are independently verified.

## Enlarged preview

An **Enlarge preview** button opens a viewport-sized reading surface. PDFs request a raster edge up to 1536 px instead of 768 px. Existing document, image, video-thumbnail and bounded plain-text/Markdown previews remain available as the selection changes. Up/Down or Previous/Next file changes the selection; Left/Right changes PDF pages or presentation slides. Escape/Close returns to the normal panel. Destructive/background shortcuts are blocked while the enlarged surface is active.

This is a selected-item preview, not a new external viewer. Plain text is limited to 80 lines/8 KiB, Word fallback to 100 lines/12,000 characters, and existing first-sheet Excel limits remain. No video playback or Markdown layout engine was added.

## PowerPoint

Supported modern packages: `.pptx`, `.pptm`, `.ppsx`. The preview first tries the package's saved first-slide image, then a real Windows shell thumbnail; both are explicitly first-slide previews. Slide text is available in presentation relationship order, with page controls and a 24,000-character cap per slide. Later slides never reuse the first slide's artwork. Legacy `.ppt` uses a real shell thumbnail when available and otherwise reports unavailable.

This does **not** reproduce full slide formatting, diagrams or animations on every slide. It does not launch Office, run macros, follow external relationships, host preview handlers or install a converter. The format was researched against [Microsoft's PresentationML structure](https://learn.microsoft.com/en-us/office/open-xml/presentation/structure-of-a-presentationml-document). XML/ZIP, image dimensions and decompressed sizes are bounded inside the existing disposable host.

## Complete All locations

The ready empty-query All locations view previously forced files-only and a 200-result top-k cap. It now includes files and folders, preserves newest-first ordering, reports the full visible total, and provides 500-row Previous/Next pages. Every item is reachable; it is not a single materialized list of millions of formatted rows. Visibility/exclusions are applied before pagination. Only the current page's paths/rows are materialized. The temporary ordinal list uses 8 bytes per eligible base entry; full-drive process memory has not been measured.

During a first incomplete index, existing provisional recent rows remain explicitly labelled as indexing. This cannot promise unindexed contents before enumeration reaches them. Typed filename queries and Largest modes retain their existing explicit result limits. Live changes can change which page an item belongs to.

## Folder addresses

Enter in the search/address field recognizes current-user `%NAME%` variables, suffix paths, quoted absolute/UNC paths and `~`. It uses [Windows `ExpandEnvironmentStringsW`](https://learn.microsoft.com/en-us/windows/win32/api/processenv/nf-processenv-expandenvironmentstringsw), including case-insensitive variables and unchanged unknown references. `%TEMP%`, `%APPDATA%`, `%LOCALAPPDATA%`, `%USERPROFILE%` and user-defined variables use the same path. Folder reads remain on the existing browse worker. No shell command is executed. Cmd.exe substring/replacement syntax is not supported by this Windows API.

## Verification

- App suite: **70 passed**, two ignored at the time of that run (live updater and explicitly selected real presentation). The subsequently added saved-image regression also passed: **71 distinct regular app checks**. The real-presentation check was then explicitly run and passed.
- Strict all-target/all-feature Clippy, normal-feature compilation and formatting pass. Only test harnesses and type-check artifacts were compiled.
- Inherited stdout regression: **213.785 ms** in the full suite; an earlier isolated sample was **72.522 ms**. Includes test-process startup; not UI preview latency. A stalled host also hit its bounded test deadline and was killed.
- Native self-contained two-page PDF panel/enlarged/page/error checks: **1359.716 ms aggregate**. The screenshot PDF passed the same native checks in **1697.820 ms aggregate**. These totals cover multiple render/load/error cases, not first-result or input-to-paint timing.
- The explicitly selected real presentation has **95 slides**. First/last slide text and its **768×432** saved preview decoded in **66.138 ms**, excluding the initial file read. No document text was logged.
- **Two index checks passed**: 640 fixture entries across all pages equal independent recursive directory truth with no missing/duplicate entries; changes after delete/rename/add, exclusions, hidden/system visibility, cancellation and out-of-range paging also pass. Initial full paged comparison: **15.870 ms**, debug test harness, owned fixture.
- Existing installed app and workspace release executable remain SHA-256 `9F735B3AF4A440705DC6FC9250A752B0908924CD0FEA389CEBA792B9D2FBAA32`; workspace normal debug executable remains `992D0CC39E8E1BCB3ABBF0BB6303DD2496A7DFDE90418DCA4E3D02FE5A1F1732`.

All app tests use owned fixture roots and a separate temporary profile. The requested real PDF/presentation were read only. Production Seek saved data was not used. GUI input stopped when the temporary window's selection changed independently.

## Check by eye after an approved build

- Startup PDF, rapid selection changes, a failed PDF's Retry button, and page navigation.
- Enlarge/Close/Escape; Up/Down across PDF, Word, text/Markdown and image; Left/Right across pages/slides.
- Readability, focus, scrolling and sizing in Light, Dark, glass and small/maximized windows.
- Real PowerPoint saved-image/text labels, slide order and blank/image-only slides.
- All locations totals and first/next/last partial pages, tab restoration, visibility and live deletion updates.
- `%TEMP%`, `%USERPROFILE%` and a quoted path with spaces on Enter; unavailable paths show a clear error.

New UI painting/keyboard interactions, production host end-to-end behavior for the changed executable, full-drive paging memory/performance and idle CPU remain unmeasured until that build is authorized. No claim that the user's original production freeze has been visually closed yet.
