# Task 3: selected-document info-panel previews

2026-10-18. Previews are confined to the existing right-hand inspector. List/grid rows retain association icons. Nothing hosts a Windows preview handler or creates an Office automation object. No Stage C or packaging.

## Behavior and boundaries

- **PDF:** Windows `Windows.Data.Pdf.PdfDocument`, loading bytes into an in-memory stream and rendering one selected page. Previous/next controls display the actual page count and disable at the ends. Native DPI-scaled output is normalized to a maximum 768-pixel edge; the UI confines the aspect-preserving page above its controls. This rendering does not require the PDF text filter used by content search.
- **Excel `.xlsx`:** pinned **calamine 0.36.1**, pure Rust and read-only. The first worksheet appears as a simple table with cell coordinates. The panel explicitly says **data only, no formatting**. It shows up to 100 used rows, 12 columns and 256 characters per cell, with shown/total counts. Formula values are cached workbook values, never recalculated; no formulas, macros, external links or embedded objects are executed. Empty sheets have an explicit empty state.
- **Word `.docx`:** a large `IShellItemImageFactory` thumbnail-only request. An association icon or blank cached thumbnail is not accepted as document artwork. If unavailable, reuse the existing bounded DOCX XML extraction and show the beginning: at most 100 lines / 12,000 characters, explicitly text-only. The owned test corpus exercises both a nonblank package thumbnail and a package without a thumbnail. Shell artwork can reflect the document's saved/cached thumbnail; it is not a promise to re-layout Word pages.

Each selection/page starts one finite child host under the existing parent-owned Job object, with a **32 MiB input cap, 64 MiB expanded Office-package cap, 8 MiB response cap, 256 MiB child committed-memory limit and 4-second parent deadline**. Native calls and parsers run off the UI thread. A stuck native provider/parser is killable without waiting for its thread to return. Changing selection, closing the panel or exiting invalidates the generation and cancels the host. Both input metadata and actual read length enforce the cap; cloud-only/recall placeholders and links are rejected.

The main app retains one displayed bitmap/table/text excerpt. Replaced/closed images are removed from GPUI's image cache on the next render, replies from obsolete generations are rejected, and the child is waited/reaped. The worker blocks on its channel when idle; its short deadline polling occurs only while a child is active. No document watcher, persistent content cache or always-running child is added.

GPUI, GPUI Kit, patched Windows backend and Windows crate versions are unchanged. Only Windows crate feature flags were added for built-in PDF APIs; calamine and its eight locked dependencies are new. Existing content extraction, row icons, native menus and file-operation behavior are preserved.

## Measured validation

All apps use fresh owned `.bench-data` data folders. Diagnostic hosts/audits exit before normal settings/profile initialization. The real Seek data folder is not used.

**16/16 actual-host integration cases pass.** Two-page PDF rendering and out-of-range rejection, vector-only PDF rendering, first-sheet truth, 100-of-200-row / 12-of-15-column truncation, empty sheet, meaningful Word thumbnail, Word text fallback, broken DOCX/PDF/XLSX, an actually encrypted PDF, oversized input and cancellation are included. The Excel known-answer fixture includes accented text, numeric and boolean cells, and a second sheet that must not appear.

Final debug sample: first/second PDF pages **258.184 / 287.201 ms**, each **576×768**; small first-sheet table **70.749 ms**; Word text fallback **227.021 ms**; real nonblank Word thumbnail **248.716 ms**. Cancellation returned in **13.565 ms**. A separately forced five-second host stall is terminated by the parent in **4,015.229 ms** with `Preview timed out`. These are one-sample owned-fixture timings, not cold-launch or general-document latency claims.

Desktop checks: **14/14 Dark** and **13/13 Light** pass, covering page stepping, spreadsheet data, Word fallback, broken/encrypted states, selected-item reload, clearing on panel close, no remaining preview child and full app exit. The Dark run also verifies a nonblank Word thumbnail. Early harness startup/edit-name assertions were corrected; failed runs are not counted as passes. Initial image overflow and fixed-column clipping were corrected and recaptured.

Impeccable finish review: **ship**, scoped to seven supplied final 125% SDR desktop captures and the new preview source. It confirms confined pages/controls, compact table, readable Word text, data/text disclosures and unchanged list icons. Loading/error appearances were verified by desktop assertions; the reviewer inspected those states in source. The documenter preserved the incumbent design system. Existing historical product/design wording about content limits was reported as drift and left outside the write boundary; current behavior is recorded in PROGRESS and the task reports.

Optimized build, strict Clippy, normal-feature check and formatting pass. Production executable: **16/16 real-host document cases, 10/10 existing content accuracy cases and 5/5 focused desktop smoke checks pass**. SHA256 `8691609826B7E9013BEA441088BAFA1939630D047842AF7EC852D3B0F62986A7`, **19,076,608 bytes**.

Selected-PDF foreground idle, debug build: **30.079 seconds, 0 ms process CPU and 0 redraws**, after 45 seconds of finite startup/thread-pool settling. Two native activity events split the longest strictly quiet foreground segment at **19.671 seconds**. The inherited harness's 20-second strict-quiet gate is therefore **false**, retained explicitly; there is no uninterrupted-30-second claim and no repeated idle loop. All sampled intervals stayed foreground and recorded zero CPU/redraws. Windows process CPU counter resolution applies. Earlier samples with mouse/focus activity and one-time cleanup were not accepted as zero-idle evidence. Selected-PDF private commit **88.89 MiB**, private working set **38.97 MiB**; concurrent compilation can affect the resident working set. Panel-close checks clear the model/image and leave no preview host.

[Full measured summary](benchmarks/task3-preview-summary.json). API references: [Windows PDF loading](https://learn.microsoft.com/en-us/uwp/api/windows.data.pdf.pdfdocument.loadfromstreamasync) and [calamine](https://docs.rs/calamine/0.36.1/calamine/).

## Limits and check by eye

Check real PDFs with different page sizes, step pages, and verify the controls never overlap the page. Check a representative Excel sheet's values/coordinates and the explicit data-only/truncation wording. Check Word files with and without useful saved thumbnails, switch selections quickly, then close the panel. Review each in Light/Dark/glass.

Only the available **125% SDR** desktop was visually inspected. Physical HDR, mixed-DPI moves, other monitors and a machine with Office uninstalled are not certified. No Office instance is required by the implementation; the real no-thumbnail fallback is verified on this machine, which has Office associations installed. Actual encrypted Word/Excel specimens and provider behavior on other Windows installations were not independently checked. Legacy `.doc`/`.xls`, rich Word layout, spreadsheet formatting and OCR are outside the requested previews.

Fixture reproduction uses Python test-only dependencies `python-docx`, `openpyxl`, `Pillow` and `pypdf` (the latter can be installed under `.references/task3-python`). Run `python scripts/make-document-preview-fixture.py .bench-data/document-preview/files`, then `scripts/check-document-preview.ps1` with an explicit executable. Every run creates separate temporary app data. The benchmark-enabled executable also accepts `--document-audit .bench-data/document-preview/preview-manifest.json` without initializing an app profile.
