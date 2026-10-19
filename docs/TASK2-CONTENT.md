# Task 2: complete, cancellable content scans

2026-10-18. No global time, visited-entry or match cutoff remains. The scanner enumerates safe candidate metadata, sorts by modification time descending, then reads every eligible file until completion, Cancel or a changed query. It publishes matches immediately and file-count progress even while finding no matches. The UI says `Searched X of Y files`, followed by `Finished` or `Cancelled`. Skipped and unreadable counts remain visible; Finished means the traversal ended, not that skipped or unreadable files were searched.

Progress-only updates do not copy/rebuild the table, and cannot evict a pending result snapshot. Scans remain on the existing event-driven worker; no persistent content cache or always-running process was added. Cancel retains partial rows; a new query/F5 starts a fresh scan. The normal name-result top-200 limit no longer appears on content results.

Local extended-length paths now work. Hydrated OneDrive cloud tags are distinguished from links/recall placeholders; links, cloud-only files, exclusions, unsupported formats and files over 4 MiB remain skipped. Root and final open-handle scope checks occur before content reads. Secret-name protection also covers API-key, private-key, access-key and auth-token filenames. Expanded document text is capped at 8 MiB; PDF filter children retain their 1-second/256-MiB bound and parent-owned kill-on-close job. DOCX/text parsing retains a per-file deadline. PDF content requires a registered text filter; no OCR or legacy `.doc` support is claimed.

## Measurements

Debug executable, one actual-folder sample per query, Windows Known Folder-resolved Downloads and OneDrive Documents. Explicit diagnostic commands run before profile initialization and do not open the real Seek data folder. Raw private filenames remain in ignored `.references`; committed summaries contain only counts and timings.

| Query | Before: first / stopped | Before attempted / matches | After: first / finished | After attempted / matches |
|---|---:|---:|---:|---:|
| note | 146.068 / 3,007.769 ms | 149 / 46, limited | 459.640 / 34,791.117 ms | 437/437 / 150 |
| invoice | 276.039 / 3,005.634 ms | 164 / 1, limited | 20,755.994 / 39,818.254 ms | 437/437 / 2 |

After: 539 skipped entries and 2 unreadable files per query, both scans finished without cancellation. These are bounded-format results, not a claim to read skipped/unsupported/failed files. The eligible set contains 280 PDFs, 86 DOCX and 71 text/code files. PDF extraction accounts for **34,169.265 / 39,025.426 ms**; all DOCX takes **412.938 / 593.520 ms**. Global newest-first order can reach a rare match later than the previous truncated traversal. No universal instant-content claim.

## Windows Search comparison

`scripts/compare-content-catalog.ps1` uses the existing running Windows Search service, read-only ADO Search.CollatorDSO SQL, batching exact scanner-approved paths (32 per batch). Secret/protected candidates are excluded before any catalog content query; no service configuration is changed.

All **437/437** eligible paths had metadata in the catalog. Fourteen exact-path batches took **975.757 / 934.968 ms** for metadata coverage. Content queries took **840.978 ms** (`note`, 122 matches) and **118.363 ms** (`invoice`, 2 matches), plus one **24.194 ms** connection open. Compare against the direct-scan timings above, not a broad unrestricted catalog query. Windows Search is much faster here but is insufficient as the sole source for Seek's literal content results: it returned 122 of the direct scanner's 150 `note` matches, and 2 of 2 `invoice` matches, with no catalog-only matches. Metadata coverage does not establish complete/current body indexing. Its word/prefix semantics also differ from Seek's literal substring semantics; the 28 differences are not all classified as catalog defects.

Decision: keep the complete direct scan as the authority. No catalog-only fast path or resident content index is introduced. Microsoft documents [CONTAINS word/prefix matching](https://learn.microsoft.com/en-us/windows/win32/search/-search-sql-contains) and [scope predicates](https://learn.microsoft.com/en-us/windows/win32/search/-search-sql-folderdepth).

## Targeted validation

Seven content Rust tests pass, including a deliberate 3.05-second pause followed by all **251/251 matches**, newest-first ordering and an extended local root; scope/junction/exclusion checks, protected names, expanded/oversize limits, Unicode and generation cancellation remain covered. The existing owned DOCX/PDF known-answer corpus passes **10/10** cases (split runs, tables, headers/footers, Unicode, compressed PDF text arrays, second page, metadata-only negatives and broken files). No general recall claim for all Word/PDF documents.

Strict Clippy, normal-feature check and formatting pass. Desktop progress/Cancel evidence and final counts are recorded in the measured summary. No Stage C, packaging or framework updates. Task 3 starts only after this task is committed.
