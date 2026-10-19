# Release content search and cache assessment

2026-10-18. Task 2, after icon commit `d69ba9d`. No Stage C, packaging,
framework upgrades, persistent text cache or real Seek profile testing.

## Delivered behavior

Windows Search supplies provisional rows while the complete, newest-first direct
scan runs concurrently. The header says **Quick results (may be incomplete)**;
each unconfirmed catalog row identifies itself. Verified matching lines replace
placeholders without duplicate paths, including case/extended-prefix aliases.
**Complete** denotes the final direct answer, with searched/total, skipped and
unreadable counts. Provisional-only rows do not enter the final answer.

The catalog gets only exact paths approved by the existing scope, hidden-file,
secret, exclusion, availability and size checks. Its finite ordinary-user child
joins a kill-on-close 256 MiB job, streams bounded output and stops after two
seconds or cancellation. Quick work considers at most 8,192 eligible files;
the authoritative direct scan has no such entry/result/global-time cutoff.
Punctuation unsupported by the catalog grammar falls back to literal scanning.
Changing the query, Cancel and close retain their cancellation behavior. No
catalog worker remains after the request; no service, watcher or startup task
was added. All menus remain GPUI menus.

## Actual release measurements

The Windows Known Folder API resolved this user's Downloads and redirected
OneDrive Documents. Diagnostics exit before profile initialization. Both
versions used optimized all-feature RELEASE builds, never debug. These are
scanner-to-publication timings, not a promised keyboard-to-painted-frame or
cold-cache benchmark. One sample per cell unless stated otherwise. No compiler
ran during the recorded scans.

| Query | Before: first / complete | After: first / complete | Final matches |
|---|---:|---:|---:|
| `note` | 354.959 ms / 23.240 s | 547.775 ms / 40.440 s | 150 |
| `rudkin` | 22,503.695 ms / 23.028 s | 258.080 ms / 25.761 s | 1 |

The single `rudkin` document was last modified on **2026-01-18**. Its first
match is about **87 times sooner**. Both final path sets exactly equal the
baseline, with zero duplicate paths. Each scan attempts **437/437** candidates,
reports **533 skipped** entries and **2 unreadable PDFs**. No claim of recall for
unreadable, unsupported, offline or oversized files is made.

The slower first post-build common-word run is retained above. A single targeted
warm control on the **same final executable** investigated that concern:

| `note`, warm control | First match | Complete |
|---|---:|---:|
| Direct only | 425.001 ms | 28.822 s |
| Direct plus quick catalog | 259.670 ms | 28.739 s |

Both return the same 150 paths. This control shows no material complete-scan
overhead in that pair, and earlier presentation. Native PDF extraction dominates:
22.859 s in the original baseline, 38.483 s in the first post-build run,
25.238 s in the older-word run, and 28.865 s in the cache assessment. DOCX
extraction in those samples ranges from 0.222 to 1.085 s. We did not isolate
the cause of native-filter/system variability, and do not claim faster complete
scans or a universal subsecond first result.

## Why the catalog misses matches

In the measured quick runs it supplies **122 `note` / 1 `rudkin`** paths. A
separate exact-path SQL snapshot supplied **121 / 1**, demonstrating that the
live catalog can change between observations. Its `note` gap against the 150
direct matches breaks down as follows:

- **15 embedded-only matches: 14 PDFs and one TXT.** The direct scanner finds
  literal substrings inside words; Windows' `"note*"` condition matches word
  prefixes, not arbitrary substrings. None of those 15 bodies has a matching
  word prefix according to the extracted-text token check.
- **12 whole-word/prefix matches in six Markdown and six Python files.** Their
  metadata is catalogued, but the content queries miss them. Neither extension
  has a registered PersistentHandler on this machine, supporting missing body
  indexing rather than a filename/scope error.
- **Two Word misses in that SQL snapshot.** Both are standard packages with a
  prefix match in `word/document.xml`. One matched on a subsequent exact-path
  query; the other still missed `note`, while matching four of seven tested
  body terms. The remaining Word discrepancy is a catalog body/token coverage
  issue; its internal filter cause was not established. Size and modification
  metadata match; we did not establish stale body data as its cause.

Thus word-prefix semantics account for much of the gap, but not all of it.
Metadata coverage cannot establish complete content coverage. See Microsoft's
[prefix wildcard rules](https://learn.microsoft.com/en-us/windows/win32/search/-search-sql-wildcards)
and [file-type/body indexing behavior](https://learn.microsoft.com/en-us/windows/win32/search/-search-3x-wds-included-in-index).
No Windows indexing settings were changed to make results appear complete.

## Cache evaluation only — not implemented

A finite, opt-in diagnostic extracted the same approved corpus into RAM, then
discarded it. It created **no extracted-text cache on disk**. The proposed key
would be path, input size and modification time; these measurements estimate
payload plus record keys, not database/filesystem overhead:

- **364 readable PDF/Word files:** 278 PDFs and 86 DOCX, with two PDFs failing.
- Extracted UTF-8 payload **5,804,048 bytes (5.535 MiB)**; estimated key/record
  data **32,254 bytes**. Uncompressed combined estimate **5.566 MiB**.
- Independent per-file Deflate/ZIP buffers produced **2,036,903 bytes** in RAM;
  plus keys, approximately **1.973 MiB**. The measured compressed buffers were
  never written to disk.
- Whole readable-corpus substring checks, **10 RAM repetitions per query**:
  `note` median **17.831 ms**, observed p95 **18.614 ms**; `rudkin` median
  **16.993 ms**, observed p95 **18.817 ms**.

Retaining text can avoid the roughly 23–40 seconds spent extracting bodies on
each scan; the RAM numbers show the CPU lower bound. They imply substantial
potential, **not measured disk-cache latency or a guaranteed speedup**. Disk
reads/decompression, metadata validation, invalidation and actual cache storage
overhead remain unmeasured because no disk cache was built. Approval is still
required before implementing it.

## Verification and reproduction

- **10/10 content Rust checks**, including scope/link/secret/cancellation limits
  and normalized-path deduplication; strict Clippy, normal-feature check and fmt.
- **9/9 targeted desktop checks** for streaming, totals above 200, query changes,
  Cancel, protected/out-of-scope fixtures and close.
- **10/10 final-release known-answer cases:** DOCX split runs, headers/footer,
  table, Unicode/literal punctuation, compressed/multipage PDF, metadata-only
  negative, absent term and broken inputs. Quick fallback does not lose results.
- Final-release native UI checks and idle sample are recorded in the sanitized
  [measurement summary](benchmarks/content-quick-summary.json).
- Final release **19,586,048 bytes**, SHA256
  `4F2C1B5D05826697FD02DD782D5D114B9C1BA0E46E9C9D9754A0741207676BF0`.
  Its native 16/32 px executable icon extraction also passes in Light/Dark.

Reproduce with `cargo build --release -p seek --all-features`, then
`scripts/measure-content-search.ps1 -Quick`. Use `-Evaluate` for the RAM-only
assessment; outputs stay in `.references`. Raw local evidence includes
`release-content-before-clean.json`, `content-search-release-after.json`,
`warm-common-{direct,quick}.json`, `content-search-cache-evaluation.json`,
`release-catalog.json`, `catalog-gaps.json`, `word-coverage-summary.json`, the
accuracy report and UI captures. They contain private paths and are not committed.

Check by eye: the icon on your taskbar/Explorer; the partial-results label and
provisional lines becoming actual matching lines; Complete and final counts;
Cancel/new typing. Physical HDR/mixed-DPI and other Windows/user configurations
were not checked. The separate large Excel preview request was outside these
two changes and was not retested.
