# Settings, info-panel resizing and retained access failures

2026-10-19. Targeted follow-up only; no Stage C, packaging or dependency changes. GUI tests use owned temporary data. The saved-index diagnosis uses a read-only copy in a temporary directory; the production data folder is never used as a test profile or modified by the tests. No document contents or credentials are read for this diagnosis.

## Settings controls

Save locations explicitly replaced the shared glass gradient with an opaque primary/accent fill. It now keeps the same glass surface as the other Settings controls. Text controls use a full-height centered content wrapper with an explicit text size and line height, rather than inheriting the Kit label layout. Theme/dropdown labels use the same sizing; the settings navigation no longer uses a manual vertical text offset. Native GPUI buttons, focus and menu behavior remain intact.

Native Light/Dark glass captures verify the two location actions. At 125% display scale, both Save locations and Rebuild current index label ink spans y=575..586, midpoint 580.5 px, matching the buttons' vertical center. Existing sizes and rounded corners are retained. Physical HDR/mixed-DPI checks were not performed.

## Resizable information panel

The existing GPUI resize handle sits inside the panel's left edge, with the native horizontal resize cursor. A typed resize drag updates layout immediately; no file drag, shell operation or framework patch is involved. The default is 280 logical pixels. The saved preference is bounded to 240–720 logical pixels and the visible width also leaves at least 320 logical pixels for the list. A smaller window temporarily limits the visible panel without replacing the wider saved preference. Closing/reopening the panel and restarting the app retain the preference. The existing Settings writer saves once at drag completion; no timer or new worker was added.

Native Light/Dark checks at 125% verify both drag directions, panel toggle and process restart. A left drag expanded 240 to 374.4 logical pixels, and reopening showed 468 physical pixels. A right drag saved 324.8 logical pixels; process restart restored 406 physical pixels (324.8 × 1.25). A Rust bounds check verifies the default, minimum, maximum and temporary narrow-window clamp/restoration. Settings round-trip verification includes the width and preservation of existing window geometry. Initial prototype coordinate and drag-offset errors were caught by these desktop checks and corrected before the final build.

Icons and Gallery also calculate their grid using the actual visible panel width, including the no-selection case, and retain the correct column count for scroll/selection anchors. Native captures with a 520-DIP panel and 21 owned fixture files verify both layouts without clipping. The patched Windows backend maps this resize cursor to `IDC_SIZEWE`, the native double-headed horizontal arrow.

## Root cause of the inflated inaccessible count

The earlier fresh-index comparison did not reproduce the retained-history problem. This follow-up inspected only a **read-only copy** of the saved filename index. It contained **1,862 recorded errors**: 1,614 path-not-found (Windows error 3) and 248 access-denied (error 5). Independent directory listing at observation time found **1,620 nonexistent paths and 242 genuine access denials**, with zero readable paths in that set. Six formerly denied paths had also disappeared. The screenshots' 1,859 / 1,872 and this saved snapshot are different observations; their exact small differences are not reconstructed, but the accumulated missing-path errors explain the large inflation. This is a counter-retention bug, not evidence that 1,800 readable folders were skipped.

Failures were persisted in the saved helper footer, restored on launch, and cleared only if that exact path was subsequently listed successfully. Deleted/moved directories could therefore leave permanent warnings. The directory enumerator also recorded path-not-found during ordinary filesystem changes as a new inaccessible location. Fresh indexes lacked that old history, which is why the previous fresh tests looked normal.

The fix uses the existing ordinary-user index worker: after priority folders are ready, retry saved failed paths. Missing paths are removed from the error map. Recovered folders have their full subtrees reindexed before an updated status is published. A complete parent listing removes obsolete failure entries for child subtrees that no longer exist. Missing-path responses from older installed helpers are filtered in the client too; no helper reinstall or administrator operation is needed. Existing access failures remain explicit, and the index's readable contents are not suppressed to make the number smaller.

On the copied saved index, ordinary **Auto** catch-up now finishes as **ntfs + usn**, with no helper error, fallback, reset or reconciliation. The final count is **249**, all independently confirmed error-5 access denials; no nonexistent paths remain. Independent listings versus indexed direct children match all six current-user Known Folders: Downloads 536, Desktop 18, Documents 134, Pictures 16, Videos 8, Music 2 — **714 entries, zero missing/extra**. The updated copied checkpoint then reloads with **249 stored errors, all error 5**, proving the obsolete errors were not saved again. The number can naturally vary as the live system changes; it is not hardcoded or capped.

Counter precision: these are unique recorded internal path spellings, not physical directory identities. The 249 records contain 248 distinct case-insensitive spellings (one case variant). Both spellings independently deny enumeration. The counter does not currently merge case aliases; this small existing distinction is separate from the repaired accumulation of 1,620 nonexistent paths.

Four focused engine checks cover vanished failures, empty-folder recovery, deleted subtrees retaining unrelated failures, and recovery of a cached inaccessible subtree's previously omitted file. The GUI settings/panel checks use an owned fixture only; no deletion/rename/content-search reruns on real files were performed. Debug copied-index diagnostics report priority refresh 313.751 ms, mapped load 504.352 ms and completed catch-up 19.541 s, including 17.757 s saving. These are diagnostic samples, not release performance measurements or a new speed guarantee.

The ordinary-user administrator-token check is **false**. The helper is already installed/connected. Genuine protected Windows locations remain inaccessible under the user's account; nothing bypasses their ACLs. There is no pending request to elevate the app. The app still has no inaccessible-location listing UI; none was added. Private path-bearing diagnostic reports remain in the owned temporary folder; only sanitized counts are committed.

## Final verification and check by eye

45 app Rust checks, four focused engine checks, strict all-target/all-feature Clippy, normal-feature compilation, formatting and the optimized build pass. A missing no-diagnostics `interaction` stub was supplied so the two application event calls also compile without the benchmark feature; that branch is excluded from the benchmark-enabled release runtime. Final-release native smoke restores the 520-DIP saved width as 650 physical pixels and closes the owned test window normally. Executable: **19,796,480 bytes**, SHA256 `E750D5635CDBAB39AFA5BF4EC061ECCCBA3F80482DFDEE3DF89DF429CE0DE57E`.

Check by eye: Save locations matches the other glass controls; Settings text is centered; the info-panel left edge shows the horizontal resize arrow and follows a drag; toggle/restart keeps the chosen width; Icons/Gallery reflow with the panel; the access counter loses obsolete missing-path errors after normal catch-up. No HDR/mixed-DPI hardware verification, new idle measurement or release search/catch-up speed claim. No repeated content or deletion tests, no Stage C or packaging. Existing unrelated user files and deleted document are preserved.

## Default-view selection follow-up

The segmented group's outline was rounded while child buttons explicitly used zero radius; clipping the parent did not make their selected backgrounds round. The outside end corners now match the group, and every selected segment gets a 7-DIP radius on all four corners, including the middle Icons choice. Five final native captures cover Dark List/Icons/Gallery and Light List/Gallery at 125% using owned temporary data. Strict all-target/all-feature Clippy, normal-feature compilation and formatting pass. This visual-only change adds no tests that mirror style constants and repeats no indexing/content/deletion/idle suites. Stage B remains closed; the next authorized stage is C.

The updated optimized release build passes (8m 27s): 19,796,480 bytes, SHA256 `1F3C9D13F2BCC2D1DB929167B2008F5C477D9C5FCC1DBEBF24B2BFE90794638B`.
