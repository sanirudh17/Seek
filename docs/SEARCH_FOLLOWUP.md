# Stage A/B search and acceptance follow-up

Closed 2026-10-18. Authorized scope: audit remaining Stage A/B checks, correct the reported Name contains behaviour, measure content-search delay and document accuracy, and assess preview feasibility. No Stage C or packaging.

## Changes and measured results

**Name contains:** the prior suggestion generated a regex-only query (`re:"t3"`). Such queries sort by modification time, so an exact filename or prefix could fall below a recent hash containing the same letters. The new literal `name:"t3"` predicate uses normal relevance scoring, preserves exact/prefix priority and still requires the literal in the filename. The UI also accepts the visible default Name suggestion on Enter instead of opening the currently selected file. A real indexed regression checks exact/prefix ordering, unrelated-parent exclusion and literal punctuation; final desktop checks confirm both plain typing and Enter retain `t3` first. This is a ranking/keyboard correction, not evidence that every old screenshot result lacked the substring.

**Content latency:** the search worker now publishes the first match immediately and later batches at most every 80 ms. The existing generation checks discard cancelled queries; the final scan still reports reached limits and unreadable/skipped input. Cheap text and DOCX files precede PDFs within each bounded directory batch. No persistent content cache, watcher, model or new resident worker was added.

The final release searched **the configured real Downloads/Documents locations** for `note`: first matching row **120.049 ms**, final scan **3,023.163 ms**, **154 checked, 47 matches, 495 skipped, 1 unreadable**. The three-second cooperative limit was reached: these are partial results, not a complete count or a whole-folder recall certification. Format work totals include **86 DOCX / 1,193.271 ms** and **7 PDF / 754.551 ms**. File enumeration/opening and other formats account for the rest. One machine/sample with existing OS caches; no percentile or instant-completion claim. The old UI would wait for the bounded scan to finish; the new UI can show rows while it continues. [Sanitized real-folder measurement](benchmarks/content-real-release-summary.json) records counts/timings without real filenames or snippets.

In the owned debug fixture, PDF-last ordering reduced the first matching row from **430.748 ms to 2.292 ms**; all known-answer cases still passed. Final release accuracy runs vary: the first query found a match in **23.109 ms** while desktop checks ran concurrently; subsequent text/DOCX cases first matched in **1.39-1.81 ms**. These are small fixtures and separate samples, not a controlled universal speedup. Final desktop traces confirm an actual partial UI update before scan completion. [Release UI](benchmarks/search-followup-ui-release.json).

**PDF accuracy:** respect the Windows filter's chunk boundary type instead of inserting a newline between every chunk. No-break chunks join directly; word/sentence breaks use a space; paragraph/chapter breaks use a newline. Preserve UTF-16 across GetText buffer boundaries before decoding. DOCX XML handling also accepts CDATA. Microsoft defines the contract in [CHUNK_BREAKTYPE](https://learn.microsoft.com/en-us/windows/win32/api/filter/ne-filter-chunk_breaktype) and [GetChunk](https://learn.microsoft.com/en-us/windows/win32/api/filter/nf-filter-ifilter-getchunk).

## Accuracy and focused validation

**10/10 known-answer content cases pass** on the final release with exact expected filename sets and no reached limit: DOCX formatted/split runs, Unicode case, literal punctuation, table cells, headers and footers; PDF split text operators and second-page text; metadata-only and absent-token negatives. A filename-only DOCX match is excluded. Two deliberately broken files are counted unreadable on each scan; legacy `.doc` is skipped. [Raw accuracy cases](benchmarks/content-accuracy-release.json), [fixture generator](../scripts/make-content-accuracy-fixture.py).

This is a finite synthetic corpus, not a claim of perfect extraction from arbitrary Office/PDF files. DOCX support reads document XML, headers/footers and supported notes/comments. Legacy binary `.doc`, image-only/scanned PDF OCR, encrypted documents, unsupported filter cases and detailed layout reconstruction remain outside verified content support. PDF text depends on the registered Windows IFilter; the host remains scope-checked, bounded and killed with its parent.

- **10 engine query/ranking tests and 6 content tests pass**, including the new literal-name and early-publication/cancellation regressions. Existing Stage A's 91-test/30-desktop suites were not repeated.
- **7/7 final release desktop checks pass**: readiness, plain `t3`, Enter accepting literal Name contains, supported content types, partial UI before final completion, query replacement and complete exit.
- **Browser file paste now verified on the release:** GPUI File actions > Copy writes the owned file to the native Windows clipboard; a loopback Chromium paste target reads actual file bytes with a matching SHA256. [Evidence](benchmarks/stage-ab-browser-clipboard-release.json). No external upload or chat message was sent. Earlier keyboard/coordinate driver attempts did not establish source clipboard delivery and are not acceptance passes. Prior successful drag-out checks remain their original evidence.
- Strict app Clippy (all targets/features), normal-feature check, formatting and diff checks pass. Optimized build **6 min 33 s**; executable **18,032,128 bytes**, SHA256 `D129CA3BC76F4ED28E0E2CD1CAB23C58A16D763D20C0FD3DD158C4D35CB33805`. Framework pins, helper installation, file-operation safety and index format are unchanged.

Early UI reports retain an unasserted menu click and an assertion that incorrectly expected the second text match in the first batch. The trace showed the first batch contained Code.rs; the final harness checks that real first row. Final release evidence above supersedes those failed checks. The initial keyboard route also genuinely opened the selected file and was corrected before the final build.

## Remaining-stage audit

| Area | Current evidence / disposition |
| --- | --- |
| Stage A sustained idle deferral | Already completed during Stage B: 29.992 / 27.306 s quiet foreground on 10k/1M, each 0 measured CPU ms and 0 redraws. Not repeated here. |
| Stage A correctness, file safety, helper/walker closed changes, Largest | Existing measured Stage A records retained; no duplicate full-drive suites. |
| Browser file clipboard | Newly verified here with actual pasted file bytes. Explorer was already verified. Individual chat apps remain uncertified; no user draft/message was modified. |
| Stage B external drag | Authorized follow-up already passed Explorer, browser input/drop, multi-selection, Escape and internal targets. No framework patch. |
| Stage B content scope, exclusions, cancellation | Existing tests plus the focused cancellation/partial-publication regression and current UI query-replacement check pass. |
| Word/PDF content accuracy and delay | New ten-case corpus and real-folder measurements above; limitations remain explicit. |
| HDR and physical mixed-DPI | Unverified: suitable hardware is unavailable. A software pass cannot certify these. |
| Whole-drive performance | Slow full no-helper relist (140.524 s in Stage A) remains a known limitation; not fixed or relabelled as a pass here. |
| True cold-cache launch, tabs and Stage C polish | Not started; separate Stage C scope. |

## Preview feasibility, not implemented in this pass

Yes: Windows can render PDF pages through [PdfPage.RenderToStreamAsync](https://learn.microsoft.com/en-us/uwp/api/windows.data.pdf.pdfpage.rendertostreamasync). Rich Word/PDF previews can also be hosted through [IPreviewHandler](https://learn.microsoft.com/en-us/windows/win32/api/shobjidl_core/nn-shobjidl_core-ipreviewhandler). On this machine, DOCX handler `{84F66100-FF7C-4fb4-B0C0-02CD7FB668FE}`, PDF handler `{3A84F9C2-6164-485C-A7D9-4B27F8AC009E}` and Word.Application are registered. Registration is not a rendering test. Word page fidelity depends on the installed provider; extracted-text fallback would not require Office layout rendering. Provider hosting needs bounded lifecycle/error handling to keep a broken document from blocking Seek.

The current icon is not a full document preview. The user asked about feasibility, so this pass adds no preview host, Office conversion dependency or new document rendering feature. Next step: review the corrected search behaviour; Stage C or rich previews only when separately authorized. [Machine-readable summary](benchmarks/search-followup-summary.json).
