# Phase 6: search engine benchmark

Machine: AMD Ryzen 7 7445HS (12 logical CPUs), 15.2 GB RAM, Windows 11, NVMe. Index: a saved snapshot of the whole C: drive, **3,541,426 entries** (`.bench-data/large`). Measured on 2026-11-22.

## Method

`seek-cli search-bench --data <dir> [--rebuild] [--save-to <dir>]` (crates/seek-cli/src/search_bench.rs):

- **Latency**: in-process engine search on a loaded index, no IPC and no painting. 3 warmups, 30 samples, median and nearest-rank p95. Each sample starts with an empty name cache, so it is a fresh search, not an answer reused from the previous keystroke. Result counts are checked to be identical across samples. "First page" asks for the 200 rows the app requests; "all results" asks for up to 100,000.
- **Typing**: `readme.md` one key at a time with the name cache kept, as in the app.
- **Accuracy**: 300 targets sampled with a fixed seed from files whose name is unique in the index; 150 from the user's own folders (under the profile, outside AppData and dot folders) and 150 from anywhere else. Each target yields up to 7 queries: exact name, prefix (first half of the name without its extension), a middle fragment, an acronym (3+ words), two words, one typo (a substitution in the middle), and a path hint (parent folder + prefix). 1,607 queries. Rank is the target's position in the top 100.
- **Junk rate**: words from the user-folder targets plus 20 common words (report, config, backup, …). Of the 234 that have a match in the user's folders, the share whose top 20 is more than half AppData, `C:\Windows`, Program Files, ProgramData, the Recycle Bin, node_modules, .git, caches, Temp, `target` and similar.
- **UI stall**: `scripts/bench-large-search.ps1` with the release app (benchmarks feature) on a copy of the index, typing 8 queries 3 times each; a watchdog thread pings the window with `SendMessageTimeout` every 4 ms.

Latency on this laptop varies by up to 2× between runs. The latency comparison below alternates before and after runs, two of each.

## What changed

| Item | Decision | Evidence |
| --- | --- | --- |
| 6.2a keystroke narrowing | **Already present**: the last search's scored names are reused while typing (`NameCache`) | typing 1.2–2.0 ms per key (median, across runs), Heimdall 4.0 ms |
| 6.2b memchr literal prefilter | **Not built**. Seek's per-class bitmaps already beat Heimdall's memmem path on 9 of its 10 queries. Fuzzy tokens (the default) can't use a substring prefilter without changing results. | table below |
| 6.2c two-stage scoring | **Already present**: names are scored once per distinct name, paths built only for the top k | — |
| 6.2d path-aware ranking | **Built**: Windows folder weights, case-insensitive, drive-level rules, the profile boost wired up (it was silently off on Windows), editable `ranking.json` | accuracy and junk tables |
| 6.2e ranking signals | **Built**: whole name or stem, then word-start matches (space, punctuation, camelCase, digits), then the rest; within a tier the score (with location) decides; a typo never beats a clean match | tier-only column |
| 6.2f bounded top-k, shared floor, cancellation | **Already present** (`TopK`, `Query::cancelled`) | — |
| 6.2g memory layout | **Not built**: search is not memory-bound here, and the index is already 62 bytes per entry (Heimdall: 84.5) | — |
| 6.2h index freshness | **Existing** USN/helper and walker paths unchanged; the sidebar now says which mode runs ("Live" or "Scan", with a tooltip) | see Unverified |
| 6.2i query language | **Built** `mod:` and `-in:`; `?` popover in the search field; docs/SEARCH_RANKING.md | tests |
| 6.2j correctness | **Built** NFC normalisation of queries (Windows `NormalizeString`). Case folding, long paths, hard links and cloud placeholders were already covered by existing tests. | tests |

## Accuracy (1,607 queries, 300 targets)

| | Baseline (081c8ff) | Weights only | Tiers only | **Weights + tiers** |
| --- | ---: | ---: | ---: | ---: |
| top-1 | 0.456 | 0.463 | 0.459 | **0.464** |
| top-5 | 0.538 | 0.550 | 0.541 | **0.550** |
| top-20 | 0.600 | 0.616 | 0.602 | **0.617** |
| MRR | 0.496 | 0.506 | 0.499 | **0.507** |
| user-folder targets, MRR | 0.570 | 0.594 | 0.569 | **0.591** |
| other targets, MRR | 0.424 | 0.419 | 0.430 | **0.423** |
| **junk rate** | **76.1%** | 65.4% | 69.2% | **55.1%** |
| mean junk share of top 20 | 69.3% | 62.2% | 64.8% | **55.5%** |

By query kind (top-1 / MRR), baseline → after: exact 0.993/0.997 → 0.997/0.998; prefix 0.324/0.382 → 0.331/0.392; middle 0.049/0.077 → 0.089/0.124; acronym 0.240/0.276 → 0.247/0.292; two words 0.206/0.282 → 0.220/0.291; typo 0.929/0.941 → 0.929/0.941; path hint 0.326/0.394 → 0.331/0.398.

No regression in overall top-1, top-5 or MRR, and none for the user's own files. Targets inside system folders lose 0.001 MRR and 0.001 top-5 against the baseline (one query of 807), as expected when those folders are deliberately demoted.

What remains in the junk rate is mostly exact names: a query `backup` puts folders literally named `Backup` in Program Files and AppData above `Backup-codes.txt` in Downloads, because a whole-name match ranks first by design.

## Latency, before and after (ms; mean of two runs' medians)

| Kind | Query | First page before | after | All results before | after | All hits |
| --- | --- | ---: | ---: | ---: | ---: | ---: |
| 1 char | `a` | 1.03 | 1.40 | 372 | 380 | 100,000 |
| 2 chars | `re` | 2.55 | 1.71 | 408 | 398 | 100,000 |
| 3 chars | `con` | 2.51 | 2.17 | 437 | 459 | 100,000 |
| whole word | `report` | 2.44 | 2.38 | 94.3 | 92.7 | 19,581 |
| whole word | `invoice` | 4.24 | 3.62 | 18.4 | 16.3 | 3,096 |
| prefix | `^read` | 3.60 | 3.32 | 35.0 | 34.2 | 9,870 |
| substring | `port` | 3.72 | 3.10 | 474 | 338 | 71,752 |
| camelCase | `MainWindow` | 11.8 | 7.53 | 11.6 | 7.76 | 122 |
| underscore | `test_utils` | 4.80 | 5.21 | 5.85 | 5.58 | 446 |
| typo | `documnet` | 10.1 | 7.86 | 98.0 | 82.4 | 16,441 |
| several words | `windows update log` | 27.6 | 21.0 | 32.4 | 25.7 | 1,180 |
| path-qualified | `users downloads pdf` | 23.1 | 21.5 | 29.1 | 26.3 | 1,265 |
| extension | `ext:pdf invoice` | 1.17 | 1.00 | 1.26 | 0.93 | 73 |
| extension only | `ext:png` | 3.07 | 2.73 | 123 | 92.7 | 42,484 |
| exact file | `readme.md` | 1.83 | 1.40 | 43.7 | 31.2 | 8,170 |
| no match | `zzqxv` | 0.45 | 0.30 | 0.45 | 0.30 | 17 |

No query kind got slower beyond run-to-run noise. Typing: 1.96 → 1.43 ms per key (median; these runs used 10 accuracy samples, so they finished quicker but the typing loop is the same). Raw files: `search-latency-{before,after}-run{1,2}.json`.

## Memory, build and restart

| | Before | After |
| --- | ---: | ---: |
| Load saved index | 59 ms | 49 ms |
| Peak working set, engine only | 271–272 MB | 275–276 MB |
| Restart to first result | 142–802 ms | 589–640 ms |
| One-off reweight on first start with new weights (rebuild from the saved index, no disk scan) | — | 1.66–1.86 s |
| UI: worst stall while typing, 3.5M index during catch-up | 13.5 ms (Phase 4 run) | 46.4 ms (first launch, during the reweight), **31.8 ms** (second launch) |
| UI: worst scroll stall | 6.7 ms | 38.4 / **8.3 ms** |

No UI-thread stall exceeded 50 ms. These UI runs happen while Seek is still catching up on the whole drive (about 30 minutes on this machine), so working set and idle CPU during them are not steady-state numbers. The index build time for a full walk of C: was not re-measured; only the in-memory rebuild changed.

## Heimdall comparison (same machine, same day, same C: drive)

Heimdall `heimdall-bench --walk --drives C --iters 30` (its own queries, a new `Searcher` per iteration, limit 50), against Seek searching the same queries with limit 50 and an empty name cache per iteration.

| Query | Heimdall in-process median (p95) | Seek median (p95) |
| --- | ---: | ---: |
| `a` | 18.8 (25.9) | 0.03 (0.06) |
| `re` | 21.5 (27.2) | 0.06 (0.08) |
| `report` | 4.9 (6.6) | 0.26 (0.32) |
| `main.rs` | **3.4** (4.9) | 6.6 (7.8) |
| `readme` | 10.0 (15.0) | 0.24 (0.43) |
| `heimdall` | 3.6 (4.6) | 0.64 (0.78) |
| `config json` | 19.7 (25.9) | 8.8 (11.7) |
| `ext:pdf invoice` | 4.1 (6.5) | 0.51 (0.66) |
| `node_modules` | 5.2 (7.3) | 3.0 (3.4) |
| `zzqxv` | 13.6 (16.9) | 0.16 (0.21) |
| typing `readme.md`, per key | 4.0 (18.3) | 1.4 (2.7) |
| index | 4,061,294 entries, walk 67.3 s, 327 MB (84.5 B/entry) | 3,541,426 entries, 220 MB file (62 B/entry) |

Not like for like:

- Heimdall counts **every** match for each query, while Seek's ranked path stops once its top 50 is certain. For one-character queries most of the gap comes from that.
- The indexes differ: Heimdall's walk saw 4.06M entries, Seek's snapshot has 3.54M; the snapshot is older and its walker skips some system locations.
- Heimdall's indexer service is installed and running here (MFT, 3.88M entries); its named-pipe round trips were 0.14–16 ms. Seek's numbers have no IPC.

Seek is slower on one query: `main.rs`. A token with a dot names a file, so it skips the ranked path and scores every distinct name; the letters m, a, i, n, r and s are common enough that the bitmaps filter little. 6 ms is far below the 50 ms budget, so it was left as it is.

**Everything** is not installed on this machine, so there is no Everything comparison.
