# Folder navigation and scroll follow-up - 2026-10-16

The reported navigation/scroll/selector corrections are implemented and verified. Framework revisions and the installed helper are unchanged. No whole-drive first-index or million-file truth benchmark was repeated.

- Folder children use a bounded prefix lookup over the live overlay, skipping unrelated folders and entire deeper subtrees. Existing index data supplies the contents; opening/expanding a folder does not scan disk. Case-insensitive folder resolution, hidden/excluded files and nested content are retained.
- Complete folder listings join once with prior rows to keep their order and current metadata. Expanded-folder requests retain the same view identity. Switching locations clears unrelated expansions. Engine notifications are coalesced while one is already queued.
- Large updates apply to a private copy of the mutable overlay outside the exclusive index lock, then publish the finished snapshot in a short swap. The immutable base mmap is shared through an Arc. Old snapshot/mmap cleanup also runs outside the lock. Readers keep a consistent snapshot during the update; this temporarily duplicates mutable overlay storage for bulk changes, not the base mmap. Small updates retain their in-place path.
- Background refresh restores selection without scrolling it into view. It anchors the first visible row and fractional pixel offset across insertions/deletions. Only deliberate navigation starts at the top. Virtualized rendering is retained.
- Catch-up uses FIFO reconciliation markers, with epochs for newer recovery work. Traffic arriving after the marker remains ordinary live work instead of requiring the whole drive to become silent. Bounded helper-response turns let control/completion messages run. The warning remains visible during actual initial recovery or a new overflow; it is not hidden or replaced with a false ready state.
- Hidden-files and location selector labels align left; other Settings button labels remain centered. Existing sidebar targets expose their native names for accessibility and exact click verification. No layout or icon redesign.

## Verification

**83 Rust tests pass**, one existing benchmark ignored; final all-target/all-feature Clippy (`-D warnings`), formatting and the normal-feature build check pass. New regressions cover overlay ranges with 20,000 unrelated/deep entries, shared-mmap snapshot isolation, stable row joining, expansion identity, FIFO boundaries under continued changes, and pixel-accurate viewport anchoring. [Source checks](stage4-navigation-final-validation.json).

Final optimized GUI checks pass **5/5**. Expand 2,049 direct children (2,050 rows including the folder) in **23.524 ms**, select the folder, scroll to the bottom, add eight live files, then delete a row above the viewport. The view stays at row 2030 during additions and follows the same content to row 2029 after deletion. The hidden-files selector is visually left aligned. Catch-up completes and Close exits 0. [GUI evidence](stage4-navigation-release-ui.json).

Real Known Folders were clicked in a disposable copy of the normal saved index, using the installed helper. User files were read-only. Metrics used a user-restricted in-memory pipe to avoid indexing the trace itself. Times measure the app request through DirectX Present return, excluding automation setup and physical scanout. Icons/previews may finish asynchronously. These are 12 real interactions, not a broad latency distribution or a guarantee for an unindexed folder. Every measured interaction must pass the 100 ms guard.

| Location | Direct rows | Request to presented contents (ms) | Catching up at that presentation |
| --- | ---: | ---: | --- |
| Downloads | 488 | 14.049 | yes |
| Desktop | 17 | 14.418 | yes |
| Documents | 134 | 16.569 | yes |
| Pictures | 16 | 12.974 | yes |
| Videos | 8 | 12.233 | yes |
| Downloads | 488 | 18.501 | yes |
| Downloads | 488 | 10.836 | yes |
| Desktop | 17 | 13.083 | yes |
| Documents | 134 | 16.714 | yes |
| Pictures | 16 | 14.633 | yes |
| Videos | 8 | 10.825 | yes |
| Downloads | 488 | 18.388 | yes |

Median **14.233 ms**, maximum **18.501 ms**. The catch-up boundary also completes in this real-drive copy after **33.752 s**. Cached navigation is available during that recovery; this is not an instant-recovery claim. Earlier candidates stalled for 1.402 s in Documents and 2.085 s in Downloads during recovery. The latter spent 2.069 s waiting for the index lock: moving old mmap cleanup alone was insufficient. The final bulk-update snapshot fix removes that observed writer stall. Diagnostic per-folder queue/lock/base/overlay timings and failed candidate reports are retained. [Real-folder evidence](stage4-navigation-real-folders.json). [Optimized build](stage4-navigation-release-build.json), SHA256 `AED4DAC8897E283BEC4F7DC05D54313DDB060E1664FCA4E58B84B062884CB3C4`.

The first harness used too little wheel distance for its chosen assertion. A later deletion test caught a draft anchor calculation using viewport height instead of row height; that draft build was stopped, the calculation was corrected and a fractional-offset regression was added. Failed reports are retained and are not passes. A test-module placement Clippy error was corrected before final validation. The final bounded visual check is inline, not a new independent-review claim. Other DPI/HDR displays and Mica fallback were not rechecked.

Earlier Stage 4 throughput, startup and idle reports remain bound to their recorded binary. The sustained foreground-idle and approximately 300 ms launch acceptance limits are unchanged by this focused follow-up. No Later ideas were started.

Reproduce on Windows after the pinned release build (these checks open their own temporary app windows):

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/check-stage4-navigation.ps1
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/check-stage4-navigation.ps1 -RealProfile -Output docs/benchmarks/stage4-navigation-real-folders.json
```
