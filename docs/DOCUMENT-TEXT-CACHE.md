# Account-private document text cache

2026-10-19. This change implements the approved PDF/DOCX text cache and the separately confirmed distinction between Recycle Bin and permanent deletion. No Stage C, packaging, framework upgrade, content-scope expansion, service or startup task.

## Behavior and privacy

Settings > Files and search has **Remember document text for faster content search**, on by default, and **Clear cache**. The displayed limit is **128 MiB**. Switching off removes saved document text. Clear cancels any current search before clearing and leaves it empty until a subsequent search. Saving a smaller content scope drops the removed folders' derivatives.

The default location is `%LOCALAPPDATA%\Seek\document-text-v1`. It is inside the current user's Windows profile. A protected directory DACL grants access only to the current account; payloads inherit that ACL. Both text and the manifest (including source paths) use Windows DPAPI CurrentUser encryption. Extracted text is Deflate-compressed before encryption. No plaintext extraction is written to disk. Cache initialization outside the resolved user profile, through a cache-directory link, or without the required permissions fails back to reading originals, with an explicit cache-unavailable status.

Only existing on-demand content searches fill this cache. There is no cache discovery/indexing worker running in the background or at launch. The existing search worker performs finite maintenance and writes. It keeps no complete plaintext corpus in memory. Successful PDF/DOCX extractions only are saved; ordinary text/code reads are unchanged.

Keys contain canonical source path, size, precise Windows modification time and extractor version. PDF keys also fingerprint the installed IFilter DLL. Every hit still opens and reads the source through the existing scoped, size-limited, secret-aware handle checks and compares its SHA-256 digest. Consequently, even a same-size edit with its old timestamp restored cannot reuse old text. Unavailable/missing sources are never answered from saved text. Renames/moves use the new path and discard the old derivative. Corrupt/obsolete entries are discarded and extraction is retried from the actual source. The complete direct scan remains authoritative; existing Windows Search quick rows remain provisional.

The cache is bounded by allocated-cluster estimates, a 4,096-entry limit and a bounded encrypted manifest; least-recently-used entries are evicted. Three MiB is reserved within the 128 MiB limit for atomic manifest replacement, encryption and allocation overhead. Atomic writes are encrypted before creating temporary files. There is no file-operation change to user documents from cache maintenance.

Existing limits remain: 4 MiB input per content-search file, 8 MiB extracted text, scoped locations only, no protected keys/secrets, and bounded PDF extraction hosts. Unsupported/offline/oversized/unreadable documents remain visibly counted, rather than being silently supplied from stale cache text.

## Verification

Four focused Rust tests cover persisted hits, same-metadata edits, renames/deletion, removed scopes, disabling, ciphertext corruption, extractor-version changes, eviction and rejection outside the profile. `scripts/check-text-cache.py` additionally runs the actual PDF/DOCX host through separate processes: ten known-answer cases with cache off, empty and persisted/restarted; then modify, rename, move, delete and remove-scope checks against fresh direct scans. The restarted first case hits all five readable document fixtures (two Word, three PDF). Broken fixtures are explicitly unreadable, not cached.

Native desktop checks verify the default-on setting, Clear leaving no derivative files, persisted Off after restart, Off searches not writing text, and re-enabling filling it. Light and Dark glass settings are visually checked. The actual directory has a protected single-account ACL; payload and manifest files inherit only that account's rule. These checks do not establish another-account logon or administrator-proof security.

Release measurements and final artifact information are recorded below and in [the sanitized summary](benchmarks/document-text-cache-summary.json). All app profiles/cache roots are unique owned temporary directories under the user profile. Real Downloads and Windows-redirected Documents are read only as explicitly requested. The real Seek data directory is never opened for testing.

## Measured release results

Optimized release with diagnostic instrumentation, ordinary user, **437/437 candidates** attempted. These are direct scans, excluding provisional Windows Search rows, so the timing comparison isolates saved-text reuse. Each passing scan explicitly reports **535 skipped entries and 2 unreadable PDFs**. Final cache-off, empty-cache and restarted-cache path sets are identical: **150 `note`, 1 `rudkin`, no duplicates**.

| Search and cache state | First verified match | Finished scan | Matches |
| --- | ---: | ---: | ---: |
| `note`, cache disabled control | 412.907 ms | 28.612 s | 150 |
| `note`, empty cache, filling | 468.922 ms | 42.538 s | 150 |
| `note`, full cache after process restart | 108.367 ms | 1.018 s | 150 |
| `rudkin`, cache disabled control | 27.433 s | 28.063 s | 1 |
| `rudkin`, full cache after process restart | 940.358 ms | 1.039 s | 1 |

The first `note` search writes **364 entries** (all readable PDF/Word files). The subsequent `rudkin` search in that same process already has those entries: **1.339 s first / 1.440 s complete**. Restarted scans each hit **364 entries**, make **zero extraction writes**, and retry the two unreadable PDFs. Repeats improve complete-scan time about **28× / 27×** versus the uncached control. Filling the cache takes longer than the uncached control in this sample; it does not accelerate the first extraction.

An earlier, first post-build empty-cache sample is retained as a **failed correctness comparison**, not hidden: `note` **1.231 s first / 58.272 s finished, 146 matches, 6 unreadable**, including four additional PDFs which were readable later. It had zero extra results. The following scan filled the four missing extractions; restarted scans returned 150. A fresh empty-cache repetition then returned all 150 and matched the unchanged executable's uncached control exactly. The cause of the initial transient extraction failures was not isolated. The cache does not solve native PDF extraction failures; they remain explicitly reported. No unconditional first-run recall claim is made.

Cache: **2,380,558 bytes logical (2.270 MiB)**, **3,178,496 bytes allocated (3.031 MiB)** across **364 payloads + one encrypted manifest**, well below the displayed 128 MiB cap. Allocation is measured using Windows FileStandardInfo; filesystem directory/MFT metadata is excluded. Directory and all 365 files grant access only to the current account. The root DACL is protected.

Diagnostic parent memory: empty-cache process **12.398 MiB sampled peak private / 22.367 MiB peak working set**; full-cache process **10.059 / 19.008 MiB**. These exclude the short-lived PDF hosts. The release GUI, using a small owned index and the real content scopes, shows 150 matches with 364 hits; one sample is **210.933 ms first / 1.378 s complete**. Afterward, foreground idle is **20.027 seconds, 0 measured CPU ms, 0 redraws, no user input**, then normal close exits **0**. GUI private commit **90.551 MiB**, working set **83.641 MiB** in that sample. This is not a measurement of the full-drive index's memory. The earlier interrupted 10-second sample is not used as idle evidence.

Final executable: **19,746,816 bytes**, SHA256 **`C4F00ED7878E962C05D567910ADAEE15B96397197BF0F87D105D1C53A6D8ADDA`**. Four cache Rust tests, 30 known-answer query/mode combinations, six owned mutation/scope assertions, targeted desktop checks, strict all-target Clippy, normal-feature check and formatting pass. No framework pins changed; SHA-256 uses the already-locked `sha2` 0.10.9 package.

## Delete behavior

The user's explicit clarification replaces the older blanket prohibition on permanent deletion for this separate command only:

- **Delete / Move to Recycle Bin:** goes directly to the Recycle Bin, retaining the existing guard against permanent fallback on unsupported locations.
- **Shift+Delete / Delete permanently…:** opens a native GPUI confirmation naming the captured selection. Cancel, Escape or closing the dialog does not enqueue an operation. **OK** submits that captured selection once to the existing worker, which uses Windows IFileOperation without recycle/undo flags. No user-file deletion runs on the UI thread.

Owned native desktop fixtures prove that a file survives before confirmation and after Cancel/Escape; OK removes it without a Recycle Bin entry. Normal Delete removes the source and creates the corresponding Recycle Bin item. The file list updates after both operations. Search-box text focus retains text editing. The destructive dialog uses a readable themed surface in glass mode.

## Reproduction and limits

```powershell
cargo test -p seek --all-features text_cache::tests -- --test-threads=1
cargo clippy -p seek --all-targets --all-features -- -D warnings
cargo check -p seek
cargo fmt --all -- --check
cargo build -p seek --release --all-features
python scripts/check-text-cache.py target/x86_64-pc-windows-msvc/release/seek.exe
python scripts/measure-text-cache.py
```

The measurement script resolves the current account's Known Folders, creates a fresh temporary data/cache directory, runs separate optimized diagnostic processes for empty, persisted and uncached scans, and compares the final path sets. Full path-bearing reports remain local under `.references`; committed summaries contain counts/timings only. Memory sampling covers the diagnostic parent, not the separate short-lived PDF hosts. Empty text cache does not mean a flushed Windows filesystem cache. Single samples are not a percentile or a cold-boot guarantee.

Check by eye: the new Settings switch, limit and Clear button in Light/Dark/glass; repeated `note` and `rudkin` searches ending at Complete with unchanged answers; and the two deletion choices using disposable files only. Another Windows account, physical HDR/mixed-DPI and another machine were not checked. No next stage starts automatically.
