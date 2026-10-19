# Seek v2 Stage B

**Current status, 2026-10-19: closed.** The original experiment and limits below are historical. Subsequent authorized work delivered external Copy drag to Explorer and Chromium ([drag follow-up](DRAG_FOLLOWUP.md)), complete streamed scans and previews, quick Windows Search results with an authoritative complete scan, the extracted-text cache and deferred extraction retries, plus the targeted [Settings/panel/access-count fixes](SETTINGS-PANEL-ACCESS-FIXES.md). PROGRESS.md records their current verification and artifact hashes. There is no outstanding administrator acceptance gate for v2 Stage B; the older v1 Stage 2 notes are superseded by the installed, connected ordinary-user helper checks.

No mandatory Stage A/B test loop remains before Stage C. Individual chat-app targets and physical HDR/mixed-DPI displays are unverified coverage; two real PDFs still fail extraction explicitly. No OCR or unconditional document recall is claimed. Stage C (tabs and measured speed polish) begins only on a separate authorization; packaging is not approved.

Scope: bounded external-drag experiment and on-demand content search, plus the requested Info alignment and Light-menu transparency fixes. Stage C and packaging are not included. Stage A's completed correctness/file-operation suites are retained rather than repeated.

## Implementation

Content search is available from the typing suggestions (Content contains) and the File actions menu (Search file contents). A compact Content chip exits the mode. Results show filenames and the matching extracted line; content mode uses List view. Settings > Files and search controls locations, defaulting to Windows' actual Downloads and Documents folders. Saving applies immediately. The selected folder intersects those configured locations and cannot widen them.

Search runs on the existing worker. Every edit advances a cancellation generation; old results cannot replace newer ones. There is no persistent content index, content cache on disk, model, embedding, service or new always-running process. Background filename-index notifications reuse the current content results instead of rescanning files continuously. A new query or F5 performs a new read. Seek file-operation replies invalidate the content results, so renamed or moved files do not retain stale paths.

Supported input includes UTF-8/UTF-16 text, code, Markdown, DOCX document text/headers/footers/notes/comments, and PDFs with a registered Windows text filter. Scanned image-only PDFs are not OCRed. DOCX package parts are read in memory; embedded objects and external relationships are not followed.

Reads are restricted to configured local locations, with exclusions and hidden-file visibility respected. Secret/key paths, including .ssh, .gnupg, .aws, .env, password stores, credential filenames and private-key extensions, are skipped. Reparse points and cloud-only files are skipped. The actual open file handle's resolved path is checked before reading bytes, preventing an intervening parent-path change or junction from widening scope.

Limits: 4 MiB per input file, 8 MiB expanded document text, 30,000 visited entries, 200 matching files and a cooperative three-second query budget. Checks run between files, read chunks and XML events. PDF extraction has its own one-second child-process deadline, subject to process-start/scheduling overhead, and a 256 MiB committed-memory job limit. The host joins its parent-owned kill-on-close job before reading input or loading a filter; closing Seek cannot leave that host running. Individual filesystem calls can exceed a cooperative deadline; this is not a hard real-time guarantee. A small status row reports matches, checked/skipped/unreadable items and reached limits. Its tooltip explains limits and full locations. Unsupported/binary/encrypted/unreadable input and limited scans are not presented as an exhaustive content search.

## PDF method decision

Windows Search was measured before choosing the method. With its existing service running, an owned PDF/text fixture was present in the catalog: the fixture query took **111.925 ms**, and `CONTAINS(System.Search.Contents, ...)` returned the known PDF match in **16.292 ms**. This is a single real-machine sample, not a distribution. [Raw query probe](benchmarks/v2-stage-b-pdf-probe.json), [probe script](../scripts/probe-v2-stage-b-pdf.ps1).

The catalog is fast and reuses Windows' index, but depends on configured indexing coverage and freshness; the query does not provide a literal matching line. Seek therefore uses the registered PDF IFilter directly, loading scope-checked in-memory bytes through IPersistStream in a short-lived child. This finds text in accessible PDFs even when they are absent from Windows' catalog and provides an extracted snippet. A hung filter can be terminated without blocking the UI worker indefinitely. The tradeoffs are per-file extraction/startup cost, filter availability, no OCR and visibly reported limits.

The standalone extraction probe returned the exact known phrase; its elapsed time was **1,092.858 ms** during concurrent compilation. This includes launching the debug executable and is not a warm per-file extraction speed claim. The subsequent small UI content scans, which include a PDF, are recorded separately in their raw reports. An initial filter-class parsing failure is retained; GUID braces were normalized before instantiation. [Extraction probe](benchmarks/v2-stage-b-pdf-extract.json).

API references: Microsoft's [Windows Search CONTAINS](https://learn.microsoft.com/en-us/windows/win32/search/-search-sql-contains), [filter interfaces and stream loading](https://learn.microsoft.com/en-us/windows/win32/lwef/-search-2x-wds-ifilteraddins) , [Windows job lifetimes](https://learn.microsoft.com/en-us/windows/win32/procthread/job-objects) and [Windows filter implementations](https://learn.microsoft.com/en-us/windows/win32/search/-search-ifilter-implementations).

## Original external drag experiment - stopped

**Superseded by the separately authorized [drag follow-up](DRAG_FOLLOWUP.md).** The historical experiment below remains as its original evidence; external file Copy drops are now implemented by a window-thread OLE handoff.

The benchmark-only prototype arms after GPUI's typed row drag threshold, waits for the pointer to leave the source, constructs a shell IDataObject for the selected files, and attempts DoDragDrop with IDropSource on a separate OLE apartment. Only COPY is offered. GPUI/Kit/the patched Windows backend were not changed. The diagnostic flag is `--experiment-drag-out`; it is disabled during normal use and is not advertised as a supported feature.

**No successful external drag is claimed.** An early pointer setup recorded "released before leaving source". After correcting the test thread's DPI coordinates, the source and Explorer windows had distinct recorded bounds, but no OLE start/completion event or copied destination file was observed. The owned source stayed intact. This does not prove that Windows OLE dragging is impossible; it means the attempted GPUI-to-OLE handoff was not cleanly established by this bounded experiment. The experiment was stopped rather than changing the framework's capture/dispatch behavior. Browser and chat drag-out tests were not attempted after the source handoff failed; Teams, Telegram and WhatsApp were installed.

Use **Copy/Cut and Paste** to hand files to other apps. Seek's native file clipboard and actual Explorer paste were already verified in Stage A. Browser/chat clipboard acceptance depends on the receiving app and is not certified by the failed drag experiment. [Final experiment](benchmarks/v2-stage-b-drag-experiment.json), [early pointer setup](benchmarks/v2-stage-b-drag-pointer-setup-failure.json), [probe script](../scripts/probe-v2-stage-b-drag.ps1). Native API reference: [Microsoft OLE drag and drop](https://learn.microsoft.com/en-us/windows/win32/com/drag-and-drop).

## UI finish

The Info toolbar control uses the same centered 14 px SVG wrapper as Hidden and Settings, removing font-baseline positioning from that icon. Glass-enabled Light popovers use `#f1f1f1cc` (80% tint opacity), down from `#f1f1f1e6` (about 90%). Dark remains unchanged. The shared popover token applies to toolbar, typing, filter and row menus. Native whole-window acrylic supplies the backdrop; this is not a new independent blur layer inside each popup. Solid mode is unchanged.

The content additions retain the existing quiet Windows/Finder design, restrained accent and neutral glass. Input/result state is visible without a large technical banner. The final visual and idle evidence is recorded in the Stage B summary.

## Validation and remaining limits

Final source passes **7 new focused Rust tests**, **9/9 targeted debug desktop checks** and **4/4 final optimized-release smoke checks**. The release smoke checks the job-backed PDF host, an external file addition discovered by F5, and complete exit. The first PDF query scanned seven eligible files in **115.794 ms**; the refreshed query scanned eight in **55.471 ms**, both with zero read errors and no reached limit. These are small owned fixtures, not representative whole-Documents latency distributions. [Debug desktop evidence](benchmarks/v2-stage-b-ui.json), [final release smoke](benchmarks/v2-stage-b-release-smoke.json).

The final optimized build completed in **6 min 05 s**, executable **17,971,712 bytes**, SHA256 `72AD5AB678DDF453E158CD60CD2C89153E0528006A3ACFA327084D0E4F2548DF`. Strict app all-target/all-feature Clippy, normal-feature check and final formatting pass. GPUI, Kit and patched Windows versions are unchanged. Added pinned ZIP/XML parsers support DOCX; only Windows API feature flags were added to existing Windows dependencies.

The independent capture reviewer reported no material scoped defect in seven final-source captures: centered Info, readable Light menus with faint underlying content, and compact content suggestions/results/Settings. Coverage is static captures at 125% SDR; this is not physical HDR certification. The separate standalone PDF extraction timing above predates the job-argument safeguard; final desktop and release-smoke checks exercise the guarded route.

All reparse points are conservatively skipped, including provider-backed placeholders even if cached. This protects scope but limits content coverage; skipped counts remain visible. The user-supplied [Files repository](https://github.com/files-community/Files) was inspected as a Windows file-manager reference at closeout; no source was imported and it did not expand Stage B.

Seven focused new regression tests pass, including backend file-operation/cache refresh and termination of an owned child on job close. The other five cover: secret/key exclusion; scope/cancellation/Unicode lines; DOCX runs/entities; real NTFS junction/exclusion isolation; oversized input and DOCX expansion. Strict app all-target/all-feature Clippy, formatting and the normal feature configuration are checked separately. Stage A's 91 Rust tests and 30 desktop checks remain their original evidence, not a repeated Stage B count.

Early desktop reports retain test-driver failures: compression assembly loading, first-trace readiness, matching an edit's value as part of its name, selecting a folder from All locations' file-only results, and an artifact built before the final text refinements. They are not counted as passes. Later checks target the remaining steps rather than redoing the Stage A suites. The Settings Done control also closes directly instead of delegating through general Escape handling.

Physical HDR, mixed-DPI movement and true cold-cache launch still require a suitable setup. The no-helper full-C catch-up limitation from Stage A is not altered by Stage B. See [PROGRESS.md](../PROGRESS.md) and [Stage A](STAGE_A.md) for the prior timings and coverage. No packaging is authorized.

## Final outstanding Stage A idle follow-up

One attempt per saved fixture on the final release; no repeated broad Stage A suites. 10,000 files: **29.992 s** quiet foreground, **0 ms CPU / 0 redraws**, private working set **23.88 MiB**, private commit **79.23 MiB**, close **419.377 ms**, exit 0. 1,000,000 files: **27.306 s** quiet foreground, **0 ms CPU / 0 redraws**, private working set **38.65 MiB**, private commit **83.76 MiB**, close **329.533 ms**, exit 0. Windows CPU-counter resolution applies. Foreground/input qualification and all raw intervals are retained; zero values are measured outcomes, not claims of impossible activity between samples. [10k sample](benchmarks/v2-stage-b-idle-10000.json), [million sample](benchmarks/v2-stage-b-idle-million.json), [summary](benchmarks/v2-stage-b-summary.json).

Stage B is closed. The next optional stage is Stage C, only after separate authorization. No packaging.
