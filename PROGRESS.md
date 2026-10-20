# Seek 1.0.1 - public release published

2026-10-20. Enlarged the content-location field and fixed first-launch All locations remaining blank until the full drive index is ready. It now shows labelled provisional recent files while complete indexing continues. Includes the committed interaction fixes. The user approved the normal test build and now authorizes publishing this version with a native Setup EXE and changelog.

**Measured:** fresh owned profiles on real C:, walker/no helper, first UI Automation result row **26,606.310 -> 2,793.024 ms** (one run each; includes polling/accessibility and uncontrolled system load; not cold cache or full-scan completion). Provisional owned-fixture rows arrived in **1.537 ms**, with the global index still unfinished.

**Verified:** 65 Rust tests, strict Clippy/formatting, 23 native installer checks, 21 package checks and 7 update-worker checks. The larger field was inspected in Dark; Light was not rechecked after observed user interaction changed the window state. Only one enabled local account exists. Clean separate release build, 1.0.1 metadata, unchanged icon, static runtime and no personal binary profile paths. No framework upgrade, real profile test, helper change or administrator action.

**Distribution:** Public release [v1.0.1](https://github.com/sanirudh17/Seek/releases/tag/v1.0.1) is published. Setup EXE **9,526,496 bytes**, SHA-256 `7062caa5c5c14c97645dbc4a8226d1a141eb1321aa56fc624618471864c4c508`; updater ZIP **9,212,488 bytes**, SHA-256 `cf75cdc8788f92e711ce43af20f304be4aedf64fb25e9f4b7c403866911e62c8`. Fresh downloads match both artifacts. The EXE installs per user and offers a native uninstaller with default data retention. GitHub currently attributes the eight release preparation commits to the generic `Seek contributors` identity; this closeout commit uses the repository owner's verified `sanirudh17` no-reply address so the owner is credited going forward. Native WinHTTP staging of the ZIP timed out twice with Windows error 12002, although direct HTTPS downloads of the API and ZIP both succeeded; the full in-app update flow remains unverified. A second account, Light recheck and new idle/memory/frame timings also remain unverified.

Check by eye: larger field, provisional All locations label, installer desktop option and uninstall data retention. The full in-app update download remains unverified after WinHTTP timeout 12002; API and ZIP downloads succeed through direct HTTPS. [Release report](docs/RELEASE_1_0_1.md), [measurements](docs/benchmarks/release-1.0.1.json), [changelog](CHANGELOG.md). Earlier build holds below are historical and superseded by this approval.

# Seek 1.0.0 - public release verified

2026-10-20. Public release **v1.0.0** is published at https://github.com/sanirudh17/Seek/releases/tag/v1.0.0. The published ZIP is **9,164,598 bytes**; SHA-256 `b21b13215d60d611fee3e021683fecd58424c0e21527c1de21ee32f3ce1fc003`. A fresh download from GitHub matched the local ZIP exactly. The app Settings Updates Check now displayed **Seek 1.0.0 is up to date** for the live public release. The UI uses a circular refresh icon, distinct from the Info panel.

The final optimized app (19,824,640 bytes) and helper have 1.0.0 Windows metadata. The existing icon remains unchanged. A clean, separate source checkout built both executables; an owned, ordinary-user profile showed its independent fixture file and closed normally (exit 0). That observed UIA smoke interval was **307 ms**; it includes polling and is not a startup performance benchmark. Binary imports contain no dynamic MSVC CRT, and the app/helper have no personal profile paths.

**Package tests:** all **20** checks passed against the extracted public release: per-user Installed apps registration, Start/optional Desktop shortcuts and icon, executable metadata, ordinary launch and close, all six current-account Known Folder sidebar labels, refusal to overwrite a running app, Cancel/default data retention/explicit owned-data removal, optional Desktop shortcut and unchanged helper registration. UIA launch observation was 1,663 ms (not a startup benchmark). Only one enabled local account exists and the current token is ordinary user; another-account test could not be done. README gives steps for a manual standard-account test.

The update worker passed **7** disposable-copy checks covering file replacement, handshake, locked-file rollback, tampered-payload rejection and preservation of owned data. The public live update check and ZIP download were also verified. A distinct future-version install cannot be tested until a later release exists. Update checks are once per app launch by default; Download and restart requires explicit user action. The package is unsigned; its checksum detects corruption but is not a publisher signature.

**Other checks:** 54 app tests passed, strict all-target/all-feature Clippy, normal compilation and formatting passed. PowerShell files parse. Public source scan across **2,963 files** found zero local profile paths and zero credential patterns. Two old screenshots containing personal paths/names are privately preserved and excluded. The release source is a fresh scrubbed snapshot, not the private historical Git history. README follows Carbon/Typr's structure and includes the shortcut table plus full build/install/uninstall/helper commands. Third-party notices contain full texts/attribution for 496 dependency/build packages and icon artwork. No GPUI/Kit/patched-Windows change or third-party version upgrade was made.

Only Windows 11 x64 and the current account were verified. A future update from one version to another, a second account, Windows 10 and physical HDR/mixed-DPI hardware remain unverified. No helper was installed or changed during packaging. The installer does not install the helper automatically; uninstall keeps saved data by default.

Check by eye: Settings > Updates refresh glyph and current status; Start shortcut and app icon; installer desktop shortcut choice; uninstall's default saved-data retention. [Packaging details](docs/PACKAGING.md), [release measurements](docs/benchmarks/packaging-release.json).

# Prior packaging draft (superseded)

The pre-publication packaging draft is superseded by the public release verification at the top of this file.

# Seek v2 Stage C follow-up - measured targets passed

2026-10-20 (checks run October 19-20). The remaining performance checks now pass on this machine and the owned one-million-file fixture. Native input-to-DirectX-Present p95 is **11.243 / 11.707 / 10.727 ms** for atlas/orbit/pixel in the ordinary path, and **14.427 / 15.188 / 14.132 ms** with accessibility active, all below 16 ms (30 samples/query, three warmups, correct 200 rows). A same-session old-release no-UIA control gives **14.374 / 13.549 / 14.182 ms**; the historical 18.551-19.704 ms measurements also reflect different system conditions, so the entire historical improvement is not attributed to this patch. Focus-interrupted/debug attempts are excluded.

**Fixes:** application-level table paint reuse, invalidated by table changes; original full rendering whenever GPUI accessibility is active, preserving native row access. No GPUI/Kit/Windows crate or vendor update. Inactive large shell previews/details now leave the CPU/GPU cache; small icons retain their existing bounded LRU. Large previews wait at final size for full imagery instead of flashing an enlarged small icon. Shared GPUI scrollbar has a **5-DIP rounded thumb, 7-DIP hover/drag thumb, 12-DIP input track**, in both themes/orientations; no idle fades. Tabs are explicit (Ctrl+T / Ctrl+Enter or native menus), never created by each new search.

**Launch:** five warm processes **227.962 ms median** (222.233-232.139 ms), below 300 ms. The user-approved one-off Microsoft-signed RAMMap diagnostic completed both cache-purge actions successfully, with standby counters **3,040,878,592 -> 3,371,008 bytes**. Seek remained ordinary-user, launched by the unelevated parent harness with owned test data. **One cold Windows file-cache launch: 303.664 ms to cached results, 3,403.720 ms to up to date.** This is not a fresh-boot/cold-SSD claim; shared OS/hardware caches are outside this measurement.

**Memory/idle:** identical release fixture/actions before versus after: one warm tab **122.176 -> 98.523 MiB** private commit; three warm tabs **144.395 -> 111.043 MiB**; after 12 preview selections **218.062 -> 115.219 MiB**; back to one tab **220.027 -> 116.699 MiB**. Current selection retains at most one large preview/detail pair, and closing the panel releases both. Process snapshots include allocator/driver caches, not pure per-tab costs or a long-duration leak proof. All memory measurements precede cache purging. Three-tab quiet foreground idle **20.0476 seconds / 0 measured CPU ms / 0 redraws**, no input or focus change.

**Verified:** 49 app Rust tests, strict all-target/all-feature Clippy, normal-feature compilation, formatting and optimized build. Ten release tab/memory/idle checks plus six release image/panel checks pass, including a 120-file scoped listing against independent directory truth, list/grid/content state, native accessibility rows, Cancel preserving all 240 owned files, close/quit, PNG/text/PNG selection and panel resource release. Light/Dark native captures reviewed at 125% SDR. No production Seek profile used, real deletion, repeated broad Stage A/B suites, packaging or framework upgrade. Remaining coverage limits: physical HDR/mixed-DPI, high-speed preview-transition recording, full-drive memory and other-machine/long-duration behavior.

Final release **19,876,864 bytes**, SHA256 `3AAB07193CB67876BD71BEA6FC26C69229B034468A52655FF7C46BC824EA39F8`. [Report and reproduction](docs/STAGE_C_POLISH.md), [measurement summary](docs/benchmarks/c-polish-summary.json). Stage C's measured requirements are satisfied on this setup; next is user visual review, with packaging still requiring explicit approval. Check by eye: drag the thin rounded scrollbar; move rapidly among PNG/text previews; Ctrl+T, Ctrl+Enter, Ctrl+Tab and Ctrl+W; return to scrolled tabs; centered Light/Dark glass dialog labels. Historical partial-status entries below are superseded by this follow-up. Existing unrelated files/deletion remain untouched.

# Seek v2 Stage C - features verified; performance acceptance partial

2026-10-19. Implemented the authorized session tabs and application text-button alignment. Tabs share one index, existing workers and active table; each retains its search, scope, view/sort, navigation history, expansion/selection and list/grid scroll position. Ctrl+T, Ctrl+Enter, Ctrl+Tab / Ctrl+Shift+Tab, Ctrl+W, native folder/search menu actions and close buttons work. Inactive tabs query current data when activated; closing the final tab quits. Permanent-delete, rename/new-folder and remaining content/result text buttons now use centered full-height labels, preserving the compact glass modal and safety behavior. No framework or dependency updates, no packaging.

**Verified:** 48 app Rust checks, strict all-target/all-feature Clippy, normal-feature compilation, formatting and optimized build pass. Nine final-release native checks pass in an owned temporary profile: independent Alpha/Beta queries; a scoped 120-file folder equals an independent listing; list/grid viewport restoration; content mode/query/scope restoration; Cancel preserves all 240 fixture files; inactive/active/final-tab close. Light/Dark glass captures reviewed at 125%. Three-tab foreground idle **20.003 seconds / 0 measured CPU ms / 0 redraws**, no input or focus loss. Three-tab fixture private commit **119.934 MiB**, working set **121.285 MiB**; not a long-duration leak proof. No production Seek profile was tested, and no broad Stage A/B deletion/content suites were repeated.

**Release speed, one-million-file fixture:** five warm-process launches improve from **317.951 ms median** to **253.232 ms** (247.177-255.715 ms). Broad-query worker p95 is **2.998 / 3.544 / 3.184 ms** for atlas/orbit/pixel. Native final-character-to-Present p95 remains **18.551 / 19.704 / 19.231 ms**, above 16 ms (before: 17.994 / 18.327 / 17.365 ms; 30 samples/query, three warmups). Query work is fast, but **the full UI target has not passed**; do not call Stage C fully closed. Post-search private commit is **105.000 MiB**, versus 110.301 MiB before, as process snapshots. Windows shell artwork now loads after the initial window; result handling avoids unnecessary path clones and shares date formatting within a batch, bounded to 512 timestamps. A focused cached-table experiment failed native accessibility lookup and was removed; the verified release keeps the original table rendering. Failed/partial benchmark harness attempts are excluded.

**Still needed:** focused main-thread/frame profiling for UI-tail latency and a true cold-system-cache launch measurement. This ordinary-user session could not certify a system cache purge; a first process/copied index is not labelled cold. Physical HDR/mixed-DPI is also unverified coverage. The app remains ordinary-user; no new administrator helper/action was added. Next: user visual review of this build, then the remaining performance work before claiming all Stage C acceptance requirements passed. Packaging still requires explicit approval.

Final executable **19,855,872 bytes**, SHA256 `4F9AAE190E1132C93F250B34C98226C53AA97D66906B960358BFB61F9E208D1A`. [Stage C report](docs/STAGE_C.md), [measured summary](docs/benchmarks/stage-c-summary.json). Check by eye: create/switch/close tabs, return to a scrolled folder/search in List and Icons, and inspect centered labels in the Light/Dark delete and rename dialogs. Existing unrelated user files/deletion remain untouched.

# Default-view selection corners fixed - v2 Stage B closed

2026-10-19. The Default view group had an 8-DIP rounded outline but explicitly square child buttons. Its end buttons now follow the outer outline, and the selected List/Icons/Gallery segment has a 7-DIP radius on all four corners, including the center Icons choice. Glass, label alignment and the existing layout-selection action remain intact. Five corrected-artifact native captures cover all three choices in Dark plus both ends in Light, at 125% scaling, using an owned temporary profile. Debug build, strict all-target/all-feature Clippy, normal-feature compilation, formatting and the optimized release rebuild pass. Release: 19,796,480 bytes, SHA256 `1F3C9D13F2BCC2D1DB929167B2008F5C477D9C5FCC1DBEBF24B2BFE90794638B`. No broad content, file-operation or idle suites were repeated for this visual-only change.

**Stage 2 means Seek v2 Stage B here: it is complete and can give way to Stage C in the next separately authorized run.** Required Stage A/B functionality and its targeted follow-ups are verified; no further mandatory Stage B test loop or administrator action is outstanding. Explorer/Chromium drag and clipboard tests are recorded; individual chat-app targets and physical HDR/mixed-DPI hardware remain unverified coverage, and two real PDFs still report explicit unreadable failures. These are documented limitations, not new Stage C work or hidden passing claims. Stage C is tabs and measured speed polish; it has not started. Packaging still requires explicit approval.

Check by eye: select List, Icons and Gallery and confirm each highlight stays rounded. [Settings follow-up](docs/SETTINGS-PANEL-ACCESS-FIXES.md), [Stage B status and historical evidence](docs/STAGE_B.md).

# Targeted Settings / info-panel / access-count follow-up - all three fixes verified

2026-10-19. Settings `0f42936`; remembered resizable info panel `8429b29`; retained-access-failure repair in this commit. The Save locations button keeps the common glass effect, and shared Settings labels are centered (Light/Dark native captures; ink midpoint matches button center). Info panel defaults to 280 DIP, supports the native double-headed resize cursor, remembers width across toggle/restart, and bounds itself while reserving list space. Native left/right drags, 324.8 DIP / 406 physical pixels after restart at 125%, and Icons/Gallery with a 520-DIP panel and 21 owned files pass.

The inflated inaccessible count is now explained and repaired. A read-only **copied** saved index retained 1,862 failures; independent listings found **1,620 vanished paths / 242 genuine access denials**. Ordinary Auto catch-up on that temporary copy removes stale errors, retries recovered folders and restores their descendants. The resulting **249 locations all independently deny access (error 5)**; the new copied checkpoint also stores only those 249. All six current-user Known Folders match independent direct listings: **714 entries, zero missing/extra**. Existing saved errors were retained indefinitely unless the exact path succeeded later; missing-path events also added new false warnings. Complete parent listings now clear removed child subtrees, and older helper missing-path responses cannot restore the false count. No hardcoded count, suppressed results, helper reinstall or ACL bypass.

Administrator token is **false**; the already-installed helper is connected as **ntfs + usn**, no error/fallback/reset/reconciliation. **No elevation is needed.** Genuine protected Windows folders remain inaccessible as expected. Tests use owned temporary profiles only; the production saved index was copied read-only for diagnosis, never opened as a test profile or written by tests. No Stage C, packaging or dependency updates.

Verification: 45 app Rust checks, four focused engine checks, strict all-target/all-feature Clippy, native Light/Dark/persistence/grid checks, formatting, optimized release build and normal-feature compilation pass. Final release native smoke restores the saved 520-DIP panel as 650 physical pixels at 125%. Executable **19,796,480 bytes**, SHA256 `E750D5635CDBAB39AFA5BF4EC061ECCCBA3F80482DFDEE3DF89DF429CE0DE57E`. A missing no-diagnostics metrics stub was also supplied for normal-feature compilation; that branch is excluded from the benchmark-enabled release runtime. [Report](docs/SETTINGS-PANEL-ACCESS-FIXES.md), [sanitized copied-index evidence](docs/benchmarks/saved-access-repair.json). The earlier claim that the high count was unexplained is superseded by this saved-history diagnosis. Known limits: the counter counts recorded path spellings (249 records / 248 case-insensitive spellings, one case alias), live-drive snapshots, no physical HDR/mixed-DPI check, no new idle CPU or release speed claim. No repeated content/deletion tests. Updated normal release app reopened for the user. Check by eye: Settings action glass/centered labels; drag the info border and reopen it; confirm only current access failures remain after normal catch-up.

# Targeted Settings / info-panel / access-count follow-up - first two fixes verified

2026-10-19. Settings glass/label fix committed as `0f42936`. Information panel now has the existing GPUI left-edge resize handle/native horizontal resize cursor, with default 280 DIP, bounded 240–720 DIP and 320 DIP reserved for the list. It remembers width across toggle/restart; writes use the existing writer only on drag completion. Native Light/Dark checks verify left/right drags, 374.4 DIP / 468 physical pixels after toggle and 324.8 DIP / 406 physical pixels after restart at 125%. Bounds and Settings round-trip checks pass. All 45 app checks and strict Clippy pass. [Report](docs/SETTINGS-PANEL-ACCESS-FIXES.md).

Next: finish the separate stale-access-error commit with copied-index before/after evidence and the optimized release artifact. Original app remains ordinary-user; administrator token check is false, helper is already connected, no elevation needed. No Stage C or packaging.

# Targeted Settings / info-panel / access-count follow-up

2026-10-19. Settings controls fixed: Save locations now retains the common glass gradient instead of overriding it with a solid accent fill. Shared text wrappers center labels; theme/dropdown text sizing is consistent, and the navigation text offset is removed. Native Light/Dark captures at 125% scale pass; Save/Rebuild text ink midpoint 580.5 px matches the button center. [Report](docs/SETTINGS-PANEL-ACCESS-FIXES.md).

Next in this authorized follow-up: finish native info-panel resizing/persistence checks, then commit the diagnosed stale-access-error fix. Read-only copied saved-index evidence identifies 1,620 nonexistent paths among 1,862 saved failures; repaired copied-index catch-up gives 249 independently confirmed access-denied paths and all six favorites match independent listings (714 entries, zero missing/extra). No administrator access is needed. No tests run against the production data folder; no Stage C or packaging.

# Ordered reliability follow-up - all four items verified

2026-10-19. Commits: retry `6c1fbc4`, centered glass confirmation/row alignment `407e6a0`, retained document text `a14d0c1`, status diagnostic in this commit. No Stage C, packaging, framework upgrades or production Seek profile testing. Real Downloads/Documents and C: were read only; all testing used owned temporary data.

- **Retry:** three uncached release `note` runs each give 150 identical paths / 437 candidates, zero duplicates. Six first failures in 1,098 document attempts (0.546%); all were the same two persistently unreadable PDFs, still failed after retry and now listed by name/error. First matches 769.426 / 378.421 / 334.396 ms, complete 44.112 / 25.355 / 23.346 s. Controlled tests verify successful transient recovery; no real transient failure was observed.
- **Confirmation and latest visual fixes:** previous compact presentation retained, centered with translucent glass when enabled and an occluding dim backdrop. Default Cancel, Tab cycle, Escape, default Enter/Space, captured names/count and one-shot red action checked using disposable files. A six-file confirmed removal generated one acceptance event and immediate list refresh. List file/folder icons and text now center in the unchanged 30-DIP row. Light/Dark, minimum-width 960-DIP window at 125% scaling and opaque fallback inspected.
- **Retention:** already default-on without age expiry; only description changed. Five Rust cache checks, 30 known-answer off/cold/restarted-cache path comparisons and six mutation/scope checks pass. An unchanged document with usage timestamp 1 survives reopening; deleted/out-of-scope derivatives are pruned. Clear and 128 MiB LRU remain.
- **Status diagnostic:** installed helper Running/Automatic, actual normal Auto connection **ntfs + usn**, no helper error/fallback/reset. Fresh full-C Auto counts **221 inaccessible paths**, independent listings fail on all 221 (Windows error 5). Walker counts **211**, all independently unreadable (209 error 5 / two error 3). Zero counted paths were readable. The earlier 1,859 is **not reproduced or explained** by current evidence; production data was not opened to reconstruct it. No unsupported no-helper/cloud/regression attribution. The misleading zero came from displaying only journal-change count during other work; phase labels now distinguish reading/saving/replay, with nonzero journal work called changed folders. Tooltip explains unique failed locations rather than skipped files or only ACL denial. No inaccessible-path listing UI added.

Verification: all **44 app Rust checks**, strict all-target/all-feature Clippy, formatting and optimized release build pass. Final-release launch/confirmation/default Cancel focus/Cancel smoke passes in an owned temporary profile. Final executable **19,763,712 bytes**, SHA256 `5FED2CB883EF846E67DABE40FECB5ADFE741C1A8DC486C303276BF52D48E28C4`. Broader native visual/safety checks were debug; content timings above are the item-1 release measurements. [Full report, limits and measurements](docs/ORDERED-RELIABILITY-FOLLOWUP.md), [counter summary](docs/benchmarks/access-status-diagnostic.json).

Known limits: two real PDFs remain unreadable; the historical 1,859 count has no captured error-map evidence; inaccessible-path comparison covers every recorded failure at observation time, not a frozen whole-drive file-by-file truth snapshot. No new HDR/mixed-DPI, idle-CPU or performance claim for styling/copy/status changes. Existing unrelated deletion, scripts and icon files preserved. Check by eye: centered glass confirmation, Cancel focus, file-row alignment, Settings retention copy, content unreadable list and informative catch-up phase. No further stage starts.

# Ordered reliability follow-up - items 1 through 3 complete

2026-10-19. Item 2 committed as `407e6a0`. Item 3 verifies already-existing default-on, no-age-expiry cache retention and changes only the Settings explanation. Clear cache and 128 MiB LRU remain. Five Rust cache checks pass, including unchanged text with usage timestamp 1 surviving a reopened session. Owned process-restart checks pass 30 known-answer cache-off/cold/warm path-set comparisons plus six mutation/scope checks; removed scope prunes four cached derivatives. No new real-folder rescan was necessary for this copy-only change. Next: item 4, diagnose the inaccessible-location counter and helper connection against independent truth. [Report](docs/ORDERED-RELIABILITY-FOLLOWUP.md).

# Ordered reliability follow-up - items 1 and 2 complete

2026-10-19. Item 2 restores the compact permanent-delete alert presentation, now centered with a dimmed blocking backdrop, translucent glass surface when enabled, default Cancel focus, red Delete permanently action, selection count/first three names/remaining count and one-shot captured-selection worker dispatch. Enter/Space at default focus, Cancel, Escape and backdrop checks preserve disposable files. Tab cycles both buttons. Confirming six owned files removed exactly six and emitted one acceptance event; the list refreshed immediately. Light/Dark glass, opaque fallback and a minimum-width 960-DIP window were visually checked at 125% scale. File/folder icons and names now center vertically in the unchanged 30-DIP list rows; other columns share the same centering. Selection-summary Rust check and strict Clippy pass. Debug desktop checks only; no new performance or HDR/mixed-DPI claim. Real Seek data untouched.

Item 1 is committed as `6c1fbc4`; item 2 is in this commit. Next: verify the existing cache-retention behavior and clarify its description (item 3), then diagnose the inaccessible-location counter (item 4). [Report](docs/ORDERED-RELIABILITY-FOLLOWUP.md). No Stage C or packaging.

# Ordered reliability follow-up - item 1 complete

2026-10-19. Content failures now get one deferred retry after every initial candidate has been searched. Only failed retries count as unreadable; **Unreadable files (N)** displays full paths, errors and attempt counts. Existing scope/secret/size/per-attempt time limits, cancellation and unique-file progress remain intact.

Verified: nine content Rust checks, native owned PDF/DOCX error-list check, strict Clippy and optimized build pass. Three separate **uncached release** scans of real Known Folder Downloads and redirected Documents each returned **150 identical `note` paths, zero duplicates, 437/437 candidates**. First matches **769.426 / 378.421 / 334.396 ms**, complete **44.112 / 25.355 / 23.346 s**. First attempts failed **2 of 366 documents per run (6/1,098 = 0.546%)**; all six were the same two persistently unreadable PDFs, retried once each and still unreadable. **No transient first-attempt failure occurred in these three real runs**; deterministic owned tests prove recovery and retry order. The two files are now visible by path/error in the UI. No unconditional PDF recall claim.

[Report](docs/ORDERED-RELIABILITY-FOLLOWUP.md), [sanitized release measurements](docs/benchmarks/content-retry-summary.json). Release SHA256 `3DDF92DA8ABEB129E32CB9A11AFF2F953C3844D0BAB200F48914C29319FB9535`. Real documents were read only; separate temporary data, no production profile testing. No Stage C, packaging or dependency upgrades. Existing unrelated user files remain untouched.

Next: item 2, centered safe permanent-delete confirmation; items 3 and 4 follow after its commit. Historical sections below retain prior behavior and measurements.

# Extracted document text cache and confirmed permanent deletion - complete

2026-10-19. Implemented only the approved PDF/DOCX cache and the separately clarified permanent-delete command. No Stage C, packaging, framework/Windows crate updates or testing against the real Seek data directory. All app/cache test profiles are separate owned temporary folders under the user profile; real Known Folder Downloads and redirected Documents are read only for the authorized measurements.

- **Cache:** default-on **Remember document text for faster content search**, **Clear cache**, displayed **128 MiB** cap. Opportunistic extraction only during existing searches; no startup indexing/task or resident process. Deflate + DPAPI CurrentUser for text and manifest; protected current-account-only ACL and inherited file ACLs verified. Path/size/precise mtime/extractor version keys, PDF filter fingerprint, and a fresh scoped source read/hash on every hit prevent stale reuse even after same-metadata edits. Changed/renamed/moved/deleted/removed-scope entries are discarded. Off clears; Clear cancels outstanding work and does not immediately refill. Cache errors fall back to originals with an explicit status.
- **Real release correctness:** 437/437 candidates; final uncached, fresh-cache and restarted-cache answers equal exactly, **150 `note` / 1 `rudkin`, zero duplicates**, with 535 skipped and two unreadable PDFs explicitly shown. Four Rust cache tests pass, plus **10 known-answer cases in 3 cache modes (30 comparisons)** and six owned mutation/scope assertions across separate processes. Native settings default/clear/off/restart/re-enable, Light/Dark glass, account ACLs, release Complete and keyboard-focus checks pass.
- **Real release times:** uncached `note` **412.907 ms first / 28.612 s complete**; empty cache **468.922 ms / 42.538 s**; full cache after restart **108.367 ms / 1.018 s**. Uncached older `rudkin` **27.433 / 28.063 s**, full cache **940.358 ms / 1.039 s**. First fill saves 364 readable PDF/Word entries; repeats hit all 364 with zero writes. Complete repeats are about 28x / 27x faster than the uncached control; filling does not speed up the first extraction.
- **Known failed sample retained:** the first post-build empty-cache `note` run was **1.231 s / 58.272 s, 146 matches and 6 unreadable**, four additional PDFs. It did not pass equality. Those extractions succeeded later; the fresh-cache repetition and uncached control both returned all 150. The cause of the transient native extraction failures was not isolated. Existing unsupported/offline/oversized/protected-file skips and two unreadable PDFs remain; no unconditional PDF recall claim.
- **Storage/memory/idle:** 364 encrypted payloads + manifest, **2,380,558 logical bytes / 3,178,496 allocated bytes (3.031 MiB)**. Diagnostic parent peak private **12.398 MiB empty / 10.059 MiB full**, excluding PDF child hosts. Release GUI private **90.551 MiB** with a small owned index and real content scopes. Foreground post-search idle **20.027 s / 0 measured CPU ms / 0 redraws**, no input, normal close **exit 0**. The earlier interrupted sample is excluded from idle evidence.
- **Delete distinction, explicitly authorized by the user:** ordinary Delete / Move to Recycle Bin goes directly to Recycle Bin and retains the guard against permanent fallback. **Shift+Delete / Delete permanently...** shows a native GPUI confirmation; only OK queues worker IFileOperation permanent deletion of the captured selection. Cancel/Escape preserve files. Owned native checks prove actual recycle entry versus confirmed permanent removal, immediate list refresh, search text focus and readable smaller-window confirmation. No real user document was deleted in testing.

Strict all-target/all-feature Clippy, normal-feature check, formatting and optimized build pass. Release **19,746,816 bytes**, SHA256 `C4F00ED7878E962C05D567910ADAEE15B96397197BF0F87D105D1C53A6D8ADDA`. [Full report and reproduction](docs/DOCUMENT-TEXT-CACHE.md), [sanitized measurements](docs/benchmarks/document-text-cache-summary.json).

Known limits: the initial transient PDF failures above; two persistently unreadable PDFs; diagnostic memory excludes short-lived PDF hosts; GUI memory is not a full-C-index measurement; no other-account login, physical HDR/mixed-DPI or other-machine verification. Existing user-owned scripts/icons and unrelated deleted document are preserved. Next: check the Settings switch/limit/Clear, repeat content searches ending Complete, and the two deletion choices on disposable files. Await a new instruction; no next stage starts automatically. Historical sections below retain their old cache-not-approved / recycle-only wording and are superseded by this explicit authorization and implementation.

# Icon and content search - both tasks complete

2026-10-18. Completed in order: **icon `d69ba9d`; content search in this commit**. No Stage C, packaging, framework/Windows crate upgrades or real Seek profile use. All GUI apps used separate temporary data; real Downloads and Windows-redirected Documents were read only for the authorized content measurements.

- **Icon:** supplied artwork, seven 32-bit ICO frames (16/24/32/48/64/128/256), 163,591 bytes; executable resource and window/taskbar icons. Native 16/32 px proofs and actual Light/Dark taskbar + Explorer checks pass at 125% display scale. White symbol stays clear; black outer edge partly blends on dark surfaces. Windows theme restored. Final optimized executable's native icon extraction also passes. [Icon report](docs/APP-ICON.md).
- **Content:** finite scoped Windows Search host streams provisional matches labelled **Quick results (may be incomplete)** while the complete direct scan continues. Normalized paths deduplicate; verified lines replace placeholders; the final direct answer alone is labelled **Complete**. Query edits, Cancel, protected paths, file caps and ordinary-user operation remain intact. No resident process or cache was added.
- **Real RELEASE measurements:** 437/437 files, 150 `note` matches and one older `rudkin` match; final path sets equal the baseline with zero duplicates. Before: `note` **354.959 ms first / 23.240 s complete**, `rudkin` **22.504 s / 23.028 s**. First post-build quick runs: **547.775 ms / 40.440 s**, **258.080 ms / 25.761 s**. The older match is about 87 times sooner. Because the first common-word complete scan was slower, one same-executable warm control measured direct **425.001 ms / 28.822 s** versus quick **259.670 ms / 28.739 s**. No complete-scan speedup claim; native PDF/system variability remains unisolated. Each scan visibly reports 533 skipped and two unreadable PDFs.
- **Catalog completeness:** quick runs return 122 / 1 matches; a separate SQL snapshot returned 121 / 1. Its 29 common-word gaps include 15 embedded-only substring matches (Windows uses word prefixes), 12 Markdown/Python body matches with no registered extension handler, and two Word discrepancies. One Word miss resolved on requery; the other still lacks some body terms. Its internal filter cause is not established. The direct scan remains authoritative.
- **Cache assessment only:** 364 readable PDF/Word files yield 5,804,048 UTF-8 text bytes. Including estimated keys: **5.566 MiB raw / 1.973 MiB compressed**. Ten RAM-only repetitions give **17.831 / 16.993 ms medians** for the two words. These are CPU lower bounds, not disk-cache timings. No extracted text was cached on disk; implementation still needs approval.

Verification: **10 Rust checks, 9 targeted desktop checks, 10/10 final-release accuracy cases and 3/3 final-release UI checks pass**, plus strict Clippy, normal-feature check and formatting. Actual UI quick-to-Complete transition returns all 150 matches. Post-completion process idle sample **10.012 s / 0 measured CPU ms**, normal close exit 0; foreground quiet/redraws were not separately asserted. Final release **19,586,048 bytes**, SHA256 `4F2C1B5D05826697FD02DD782D5D114B9C1BA0E46E9C9D9754A0741207676BF0`.

[Measurements, explanation, reproduction and limits](docs/CONTENT-QUICK-RESULTS.md), [sanitized numbers](docs/benchmarks/content-quick-summary.json). Known limits: two PDFs unreadable; unsupported/offline/protected/oversized entries explicitly skipped; no physical HDR/mixed-DPI or other-machine check; real disk-cache latency unmeasured. The large Excel preview request was outside these two changes and was not retested. Existing user-owned scripts/icons and unrelated deletion remain untouched.

Next: check the icon in Explorer/taskbar, quick label and provisional lines becoming verified lines, Complete/final counts, and Cancel/new typing. Await separate approval before any persistent text cache. No stage starts automatically. Historical sections below retain their earlier wording and measurements.

# Icon and content search - Task 1 complete

2026-10-18. Supplied app icon converted to **seven 32-bit sizes (16/24/32/48/64/128/256), 163,591 bytes** and installed as the executable resource and DPI-sized window/taskbar icons. Exact 16/32 px native extraction and actual Light/Dark taskbar + Explorer checks pass at 125% display scaling. The white symbol remains clear; the black outer edge partly blends on dark surfaces. Original Windows theme restored, temporary app closed normally, real Seek data untouched. Debug build and strict all-feature Clippy pass. [Evidence and reproduction](docs/APP-ICON.md).

No unrelated app changes, Stage C or packaging. Existing supplied alternative icons/scripts and unrelated deletions are preserved. The optimized artifact will be rebuilt and verified for Task 2's RELEASE baseline; no current release icon claim yet. Next: commit Task 1, then only the requested quick/complete content search, real-folder release measurements and cache evaluation (no persistent cache implementation).

# Ordered tasks - all three complete

2026-10-18. Completed the three requested tasks in order, with a commit after each. **Task 1 `38a8141`; Task 2 `a7a3d73`; Task 3 in this commit.** No Stage C, packaging, framework update or real Seek data-folder testing.

- **Sidebar:** navigation retained Content/Largest mode and browsing waited for global index publication; an unindexed folder looked empty. Navigation now resets the mode and complete metadata arrives on finite priority/demand workers, with distinct loading/empty/error states. One current-user Windows Known Folder resolver handles actual OneDrive redirection. All six sidebar folders and both location roots tested match independent directory truth with zero missing/extra visible entries. Fresh backend publication **7.403 ms before global readiness**; non-C tested through an owned SUBST alias, not another physical disk. [Task 1 evidence](docs/TASK1-SIDEBAR.md).
- **Content:** no 3-second, 30k-entry or 200-match scan stop. Newest-first, searched/total progress, Finished/Cancelled, protected scopes and per-file limits. Actual Downloads/Documents `note`: **150 matches, 437/437 attempted, 459.640 ms first / 34.791 s finished**; `invoice`: **2 matches, 437/437, 20.756 / 39.818 s**. Each reports 539 skipped entries and 2 unreadable. Windows Search catalog **122 / 2 matches in 840.978 / 118.363 ms**; metadata covers 437/437, but word/prefix semantics and literal-match coverage differ. Direct scan remains authoritative; no general/failed-file recall claim. Debug single samples, PDF extraction dominates. Seven Rust and nine desktop checks passed; final-release known-answer accuracy **10/10**. [Task 2 evidence](docs/TASK2-CONTENT.md).
- **Info-panel previews:** built-in Windows PDF page rendering/navigation, calamine first-sheet data table, meaningful Word shell thumbnail or bounded text fallback. Selected item only, 32 MiB input / 4-second killable host / 256 MiB child-memory bounds, explicit unavailable states, cancel/free on close, unchanged row icons. **16/16 production host cases, 14 Dark + 13 Light desktop checks, and 5/5 production smoke checks pass.** Scoped visual review **ship** at 125% SDR. [Task 3 evidence and limits](docs/TASK3-PREVIEWS.md), [summary](docs/benchmarks/task3-preview-summary.json).

Final optimized build, strict Clippy, normal-feature check and formatting pass. SHA256 `8691609826B7E9013BEA441088BAFA1939630D047842AF7EC852D3B0F62986A7`. Foreground selected-PDF debug sample: **30.079 s, zero measured CPU and redraws**; two native activity events split strict quiet at **19.671 s**, so the old 20-second strict-quiet gate remains false. No uninterrupted-30-second claim and no idle rerun. Private commit **88.89 MiB** in that sample. Every app used separate temporary data; diagnostic hosts exit before profile initialization. Historical records below retain their earlier measurements/deferrals and are superseded by this closeout.

Known limits: physical HDR/mixed-DPI, another physical drive and an Office-uninstalled machine were not checked; genuine encrypted Word/Excel specimens were not available. Encrypted PDF and broken files are verified. Content skips unsupported/protected/offline/oversize files visibly; two real files were unreadable. Full no-helper C: catch-up performance is unchanged from historical reports.

Next: user checks the six sidebar locations, streaming content progress/cancel, PDF page controls, Excel values/data-only label, Word thumbnail/fallback and panel close in Light/Dark/glass. No next stage starts automatically.

# Ordered tasks - Tasks 1 and 2 complete

2026-10-18. Task 1 committed as `38a8141`. Task 2 removes the 3-second, 30k-entry and 200-match scan cutoffs; scans newest-first until complete/cancel/new typing with searched/total progress and explicit Finished/Cancelled. Hydrated OneDrive and extended local paths work; protected-key names, scope checks and per-file caps remain enforced. No Stage C, packaging or dependency upgrades.

**Task 2: 7 content Rust checks, 10/10 known-answer DOCX/PDF cases and 9 targeted desktop checks pass** (including progressive rows, 290 matches, real denominator, explicit cancellation and close). Strict Clippy, normal-feature check and formatting pass. Actual Known Folder Downloads/Documents: `note` **150 matches, 437/437 files, first 459.640 ms, finished 34.791 s**; `invoice` **2 matches, 437/437, first 20.756 s, finished 39.818 s**. Each reports 539 skipped and 2 unreadable; no unsupported/failed-file recall claim. Debug measurements, one sample each. PDF extraction dominates.

Windows Search metadata covers **437/437** eligible paths; scoped content queries **840.978 / 118.363 ms**, **122 / 2** matches. It is faster but does not reproduce all 150 literal `note` matches; word/prefix semantics differ, and metadata coverage does not prove body completeness. The direct scan remains authoritative. [Task 2 measurements and limits](docs/TASK2-CONTENT.md), [summary](docs/benchmarks/task2-content-summary.json). All test apps use separate temporary data; the real Seek data folder was not touched.

Next: commit Task 2, then implement only Task 3 selected-document info-panel previews.

# Ordered tasks - Task 1 complete

2026-10-18. Task 1 sidebar population is fixed and verified; Tasks 2/3 have not started. Two old-release reproductions confirm retained Content mode and published-index-only browsing returning empty for real folders outside the index. Sidebar now exits Content/Largest modes, displays a loading state, and gets complete directory metadata on finite workers before the global index is ready. One shared current-user Known Folder API resolver handles redirection and omits unavailable folders.

Three new Rust/integration checks pass. Fresh backend: 2 rows in **7.403 ms with global index not ready**. All six current-user sidebar folders (including actual OneDrive redirection), the location root and a temporary non-C drive-letter root match independent listings with **zero missing/extra visible entries**. Empty/error/loading states, both mode exits and close pass. Strict Clippy, normal-feature check and formatting pass. These are debug measurements; only a physical C: volume is present, so non-C uses an owned SUBST alias. Every app test used temporary data; the real Seek data folder was not used. No framework updates, Stage C or packaging.

[Task 1 evidence and limits](docs/TASK1-SIDEBAR.md), [summary](docs/benchmarks/task1-sidebar-summary.json). Next: commit Task 1, then Task 2 content completeness and Windows Search comparison; Task 3 follows only after Task 2 is done and committed.

# Seek v2 - Stage A/B search and test follow-up complete

2026-10-18: fixed Name contains ranking and Enter accepting the visible default suggestion; content results stream before scan completion, with text/DOCX ahead of PDFs per directory; PDF chunk boundaries and UTF-16 handling are corrected. No Stage C, previews or packaging added.

**16 focused Rust checks, 10/10 known-answer DOCX/PDF cases, 7/7 release desktop checks and a native-file-to-browser paste check pass.** Existing Stage A full suites and Stage B's sustained zero-CPU/redraw idle samples were audited and retained, not repeated. On configured real Downloads/Documents, `note` first matched in **120.049 ms**; the scan ended at **3,023.163 ms with its limit reached** (154 checked, 47 matches, 1 unreadable). This remains a partial scan; no full recall claim. Physical HDR/mixed-DPI and individual chat-app targets remain unverified. The known slow full no-helper catch-up is unchanged.

Strict Clippy, normal-feature check, formatting and optimized build pass. Release SHA256 `D129CA3BC76F4ED28E0E2CD1CAB23C58A16D763D20C0FD3DD158C4D35CB33805`. Rich Word/PDF previews are feasible and both handlers are registered, but registration alone is not a rendering check. [Full follow-up, remaining-check audit and preview feasibility](docs/SEARCH_FOLLOWUP.md), [summary](docs/benchmarks/search-followup-summary.json).

Next: user review; Stage C or rich previews only after separate authorization. Older wording below retains historical results and deferrals; the current audit above supersedes those pending notes.

# Seek v2 - drag follow-up complete

2026-10-17: native external file Copy drops now use a shell IDataObject and Windows OLE on the source window thread, after GPUI releases its typed drag and mouse capture. Normal builds include it without an experiment flag. No framework update or patch, no polling worker, no Stage C or packaging.

**Final release: 7/7 targeted interaction checks pass, plus 3 clean exits.** Strict Clippy, normal-feature check, formatting and optimized build pass. Release SHA256 `6ABFC929F433F250F9F0C7DF240A97E6A80DAEB1353F819A0FBD886392802F88`. No repeated broad suites.

Fresh owned-fixture checks verify both internal move targets (folder row and sidebar). External Explorer and Chromium file input/HTML DataTransfer checks verify actual copied file hashes. Multi-selection and Escape cancellation are included. The original Stage B experiment's unsupported status below is historical and superseded by this follow-up. [Implementation, release evidence and limits](docs/DRAG_FOLLOWUP.md).

# Seek v2 - Stage B closed

Updated 2026-10-17. Delivered on-demand scoped content search for text/code/Markdown, DOCX and registered-filter PDF text, cancellable per edit, with matching lines, configurable Downloads/Documents defaults, protected-file exclusions and visible limits. Centered Info artwork and lighter Light-menu tint are included. Framework pins and file-operation safety remain unchanged. No Stage C or packaging.

- **7 new focused Rust tests, 9/9 targeted debug desktop checks and 4/4 final release smoke checks pass**. Strict Clippy, formatting, normal-feature check and optimized build pass. Small final-release PDF query **115.794 ms**, F5 rescan **55.471 ms**, zero read errors; not a universal latency claim.
- Outstanding Stage A idle follow-up now passes on final release: **29.992 s / 27.306 s** quiet foreground on 10k/1M saved fixtures, each **0 ms measured CPU and 0 redraws**, both close with exit 0. One sample each; existing Stage A suites were not repeated.
- The bounded OLE drag experiment was stopped: no clean source handoff or Explorer copy was established, source preserved. External drag is unsupported and diagnostic-only; browser/chat drag was not attempted. Native Copy/Cut/Paste remains the supported handoff.
- Independent capture review found no material scoped defect at **125% SDR**; physical HDR/mixed-DPI remains unverified. Content is bounded, no OCR, requires an installed PDF filter, and skips reparse/provider placeholder paths. Prior slow no-helper full-C catch-up remains a known limitation.
- Release SHA256 `72AD5AB678DDF453E158CD60CD2C89153E0528006A3ACFA327084D0E4F2548DF`, 17,971,712 bytes. [Stage B report](docs/STAGE_B.md), [measured summary](docs/benchmarks/v2-stage-b-summary.json).

Next step: Stage C only on a separately authorized run. No packaging without explicit approval. Historical Stage A and v1 records below retain their original wording and artifact hashes; the fresh idle follow-up above supersedes the earlier deferral.

# Seek v2 - Stage A closed

Updated 2026-10-16. Stage A daily-use basics are implemented and closed. The user requested that repeated testing stop; this closeout records existing evidence and does not run more tests. Stage B is next, with no B/C implementation or packaging included here.

Delivered: priority folder catch-up and real indexing progress; Type menu/search chip/suggestions; GPUI file/actions menus; native Windows file clipboard and focus-aware shortcuts; worker IFileOperation rename/new folder/Recycle Bin with collision prompts; typed internal drag moves; top-200 Largest files/folders with All items return. The final light-menu frosting, preview quality, toolbar icon centering and Appearance navigation alignment fixes are included. Dependency pins and the installed helper remain unchanged.

- **91 Rust tests and 30/30 final release desktop checks pass**, including actual Explorer paste, collision Skip preserving both files, both internal drop targets, previews settling and closing the process. Formatting, strict Clippy, normal-feature check and optimized all-feature build pass. Release SHA256 `607503F85EA19235D2BD73D916B36A2598CE41452E0862841481511F6178704E`.
- Real C: priority folders are current and presented in **316-373 ms with the helper / 881 ms without it** in the measured after samples. Ordinary helper full catch-up is **2.143-2.291 s**; a recovery sample took **32.651 s**. The correct full no-helper relist took **140.524 s**, slower than the earlier **57.721 s** baseline. This remains a known background performance limitation, not a claimed speedup.
- Closed-app correctness: the independently listed owned real-C subtree has **11 entries and zero missing/extra/stale/denied**, both sources. Before/after synthetic tests also have zero discrepancies across first/live/closed/corrupt history with **4,932 live and 20 closed operations** each. This does not independently certify every path on C:.
- Largest on 1,001,008 indexed entries: files median **48.806 ms / p95 57.750 ms**; folders **5.774 / 6.479 ms**, 30 samples each. Existing large-file size rounding is disclosed in the UI.
- **Fresh sustained Stage A foreground idle sampling was not rerun at the user's request.** The v1 zero-CPU/redraw results below remain historical. Physical HDR/mixed-DPI appearance and true cold-cache launch remain unverified; current visuals were checked at 125% SDR. Browser/chat file paste is not independently verified.

[Full Stage A report and limits](docs/STAGE_A.md), [machine-readable summary](docs/benchmarks/v2-stage-a-summary.json). Next step: Stage B on the next authorized run; no more Stage A test loops. No packaging without explicit approval.

## Finished v1 baseline

# Stage 4 - implementation and available-hardware validation complete

Updated 2026-10-16. The toolbar action/view buttons now share the same accent selection/press fill. Startup overlaps existing index loading with native UI/artwork initialization. **83 Rust tests and 30 focused release checks pass**, with formatting, strict Clippy and both feature configurations verified. GPUI/Kit/patched Windows and the installed helper remain unchanged.

The foreground-idle blocker is resolved on the final release: **30.132 s / 30.024 s** quiet foreground (10k/1M), each zero measured CPU time and zero redraws. Independent million-file truth again matches **1,001,001 entries**; both 49,288-operation stress sources and helper Unicode/restart lifecycle pass with zero discrepancies. Source/release evidence and raw intervals: [Stage 4 report](docs/STAGE4.md), [summary](docs/benchmarks/stage4-summary.json).

Saved launch medians are **321.207 / 317.699 ms**, slightly above the approximate 300 ms goal. Broad-query UI p95 is **16.896 / 17.805 ms**, so no universal sub-16 ms claim. Final real-folder requests stay under 100 ms; live scrolling, themes, settings persistence/relaunch and complete exit pass. The normal app is reopened and responding. SHA256 `8ad37a41061f218e4c5a39a2cefe47fdfa38d8cad61643835cecb2f6c111ff31`.

Display coverage: actual **125% SDR**, plus 100/125/150/200% native-handler renderer simulations in both themes. **HDR appearance and physical mixed-DPI movement remain unverified**: the connected panel reports HDR Not Supported and no suitable second display is present. Other devices/wallpapers/Mica fallback and physical power-loss testing remain outside the verified setup. Later ideas remain deferred.

## Historical navigation follow-up and earlier acceptance records

The sections below retain their original wording and artifact hashes; current results are above.

Latest follow-up, 2026-10-16: the reported folder delay, scroll jumping and hidden-selector alignment are fixed. Direct indexed children skip unrelated live-overlay branches, expanded requests retain their view identity, and background selection restoration preserves the viewport instead of revealing the selected row. FIFO/epoch completion markers let catch-up finish on a continuously active drive; actual recovery remains visible. Selector labels align left and other button labels remain centered.

**83 tests pass**, final Clippy/formatting, normal-feature check and optimized build pass; release navigation GUI **5/5** passes, including live additions/deletion while scrolled to the bottom. Real Downloads/Desktop/Documents/Pictures/Videos clicks: median **14.233 ms**, maximum **18.501 ms** for 12 observed requests, all under the 100 ms guard. Bulk updates now build outside the exclusive lock with a shared immutable mmap, then swap briefly; old snapshot cleanup also runs outside the lock. Earlier 1.402 s / 2.085 s stalls are retained as failures. This is a small interaction sample, not a universal latency promise. The ordinary real-profile app is reopened after verification. [Follow-up details and limits](docs/benchmarks/stage4-navigation-report.md). Current binary SHA256 `AED4DAC8897E283BEC4F7DC05D54313DDB060E1664FCA4E58B84B062884CB3C4`. Earlier Stage 4 idle/startup acceptance limits remain open; no full benchmark was repeated and no framework/helper installation changed.

## Prior Stage 4 baseline - 2026-10-15

Updated 2026-10-15. Stage 4 is authorized. The requested Settings alignment, custom-accent visibility, immediate default-view update and stronger Settings selection states are implemented. Final optimized GUI checks pass **6/6 + 7/7**, including saved locations/exclusions and relaunch. The delegated reviewer hit its quota; the bounded final screenshot review was completed inline and disclosed.

- **78 Rust tests pass**, one existing benchmark ignored. Formatting, all-target/all-feature Clippy with `-D warnings`, debug workspace and optimized workspace builds pass. The final optimized app hash is `6f07b9f048ccddc9192db9f2acf9300137d3a144a033250dac133135a6bbd8d7`; [summary](docs/benchmarks/stage4-summary.json), [report/build/reproduction](docs/STAGE4.md).
- Saved mmap validation rejects corrupt dimensions/references, while atomic snapshot/cursor replacement preserves old readers. Corrected an initial overly strict duplicate-name ordering check; the new ranked-folder regression and real 3,281,665-entry saved-index load pass. No index-format change.
- Bounded filesystem-event coalescing preserves checkpoint/control ordering; both final 49,288-operation stress runs pass first/live/closed/corrupt-history truth with zero discrepancies. Concurrent query p95 **1.2498 / 2.2837 ms** (walker/helper). Removed an unused growing subtree queue and capped the derived folder-prior cache at 512 entries / 512 KiB, without dropping indexed data.
- Retained million-file independent truth: **1,001,001 entries, zero discrepancies**. Final-artifact 10k/1M benchmarks each pass seven query cases with 30 measured UI/backend samples. Final 1M UI medians are 6.4-13.0 ms, all p95 below 15 ms; the 10k common-query p95 is 16.533 ms, so not every sample meets 16 ms. The scoped 1M broad-query backend is 23.411 ms median / 28.786 ms p95.
- Five-run saved launch medians **372.926 / 347.269 ms** (10k/1M), above the earlier ~300 ms goal. OS caches were not flushed. Private working set **36.39 / 51.93 MiB**, mapped indexes counted separately. Whole-C: copy launch and all closes pass; first history recovery is slower and reported separately. No repeated first-index optimization work.
- Sustained quiet foreground idle remains **unverified**. Earlier strict samples lost foreground focus or received native input. The final bounded sampler retains all intervals and separates stable background from foreground periods using native-activity/query/status/focus evidence; CPU and redraws are outcomes and never reasons to discard intervals. See the exact longest segments in `benchmarks/stage4-quiet-idle.json`. The final sample records 1.198 s quiet foreground and 20.590 s quiet background, both zero CPU/presentations. No focus lock, global input blocking or Windows policy change was used. A background result does not satisfy the foreground gate.
- Exact next step: Obtain one valid uninterrupted 20-second focused, untouched sample after startup settles; assess the retained above-target startup distribution separately. No production fix is inferred from focus-contaminated samples. GPUI/Kit/patched Windows revisions and installed helper are unchanged. Other DPI/HDR screens, arbitrary wallpaper/Mica fallback and physical power-loss durability remain unverified. The Later ideas remain deferred. The final ordinary real-profile app is reopened and responding.

## Accepted Stage 3 baseline

# Stage 3 - complete

Updated 2026-10-15. Stage 2 is accepted. Stage 3 product implementation and the requested visual redesign are in place. GPUI Kit 0.7.1, GPUI revision 279fe070bb389b79652e52065b2f001edcc0b11b and the patched Windows crate are unchanged. Stage 4 and indexing-speed work have not started.

Final Settings follow-up: recreated the supplied four-page mockups with a frosted rounded sheet, left navigation, separate cards and neutral glass controls. System/Light/Dark use fixed **16 px thin icons**. The rounded List/Icons/Gallery frame ends at Gallery. Graphite is **#909090** (interpreting "90"); old #707070 presets migrate, while other custom colors remain. Presets, custom hex and the real color-picker popup work; accent stays limited to selected controls, the glass switch and Save locations. Existing helper actions, indexing and framework pins are unchanged.

- Focused validation: **15/15 app tests**, all-target/all-feature Clippy with `-D warnings`, formatting and workspace debug build pass. Final optimized workspace build passes in **291.536 s**. Exact commands: `stage3-settings-{tests,validation,release}.json` in `docs/benchmarks`.
- Debug desktop checks: **7/7**, including picker/hex, visibility, all views, locations/exclusions and restoration after complete exit. Small-fixture private working set **34.46 MiB**, private commit **88.52 MiB**; close **78.414 ms**, exit 0. Final release desktop checks also pass **7/7**: private working set **38.00 MiB**, private commit **91.84 MiB**, close **89.168 ms**, exit 0. The helper action is compact in both themes; independent visual review disposition is **ship**. The real-profile release app was reopened. Current binary hash and evidence: `docs/benchmarks/stage3-summary.json`. Prior 70-test/23-check product and engine baselines below are retained; no whole-drive benchmark was repeated.
- Failed harness attempts are retained as `stage3-settings-ui-{first,second}-failure.json`: an assertion used an edit's accessible name instead of its value; a later run found Locations open during the picker step. Correct value assertions, bounded fresh-control lookup and an uninterrupted run pass. Neither failed attempt is reported as acceptance.
- Remaining visual limits: other DPI/HDR displays, fallback Mica and arbitrary wallpapers. Exact next step stays **Stage 4 only after authorization**; the Later ideas remain deferred.

- Product: complete System/Dark/Light themes, whole-window glass, settings/accent, native Windows file/Favorites artwork with bundled fallbacks, List/Icons/Gallery, one selected-item preview with details, bounded worker previews, single instance and indexed folder expansion. No tray, hotkey or app autostart.
- Latest toolbar mockup: 52 px header, sidebar/history left, wide flexible rounded-rectangle search, fine separators and glass groups for views/actions/native caption targets. No location title, history dropdown or Ctrl+K badge. Regular 14 px symbols, 16 px pane/search; zero button padding avoids squeezed artwork and a 1 px focus line avoids neighboring controls. Hidden-files artwork now uses Kit's inherited SVG tint; Info highlights only with a visible inspector. File/Favorites icons remain 16 px, with one shared yellow folder. Settings retain compact headings, square swatches, centered labels and readable Light Graphite text. Sidebar movement takes 160 ms, including reversal, and requests frames only during the transition.
- Correctness: preserved BGRA image fixes, persistent visible thumbnail requests, worker-side file fingerprints and retained recent rows that remove deleted files without jumping. Empty folder views use all direct indexed children; searches/global recents retain the specified top-200 limit. The folder regression checks 241 direct children and scoped/global nested package.json queries. Windows-denied locations remain visibly reported.
- Current checks: `cargo test --workspace --all-features --locked --offline`: 70 passed, 1 existing benchmark ignored; `cargo clippy --workspace --all-targets --all-features --locked --offline -- -D warnings`: exit 0; formatting and debug workspace build pass. Exact commands/timings: `docs/benchmarks/stage3-targeted-validation.json`. A retained-hit test initially raced a new-file event and now waits for the complete expected live state.
- GUI baseline: 23/23 passed in `stage3-taste-ui-final.json`, covering expansion/sort/hidden, real PNG/SVG/video/text previews, all views/themes, glass, accents, saved locations/exclusions/default view, Escape, single instance and complete exit. Debug private working set 52.43 MiB, private commit 101.34 MiB, close 119.555 ms / exit 0. Subsequent appearance refinements use a separate small targeted check; earlier failed test-harness reports remain as history. No whole-drive tests were repeated.
- Glass: verified visually over a temporary colored window. Kit's theme reconciliation had restored an extra background tint; an extra DWM frame also painted duplicate caption glyphs. The root is now transparent, with one 80% neutral tint and GPUI's native Acrylic/Mica fallback. Sidebar and toolbar add no opaque fill. No framework/vendor update was needed.
- Benchmark scope: prior Stage 3 10k backend medians 0.0027-0.0048 ms and UI medians 6.603-6.924 ms, p95 10.035-10.566 ms, precede this visual redesign. Accepted Stage 2 whole-drive numbers below are unchanged. No fsearch parity claim.
- Assumptions/limits: Windows 11 on this laptop, native Segoe Fluent artwork, default Windows folder associations. Files named folder.png retain their real image previews. Last opened records opens through Seek. Other DPI/HDR displays, fallback Mica appearance and arbitrary wallpapers still need visual checks; sustained idle/heavy churn/resource tuning remain Stage 4. Test windows use owned fixtures; normal launch uses the real LOCALAPPDATA/Seek index.
- Final toolbar checks: `stage3-toolbar-ui.json` has 7/7 passes, including separate non-overlapping bounds for all 14 header controls, Info open/close, sidebar motion, both themes and process exit. `stage3-toolbar-validation.json` records successful formatting, all-target/all-feature Clippy and debug build. The original blank SVG/focus halo failures were visible and fixed; the unrelated drive checks were retained.
- Final test correction: the first release check read Windows accessibility immediately after a view change and missed the already-selected item. A bounded 3 s wait for the expected visible region fixes that harness race; the same binary passes 7/7. The failed report is retained as `stage3-toolbar-release-ui-first-failure.json`.
- Toolbar release baseline before the final Settings follow-up: workspace all-feature build exit 0 / 436.907 s; 7/7 GUI checks pass. Small-fixture private working set 40.62 MiB, private commit 85.95 MiB; close 125.887 ms / exit 0. Current final binary evidence: `docs/benchmarks/stage3-summary.json`.
- Exact next step: Stage 4 hardening only after authorization. Stage 3 is complete; indexing-speed work and the Later ideas remain deferred. Build/run with the pinned MSVC toolchain in README.md and docs/STAGE3.md.

## Later ideas



Deferred: faster first indexing using a one-pass NTFS MFT read ([wsearch](https://github.com/al3rez/wsearch)); grep content search; optional search by meaning ([DigUp](https://github.com/ARahim3/DigUp)). None is being implemented in Stage 3.



## Accepted Stage 2 baseline



# Stage 2 — complete



Updated 2026-10-12. The user explicitly authorized Stage 2 after the real-world Stage 1 audit. **Stage 2 is complete and accepted by the recorded real-driver checks. No Stage 3 code was started.**



Stage 2 final acceptance — 2026-10-12, 16:17 UTC:



- **Complete: all 24 administrator/ordinary-user checks pass**, `docs/benchmarks/stage2-final-results.json`; short report `stage2-summary.json`. The agent ran the final prepared controller through normal Windows RunAs after the user's explicit request. Actual ordinary worker SID/non-elevated token, protected installed binary/hash and service ACL checks pass. Installed helper SHA256 **`151539B785DBAF56E80AC8F403BCF383988A48762FE8A13B47F21652700D9560`**. Only Seek's service/artifacts changed; real folders were read-only and Windows' journal was never modified.

- Correctness: **3,015 fixture entries, zero missing/extra/stale/denied** at first index, live updates and closed changes/relaunch; **10/10** top-five accuracy, closed catch-up **26.770 ms**. Real **Downloads 500 / Documents 565 / Desktop 24** each have **zero missing/extra/stale/denied**, individually and inside the whole-C: index. Denied-directory check exposes exactly one expected denied folder and no elevated children. Closed create/rename/move/delete plus a deleted nested parent replay in **19.680 ms**, with **0 resets/reconciliations/fallbacks**. Invalidated Downloads history **21.632 ms** and invalidated changed-fixture history **18.425 ms** recover correctly. Provider-backed roots intentionally use listing plus USN.

- First whole-C: **helper 21,932.127 ms / walker 22,767.712 ms**, about **3.7% faster** in this sequential live-drive sample. Helper **3,016,024 entries / 278,956 directories / 174,237,516-byte index / 189 skips**; walker **3,016,013 entries / 185 skips**. CLI totals **24,133.142 / 24,976.565 ms**, shutdown **12.381 / 13.608 ms**. Actual fast baseline **NTFS + USN**, **0 resets/reconciliations/fallbacks**. Metadata **8,578.463 ms**, ordinary namespace/access checks **9,277.049 ms**. Drive activity explains differing snapshot counts; the entire C: namespace was not independently ground-truth verified.

- Saved whole-C: app **277.791 ms cached presentation** (catching-up warning active), **2,805.871 ms correct presentation**, **2,731.649 ms USN catch-up**; **99 journal batches / 100 directories / 0 resets, reconciliations or NTFS fallbacks**. Private working set **47,923,200 bytes / 45.70 MiB**, private commit **89.31 MiB**, plus the memory-mapped index. Closing exits fully in **110.675 ms / exit 0**. Helper after disconnect: **0 ms CPU / 3,146.962 ms**, private working set **8.97 MiB**, private commit **14.28 MiB**. This short helper observation is not sustained foreground-app idle certification.

- Fixes: logical kernel records remain separate from raw-sector fixups; supported stream versions retain all hard links/resident lengths; cloud/provider directories are traversed while redirecting links are recorded and skipped. **Four** low-priority metadata workers use independent native handles and disjoint filters covering every **48-bit record ordinal**, including later MFT growth; batches/channel storage are bounded and workers joined. **Sixteen** impersonated directory workers authorize parent access before descendants, in batches <=256. Historical journal records recover deleted-parent chains across native read batches with an **8 MiB payload budget**; incomplete/corrupt/overflow history still requests one explicit reconciliation. Closing is marked before cancellation, avoiding a spurious walker rescan during quit. No polling/background app/tray/hotkey was added.

- Verification commands: `cargo test --workspace --all-features --locked` **56 passed / 1 unchanged explicit pipe benchmark ignored**, fmt/test combined **63.674 s**; `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` **exit 0 / 11.375 s**; `cargo check --workspace --locked` **exit 0 / 3.608 s**; `cargo fmt --all -- --check` **exit 0 / 4.702 s**. Optimized `cargo build --workspace --release --all-features --locked` **exit 0 / 328.785 s**; final script `-CheckOnly` passes with these executables. Final controller **16:15:50–16:17:45 UTC**, exit 0. Reports: `stage2-ranged-scan-tests.json`, `stage2-ranged-scan-checks.json`, `stage2-ranged-scan-release-build.json`, `stage2-agent-admin-launch.json`, final results. Unchanged UI/query benchmark suites were retained, not rerun.

- Failed run and diagnosis retained in `stage2-before-ranged-scan.json`: old first helper **35.708 s / walker 25.150 s**, metadata **21.121 s**. Native filtered-range probes pass; four-range enumeration-only **7.325 s**; growth-covering probe **9.086 s** during another live run. Old CLI quit diagnostic **40.487 s total** despite a **2.506 s** catch-up; fixed quit regression **16.226 ms shutdown / 4.942 s total** on the saved drive during compilation. These diagnostics exclude parts of the full index/UI workload and are not parity benchmarks.

- Assumptions/limits: Windows 11 Home on this laptop; same-account Explorer desktop; redirected Known Folders resolved by Windows; OS caches not flushed; metadata can contact OneDrive; files change between sequential drive runs. No fsearch parity or published 98% typo-rate claim. Whole-drive cold-cache/distribution, sustained foreground idle, peak memory/heavy churn and additional devices remain Stage 4 work. Prior sidebar/Pictures/Videos/global-vs-scoped package.json fixes and checks remain valid. Tested GPUI Kit **0.7.1** remains in Stage 2; **0.7.3** is verified available.

- Build/run: Rust **1.96.0 x86_64-pc-windows-msvc**, VS 2022 Desktop C++ tools and Windows SDK **10.0.26100.0**; `cargo build --workspace --release --all-features --locked`; run `target/x86_64-pc-windows-msvc/release/seek.exe` as an ordinary user. The installed helper is optional; `seek.exe --walker` forces fallback. Reproducible administrator test: `powershell -NoProfile -ExecutionPolicy Bypass -File scripts/finish-stage2.ps1`; add `-Uninstall` to remove only Seek's helper.

- **Exact next step: Stage 3**, complete System/Dark/Light themes, glass/accent/settings, single instance, bundled icons/shell thumbnails, Icons/Gallery and Space preview. **No Stage 3 code was started in this stage.**

Earlier continuation, after the reported restart:



- The user successfully updated the protected helper at **2026-10-11 17:58:43 UTC**: install, binary hash/ownership, LocalSystem identity and service ACL checks all pass. The controller then failed to launch its ordinary worker (`ERROR_DIRECTORY`). Prepared a `CreateProcessAsUserW` linked-token launcher, validated executable/directory inputs, added API-specific errors and an inherited-directory secondary-logon fallback for missing quota privilege. **The new elevated-to-ordinary launch still needs the administrator retry.** The agent launched the worker directly with its existing ordinary token to continue testing without elevation.

- That real-driver follow-up found zero discrepancies for separately indexed **Downloads 500 / Documents 565 / Desktop 24**; first/live/closed fixture verification and **10/10** accuracy also pass. **Performance fails:** the bulk decoder rejected a stream version (the old diagnostic did not include its value), and deleted/reused records repeatedly forced conservative scans. Old whole-C: first-index catch-up: **walker 24,270.502 ms; helper fallback 695,424.629 ms / 24 conservative reconciliations**. These are failures, not fast-path/parity results. Added bounded version-1/version-2 stream decoding, detailed unsupported-version diagnostics, and a distinct vanished-record result so already retained journal parents are refreshed without treating ordinary file deletion/reuse as corruption; incomplete extension records still force reconciliation. Unknown parent resolution still reconciles conservatively.

- The broader-index check also exposed **565 Documents / 24 Desktop entries missing** from the whole-drive walker fallback: cloud reparse folders were being treated as links. Walker listing and live metadata now use the reparse tag to retain cloud/provider files and descend provider directories while skipping redirecting links. The independent verifier uses Windows `std::fs::FileType` to distinguish links. `SKIDX003` forces old incomplete snapshots to rebuild once. Corrected debug `seek-cli verify --source walker --root C:\ --verify-root <Known Folder> --data <owned follow-up index>` now reports **0 missing / extra / stale / denied** for **565 / 24 / 500** entries. Whole-drive index ends at **2,969,142 entries / 172,853,120 bytes**, **214 skipped locations**; the entire drive was not independently verified. Debug catch-up **77,659.958 / 91,153.374 / 85,072.145 ms** under concurrent compilation is correctness evidence, not release-speed certification.

- Current checks: `cargo test --workspace --all-features --locked` **33 passed / 7.667 s** after the final vanished-directory change; all-target/all-feature Clippy `-D warnings` **exit 0 / 4.916 s**, default workspace build **exit 0 / 6.976 s**, formatting **exit 0 / 5.689 s**. Final release workspace build **exit 0 / 229.413 s** (earlier rebuild **236.709 s**); a Clippy test-module-placement error was corrected before the final checks. Script compilation/path/read-only invocation **exit 0 / 1.456 s**. Release Downloads GUI: correct first presentation **255.194 ms**, walker catch-up **15.861 ms**, private working set **25.81 MiB**, close **117.058 ms / exit 0**; this single fresh/small-scope run is not a whole-C: or cache-cold result. Full evidence: `docs/benchmarks/stage2-driver-followup.json`, also collected in `stage2-final-results.json`.

- **Remaining block / exact next step:** the protected service still contains the binary from the user's first successful update; the rebuilt helper cannot be installed by the ordinary agent. Run the same `scripts/finish-stage2.ps1` once in the already elevated PowerShell window. The script now skips the expensive drive tests if an earlier helper/correctness check fails. Read the results, fix any remaining failure, repeat the administrative update only as needed, then measure whole-C: app catch-up and commit after all acceptance checks pass. The native launcher, new real bulk layout and fast baseline remain unverified until that run. No Stage 3 work is authorized.



Earlier continuation evidence:



- A second user run at **2026-10-11 17:57:02 UTC** also stopped before any helper update because its token was not elevated. The script and administrator guide now provide an explicit user-run `Start-Process ... -Verb RunAs` command to show the Windows UAC prompt; the agent does not invoke it. After consent, the controller still runs verifier/app checks with the ordinary user token. Updated script `-CheckOnly`: **exit 0 / 1.036 s**, compilation/path/read-only CLI checks pass; `git diff --check` exits **0**. Exact next step: user runs this launch command, accepts Windows' prompt, then returns the saved results; Stage 2 remains pending.

- The user's final script run at **2026-10-11 17:52:07 UTC** stopped at the elevation check: **PowerShell was not running as administrator**. `stage2-final-results.json` has no worker results; no helper update or real-driver checks started. The script now prints the exact failure and instructions to open administrator PowerShell; `powershell -NoProfile -ExecutionPolicy Bypass -File scripts/finish-stage2.ps1 -CheckOnly` passes, **exit 0 / 0.941 s**. Stage 2 still needs the same script run in that terminal; no Rust code or prior verified results changed.

- Corrected the FSCTL boundary: logical kernel records never receive raw-sector USA replacements, including coincidentally matching trailers. Cross-sector filename/truncation regressions pass. Initial file metadata now uses bounded `FSCTL_QUERY_FILE_LAYOUT` batches retaining all hard-link names and resident/empty stream sizes; protocol v2 sends compact listing frames. Directory access checks stay under the ordinary client token. USN batches coalesce repeated files, scope paths, retain hard-link parents and resolve inaccessible parent metadata without requiring an unrelated full-root rescan. **These driver changes still require the real elevated service retest.**

- Added explicit replay/reset/reconciliation/fallback counters, persisted scoped skipped-location reports with the helper checkpoint, and update rollback that restores the previous helper binary/service or removes a failed new installation. No Windows journal mutation was added. The installed service remains the older binary; no administrator action was performed by the agent.

- `cargo test --workspace --all-features --locked`: **31 passed**, **39.343 s**; Clippy all targets/features with `-D warnings`, formatting and default workspace build all exit 0 (**7.602 / 3.779 / 6.503 s**). Final measurement change also passes Clippy/formatting (**2.968 / 3.952 s**). `stage2-final-validation.json`. The new `verify --verify-root` checks a subfolder against a broader saved index: **1,001 entries, zero missing/extra/stale/denied**, walker catch-up **45.917 ms**, independent verification **241.075 ms**. An initial harness command named a nonexistent fixture subfolder; it was corrected to the actual `batch-0000` directory, without changing files.

- Optimized workspace build passes: **250.406 s** final instrumentation rebuild (earlier complete build **311.311 s**). Whole-drive measurement now uses a user-restricted pipe, so writing its own trace cannot trigger indexing/redraw feedback. Actual debug/release 10k GUI pipe checks exited **0** on window close: release first correct presentation **200.725 ms**, walker catch-up **17.306 ms**, private working set **28.21 MiB**, close **42.901 ms**. These are single saved/warm fixture samples, **not whole-C: or true cold-launch numbers**. Final `cargo test -p seek --all-features --locked`: **2 passed** after the instrumentation change. `stage2-final-release-build.json`, `stage2-{release-}metrics-pipe-check.json`.

- One administrator command is prepared: `powershell -NoProfile -ExecutionPolicy Bypass -File %USERPROFILE%\Seek\scripts\finish-stage2.ps1`. Script syntax, native compilation, ordinary read-only CLI invocation and actual GUI pipe measurement passed. It updates only this user's Seek helper, checks its hash/permissions, launches all verifier/app checks with the ordinary UAC-filtered token, tests real Known Folders and invalidated owned checkpoints (including closed changes), compares separate first C: indexes, verifies real folders inside the whole-drive index, then measures app catch-up only if correctness/speed pass. Final output: **`docs/benchmarks/stage2-final-results.json`**. `-Uninstall` removes only the helper and preserves previous validation results. No real-user files, accounts, unrelated services, Windows journal or global policies are modified.

- Assumptions/remaining blocker: administrator PowerShell must use the same Windows account as this checkout; OneDrive metadata listing can contact its provider; sequential live-drive timings use existing OS caches. The token launch, actual bulk NTFS result format, installation rollback and real-drive performance remain administrative acceptance checks. **Stage 2 is still open; final commit waits for that acceptance. Exact next step: user runs the single script and says `done`; read its results locally, fix any failed gate, repeat the same script as needed, and commit only when Stage 2 passes. Do not start Stage 3 without the user's instruction.**



- Real audit: Downloads **493 files / 499 entries**, redirected Documents **428 / 565**, redirected Desktop **22 / 24**; **0 missing / extra / stale / denied** each. Independent PowerShell listings agree. An inherited embedded-word typo bug caused **19/20** initial searches; the fix passes **20/20**, single-file targets all top 4, no nonexistent/unlisted result. Reports and reproduction: `docs/benchmarks/real-world-{before-fix,check}.json`, `scripts/check-real-data.ps1`. No user-file contents were read or user files changed by this audit.

- Stage 2 code: Rust NTFS metadata source, all hard-link names/extension records, USN replay/blocking reads, client-impersonated ordinary access checks, OneDrive provider-listing fallback, bounded user/SYSTEM-only local pipe, trusted SCM server PID check, optional protected LocalSystem service and opt-in settings actions. Cursor/index snapshots share one atomic file footer; chunked deltas advance the cursor only after the final chunk. Cached results keep native watcher updates during initial helper streaming. No background timer or app elevation/autostart was added.

- Verified: `cargo test --workspace --all-features --locked` **26 passed**; all-target/all-feature Clippy with `-D warnings`, formatting check and default workspace build exit 0. Final debug check batch **66.025 s**; `stage2-validation.json`. `scripts/check-stage2.ps1` passed the original helper-absent/unelevated smoke checks (bounded real pipe, Ping, explicit access-denied fallback, reconnect, rejection of an unregistered server). `self-test --source walker`: **3,015 entries**, all three phases zero discrepancies, **10/10** accuracy; initial **59.966 ms**, live verification **179.443 ms**, closed catch-up **13.010 ms**. Synthetic journal tests prove cursor reset/checkpoint/chunk integration, not real Windows USN lifecycle acceptance.

- After the new engine integration, real-folder verifier checks again report zero discrepancies: **499 / 565 / 24 entries**; walker catch-up **7.410 / 154.330 / 2.186 ms**. The 20-search audit was not unnecessarily repeated. GUI helper sheet opening, Escape, Close and process shutdown passed; **86.265 ms**, exit **0**, private working set **30.86 MiB** on Downloads. The initial automation expected an absent-helper label and timed out after the service appeared; the corrected check handles the actual installed state and invokes no install/uninstall action. `scripts/check-stage2-ui.ps1`, `stage2-ui.json`.

- **Actual blocker:** a trusted installed helper appeared during interactive GUI testing; its Program Files binary is owned by Administrators with user read/execute only, and its service is LocalSystem/automatic. The agent did not install/update it. Its older binary reports `torn NTFS file record`. Helper-connected Downloads verification still gives **499 entries, 0 missing / extra / stale**, but source **walker + usn**, **24,564.187 ms**; this fails the fast-path goal. A checked-kernel/raw-record parsing correction is prepared and unit-tested, but **not verified against the real driver after a service update**. Use the verified `--walker` app option until retested. Do not call Stage 2 complete or claim helper/fsearch speed parity.

- Release build passed: `cargo build --workspace --release --all-features --locked`, **305.748 s**, followed by a CLI benchmark-source flag rebuild (**37.284 s**). Release helper-sheet checks also pass: **80.686 ms** close, exit **0**, **28.79 MiB** private working set. The checkpoint diagnostic changed a test-only saved journal ID **456 → 457** without touching Windows' journal. Helper-connected closed reconciliation of a 3-entry fixture gave zero discrepancies (**22,837.649 ms**), but record fallback and a subsequently corrected mock-root harness serialization mask specific reset/delta proof; that acceptance remains pending. Reports: `stage2-release-{build,ui}.json`, `stage2-checkpoint-diagnostic.json`, `stage2-actual-helper-reset.json`.

- Current release backend check: `seek-cli bench --files 10000 --runs 30 --source walker`, 3 warmups and 30 measured runs per exact query, expected result checked each time. Medians **0.0063–0.00725 ms**, worst p95 **0.0321 ms**, index **763,008 bytes / 10,018 entries** (**76.16 B/entry**). This is an exact-name workload only; no new 1M/GUI/broad-query or fsearch-parity claim. Final CLI tests and Clippy pass after adding matching walker-source forwarding to the UI benchmark harness. `stage2-backend-10000.json`.

- Memory/idle: installed earlier helper with no connected app **1.98 MiB private working set**, **3.03 MiB private commit**, **0 ms CPU / 3,143.259 ms** (short sample, not activity/tailing certification). The earlier temporary unelevated console used **0.88 MiB private working set**, 0 ms over a configured ~3 s sample; its original elapsed field included counter collection. Stage 1 large-index memory/latency numbers below are retained and predate the matcher and Stage 2 changes; no new 1M/UI latency claim.

- Known limits: one helper client at a time (close app before helper CLI tests); conservative full-root reconciliation for hard-link namespace changes/unresolvable parents; unsupported NTFS record/journal formats explicitly fall back; raw scanner speed, real closed USN replay, installation failure rollback, stop during large scans, remote/second-account ACL rejection and uninstall remain administrator checks. Complete themes/previews/settings belong to Stage 3; cold-cache/quiet-foreground/stress certification stays in Stage 4.

- Assumptions: use actual Windows Known Folders, including OneDrive redirection; list provider names without intentionally hydrating contents; keep NTFS elevation confined to the already opt-in helper; Windows retains journal history while the app is closed. The driver-record correction is a hypothesis until a live retest. GPUI Kit stays at the verified current stable **0.7.1**, and the recent crossplatform engine update is already pinned.

- **Exact next step:** update the already installed protected helper from an administrator terminal, then run helper `self-test`, the three real-folder verifiers, checkpoint-invalidated history recovery and separate first-drive walker/helper timings. Exact commands: `docs/STAGE2-ADMIN-TESTS.md`. Stop/report if MFT still falls back or correctness fails. Only after this Stage 2 gate passes, proceed to Stage 3 when authorized.



The earlier Stage 1 measurements and limitations follow as historical evidence.



## Stage 1 foundation closeout



At the Stage 1 closeout, the walker engine, CLI, GPUI spike and application shell were delivered. Focused debug/release GUI lifecycle checks passed, including chips, sorting, navigation, live/closed changes, geometry and real Open/Reveal. **Product acceptance remains conditional:** true OS-cache-cold launch, sustained quiet foreground idle, broad/fuzzy GUI latency and device/stress cases remain hardening work. These are not certified by warm exact-query measurements.



Saved-index launches already measured from Windows process creation have medians **255.539 ms (10k)** and **243.910 ms (1M)**; one 1M sample took **315.502 ms**. Exact-query UI medians are **6.1–6.9 ms**, private working set **26.3 / 38.5 MiB**. Those passing benchmark runs were retained rather than repeated. The recently updated engine head matches our pin; GPUI Kit 0.7.1 remains the verified latest stable version.



## Real-world gate before Stage 2 (2026-10-11)



- Actual Windows Known Folders: Downloads **493 files / 499 entries**, redirected OneDrive Documents **428 / 565**, redirected Desktop **22 / 24**. `seek-cli verify` reports **0 missing, 0 extra, 0 stale, 0 denied** for each. Counts also agree with an independent `Get-ChildItem -Force -Recurse` listing. No user files were changed, opened or downloaded.

- Initial audit: **19/20 searches passed**. `lambanual` missed an existing PDF because inherited typo recovery covered only filename starts and spaces. Extended existing masks/matcher to underscore/hyphen word boundaries; saved-index magic `SKIDX002` triggers one rebuild. Regression covers base, live overlay and restart. Fixed audit: **20/20**, every expected single-file target in the **top 4**, no nonexistent or unlisted result. Complete extension checks: **247 Downloads PDFs, 19 Documents PDFs, 54 Documents DOCX**; **28** downloads modified within 7 days; deep scope and newest-first checks pass. Audit limit 2,000 avoids truncation and does not measure UI latency.

- Command: `scripts/check-real-data.ps1` (debug CLI); per-folder verification plus before/after search reports in `docs/benchmarks/real-*.json`. `cargo test --workspace --all-features --locked`: **13 passed**; Clippy all targets/features with warnings denied and default workspace build pass; formatting applied. Existing memory/latency figures above predate this matcher fix and were not repeated.

- User explicitly authorized Stage 2 after this gate. **Next: implement Rust NTFS records/source and journal replay, then the opt-in elevated service/user-restricted pipe.** Administrative installation and volume checks will require explicit opt-in/manual testing; the app remains unelevated.



## Done



- Rust workspace with engine, CLI and GPUI shell. MSVC toolchain pinned to 1.96.0. Release profile uses opt-level 3, fat LTO, one codegen unit, abort and stripping; `release-small` exists for the unmeasured opt-level `s` comparison.

- Read all requested references. Engine pinned to fsearch-crossplatform `16eaebd89b8db161e4a5cac961f746565b878686`, with MIT LICENSE and attribution. Preserve the flat subtree-contiguous mmap index, name interning/masks, portable parser/matcher/ranking and live overlay. Content search and the Unix socket are absent from compiled core paths.

- Current **GPUI Kit 0.7.1**, vendored at `1ccf011620da24d633f7cd6d3c11ca2e97798dcf`. Latest stable version checked on crates.io. Official Zed GPUI pinned to `279fe070bb389b79652e52065b2f001edcc0b11b` (2026-10-04), the revision named by the kit's GPUI snapshot metadata. Dependency aliases, macro crate-name lookup and optional static caret are documented in `vendor/gpui-component/SEEK-PATCHES.md`.

- Spike: native window, Acrylic, kit input, 100,000 lazy rows, one image; explicit Mica mode. Acrylic visible after removing the root's opaque background. Native caption buttons, image and scrolled rows were observed.

- WalkerSource: FindFirstFileExW Basic/LARGE_FETCH, bounded crawl threads, no hidden/system/noise-folder exclusions, no reparse traversal. Access/listing failures counted. Reversible WTF-8 path representation, extended Windows paths and ordinal case-insensitive index lookup/order.

- Blocking overlapped ReadDirectoryChangesW with cancellation and overflow rescan. First read armed before the baseline can be published; this fixes the startup watch registration race. Directory-attribute overlay fixes stale folder timestamps without replacing their contiguous base subtree.

- Saved index and per-root metadata; memory-mapped load. Cached results published with a catching-up status before launch reconciliation. Helper-absent reconciliation relists indexed directories once per launch for correctness. Persisted bounded open counts and Downloads/Desktop/Documents priors; exact/prefix ranking protection and newest-first empty-query top 200 are tested. Cooperative query generation cancellation is implemented at phases/chunks.

- CLI `index`, `search`, `bench`, `verify`, `self-test`. Independent verifier reports missing, extra, stale and denied locations. Real NTFS benchmark datasets generated.

- Application shell: sidebar, toolbar/input, sortable/resizable virtual table, generic file/folder icons, status and selection breadcrumb. Worker search and filesystem open/reveal code; bounded 200-row results and one-update channel. Real GUI checks now cover filter creation/removal, Name sort, arrows and Escape. Matched filename characters are highlighted using worker-computed, UTF-8-safe ranges; exclusions and filter values are not highlighted. Window position/size are atomically stored under `settings.json` on a background quit task and restored on launch; caption visibility is checked against connected displays.

- `vendor/gpui-windows` contains only the Windows crate from the same official GPUI pin, with Apache license and reproducible provenance. Per-window demand flags and a blocking condition variable park the VSync thread. GPUI's existing frame-waker/schedule hooks wake it; draw retries, resize failures and active touchpad gestures re-arm it. No upstream messages/PRs were sent. Keep this patch under review until an official revision includes an equivalent fix.

- Backdrop APIs now return success/failure and select Acrylic → Mica → opaque. API acceptance of Acrylic was captured in a diagnostic log; unsupported-system fallback remains untested. Native caption controls remain.

- Last-window close quits without joining a crawler/checkpoint on the UI thread. Atomic saved snapshots survive interrupted work; launch reconciliation recovers changes. The one-item result channel replaces an obsolete update instead of blocking the search worker on a closed UI. Favorites use Windows Known Folder paths. Initial input focus, preserved-cursor chip decoration, quoted filter spans and bounded recency boosts are implemented.

- Optional `benchmarks` feature and `--metrics` add native WM_CHAR receipt and successful DirectX Present-return instrumentation, with file writes on a worker. `seek-cli bench --ui-executable PATH` runs the dedicated Windows harness. Normal builds omit measurement observers. Harness fixes: unique trace per launch, shared read access, explicit foreground readiness and queued backspace messages.



## Focused closeout checks



- Catch-up keeps its warning while applying notifications already queued during the crawl/relist, including overflow recovery, before blocking on the channel. This corrects the premature up-to-date status; heavy/in-flight watcher races remain a Stage 4 stress target.

- The focused GUI check found a real Escape conflict after sorting: the kit's table Cancel binding intercepted it. Seek now captures input/table Escape actions so the first press clears search and the next clears selection. Selection follows its file across result updates; a cleared selection no longer displays an unrelated first-row breadcrumb. Stale-open messages survive subsequent filesystem updates.

- `scripts/check-stage1.ps1`: **8 focused checks passed** using real GUI processes, UI Automation readback/Invoke, native keys, a painted-result trace and independent `seek-cli verify`. **6 ground-truth/indexed entries, 0 missing / extra / stale**, initial, live and GUI-process relaunch after closed changes. GUI walker catch-up **2.251 ms** on this small fixture. Both closes exited with code 0 in **55.578 / 56.233 ms**. A moved/resized **1200×780 physical-pixel** window restored exactly. See `stage1-focused-check.json`; the established 3,015-entry lifecycle/Unicode/long-path fixture also passes in the workspace tests.

- `cargo test --workspace --all-features --locked`: **12 passed**. Clippy all targets/features with warnings denied, formatting and default workspace build all exit 0; combined command time **29.207 s**. See `stage1-closeout-validation.json`. PowerShell harness syntax checks passed. The two standalone frame-scheduler tests were already verified and were not repeated.

- Final instrumented release build passed in **279.147 s**. `scripts/check-stage1.ps1 -Executable target/x86_64-pc-windows-msvc/release/seek.exe -ShellActions -Output docs/benchmarks/stage1-release-shell-check.json`: all 8 GUI lifecycle checks plus real Open/Reveal passed. Explorer location and selected-file path were read back; only the unique fixture's Explorer windows were closed. Independent verification again reports **6 entries, 0 missing / extra / stale** in all phases; catch-up **1.192 ms**, closes **56.452 / 63.821 ms**, exit 0. The harness now waits for asynchronous UI Automation provider initialization/readback. Saved benchmark numbers above precede these final small UI changes; no parity or new latency claim is inferred.

- One final `scripts/measure-startup.ps1 -Files 10000 -Runs 1 -IdleSeconds 20 -Output docs/benchmarks/stage1-closeout-idle.json`: **0 ms process CPU / 20.234 s**, **2 presentations**, **6 native activity/focus messages**, but foreground lost on **68/78** checks. It is explicitly **not a quiet foreground certificate**, and was not retried. Private working set **27.00 MiB**; this single saved/warm launch presented 200 up-to-date rows in **201.586 ms** from kernel process creation; close **48.877 ms**, exit 0. No benchmark distribution is inferred from that one run.

- Visually observed the sidebar, native caption buttons, two `invoice ext:txt` results, a removable chip and bold matching filename characters. No Finder screenshots were attached, so visual fidelity to that target remains unverified.



## Latest continuation measurements and verification



- Engine head rechecked via GitHub API: **`16eaebd89b8db161e4a5cac961f746565b878686`**, latest merge **2026-10-10 21:49:20 UTC**. This recent speed-lead update is already vendored; no upgrade is needed. `upstream-check.json` records the check. GPUI Kit **0.7.1** remains the latest stable crates.io version in a fresh check; see `kit-version-check.json`.

- Startup phase and optional frame tracing identify initialization cost. DirectWrite now uses its font cache on startup, retaining refresh after a failed family lookup. Both shell and spike use the installed `Segoe UI Variable Text` family; the old `Segoe UI Variable` name caused a failed lookup and refresh. `scripts/measure-startup.ps1` records cached and reconciled presentations separately, foreground checks and redraw counts. Both launch harnesses record Windows process creation, excluding PowerShell setup while retaining its separate interval.

- Diagnostic `cargo build -p seek --release --features benchmarks`: **4m 53s**; optimized rebuild after the two fixes: **4m 42s**. Both completed and ran. Phase records: initial DirectWrite setup about **108–121 ms → 5–7 ms**; shell construction → window-open return about **139–153 ms → 7–15 ms**. These are bounded measurements, not every Windows configuration.

- `scripts/measure-startup.ps1 -Runs 5 -IdleSeconds 0 -Files 10000 -Output docs/benchmarks/startup-kernel-clock-10000.json`, repeated with `-Files 1000000`: **10k cached presentation 240.755–260.562 ms, median 255.539 ms**; **1M 233.455–315.502 ms, median 243.910 ms**. Each contains 200 cached rows; the million-file rows carry `catching_up=true`. OS file cache was not flushed. Earlier `startup-before/after` JSONs start before `Start-Process`, so include launch-harness overhead; retain them with explicit clock notes. First rebuilt executable sample took 830.102 ms on that older clock, 207.820 ms after process entry; the intervening time is not attributed to a specific cause.

- Final release UI benchmark, three warmups and **30 samples per query** on each dataset: **10k medians 6.132–6.918 ms, worst p95 11.169 ms; 1M medians 6.058–6.582 ms, worst p95 10.129 ms**. Expected one-row exact result checked each time. Private working set after queries **26.32 / 38.51 MiB**. Kernel-clock first cached presentations **240.632 / 252.073 ms**; walker catch-up **16.260 / 1,129.162 ms**. Close **110.452 / 86.485 ms**, exit 0. Raw `ui-font-fix-10000.json` and `ui-font-fix-1000000.json`; no fsearch parity or broad/fuzzy latency claim.

- `target/x86_64-pc-windows-msvc/debug/seek-cli.exe self-test --output docs/benchmarks/correctness-font-fix.json`: **3,015 indexed / ground-truth entries**, **0 missing / extra / stale** at first index, live changes and stopped-engine/relaunch. Live verification **148.828 ms**, walker relaunch catch-up **13.656 ms**; **10/10 top-five accuracy**. This tests the engine lifecycle rather than the complete GUI process lifecycle.

- `cargo build --workspace --locked` **22.39s**, `cargo test --workspace --all-features --locked` **18.90s** (**10 passed**), `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` **11.62s**, formatting **4.49s**: all exit 0. Standalone scheduler **2 passed**. `font-fix-validation.json` preserves child exits/timings and notes an empty-log parsing issue corrected in the ignored reporting wrapper; no child check failed.

- Default-feature debug shell ran on the saved 10k index, backdrop log reports requested/actual **Blurred**, private working set **27.17 MiB**, WM_CLOSE exited with code 0 in **74.656 ms**. A **1.003-second** foreground-endpoint sample recorded **0 ms CPU**; this short sample is not sustained-idle certification. Latest release spike had input/row nodes in its accessibility tree and Acrylic API success; its screenshot was occluded, so glass/image appearance was not reverified. Its unattributed one-second sample used **437.5 ms CPU**; keep this conflicting result visible. Close **65.876 ms**, exit 0.

- Twenty-second idle trials did not certify quiet foreground idle: 10k lost foreground for all checks; one 1M trial stayed foreground but the trace shows typing and generation changes (3,312.5 ms CPU / 359 frames). Additional 10k/1M attempts lost foreground, with one redraw and **31.25 / 15.625 ms CPU**. Raw reports mark invalid samples. Stopped GUI automation when capture showed occlusion; no browser actions were taken and no screenshot was committed.



## Previous continuation measurements (before the font fix)



- Release build with measurement feature: `cargo build -p seek --release --features benchmarks`, **5m 16s**, passed; the shell ran on 10k and 1M saved indexes. Raw `ui-10000-release.json` and `ui-1000000-release.json`: three queries, three warmups and 30 samples each, expected one-row result checked every time. Native final-text-character receipt → result application → DirectX Present return, excluding physical DWM scanout and harness waiting.

- **10k UI:** medians **6.296 / 5.901 / 4.907 ms**, p95 **8.332 / 10.789 / 10.738 ms**. Launch to up-to-date presentation **474.070 ms**; process-entry interval **394.184 ms**. Private working set **28.07 MiB**, total **63.56 MiB**. Close **77.088 ms**, exit code 0.

- **1M UI:** medians **5.872 / 6.192 / 5.779 ms**, p95 **9.730 / 9.603 / 9.389 ms**. First cached 200 results presented **572.675 ms after launch**, with catching-up visible in the recorded status; up-to-date presentation **1,232.471 ms**; walker catch-up **1,120.736 ms**. Cached-first timing was extracted from the original trace of the same run. Private working set **40.87 MiB**, total **142.58 MiB**, private commit **85.07 MiB**; mmap file **72.65 MiB**. Memory captured after the queries, not a separate long idle sample. Close **80.510 ms**, exit code 0. Launch samples did not flush the OS cache; true cold launch is unmeasured and cannot be claimed under 300 ms.

- Current opt-level 3 backend medians **0.00390–0.00410 ms** (10k), **0.00290–0.00305 ms** (1M); worst p95 **0.00450 / 0.00500 ms**. Opt-level `s`: medians **0.00300–0.00450 / 0.00340–0.00790 ms**, worst p95 **0.00460 / 0.00950 ms**. CLI executable **2,297,344 vs 1,839,104 bytes**. Retain opt-level 3: `s` slowed the larger workload. These are separate non-randomized warm runs, not parity evidence. Existing-file fixture checks were timed separately from queries.

- `seek-cli self-test --output docs/benchmarks/correctness-extended.json`: **3,015 indexed / ground-truth entries**, **0 missing / extra / stale**, initial, live and stopped-engine/relaunch phases. Hard links, junction loop, on-disk unpaired surrogate and live/closed case-only renames added. Live reconciliation **83.012 ms**; walker catch-up **3.233 ms**. Accuracy **10/10 top-five**, adding exact-name, prefix and path scope; no upstream typo-rate comparison.

- Patched debug Acrylic spike: **0 ms CPU / 11.726 s**, **34.32 MiB private working set**; debug Mica: **0 ms / 20.003 s**, **34.16 MiB**, close **57.697 ms**, exit 0. Release repeat samples: **0 ms / 20.019 s** and **0 ms / 20.005 s**, around **29 MiB private working set**; foreground flags were false, so these do not verify active-window idle. Another release sample used **140.625 ms / 20.012 s** without focus/input attribution. Preserve that conflicting sample; do not certify universal zero idle cost.

- `scripts/measure-spike.ps1 ... -Root .bench-data/1000000/files -Data .bench-data/close-during-index -CloseDuringIndex`: fresh million-file index interrupted, process exited in **58.909 ms**, code 0. This proves close during that scan, not all shutdown/persistence races.

- Extended engine/CLI suite: **10 tests passed, 0 failed**. Standalone scheduler test command `rustc --test vendor/gpui-windows/src/frame_demand.rs -o target/frame-demand-tests.exe`, then run it: **2 passed**, including 1,000 wake/wait exchanges. Final workspace command outcomes are recorded in `docs/benchmarks/continuation-validation.json` after completion.

- Final `cargo build --workspace --locked` without benchmark features: passed in **30.37 s**. All-features build **4.37 s**, tests **2.68 s** (10 passed), Clippy **1.86 s** with warnings denied, formatting passed. Final instrumented release rebuild **5m 20s**; it opened the real 10k shell and closed with code 0. `final-release-run.json` reports close **166.606 ms** and labels its combined launch/exit interval correctly; it is not another first-paint measurement.

- Integrated CLI invocation `seek-cli bench --files 10000 --runs 30 --ui-executable target/x86_64-pc-windows-msvc/release/seek.exe --output docs/benchmarks/cli-bench-integrated.json`: exit 0, backend plus foreground UI report parsed successfully, 30 measured samples per query and application exit 0. Retain the dedicated standalone runs above as the primary comparison.



## Earlier checkpoint measurements and commands (retained history)



Hardware: Windows 11 Home build 26300; Ryzen 7 7445HS; 12 logical processors; NTFS C:. SDK 10.0.26100.0 and VS 2022 Build Tools.



- `cargo check -p seek`: current kit/pin compiled after alignment. `cargo build -p seek` ran the spike and later the shell. `cargo build -p seek --release` built the release spike in 12m 22s; release spike ran and closed with code 0. Final shell uses the debug profile.

- `scripts/measure-spike.ps1 -Executable target/x86_64-pc-windows-msvc/debug/seek.exe -Close`: debug spike private working set **32.79 MiB**; static-caret sample **32.45 MiB**. Release spike **60.48 MiB**. Completed spike close tests exited with code 0. Historical `input_idle_ms` fields actually measure window-handle availability; script label corrected.

- Scoped debug shell: private working set **29.99 MiB**, total working set **90.46 MiB**, private commit **73.61 MiB**. Close exited within the 10-second wait; attached-process exit code unavailable. Quiet sample **609.375 ms CPU / 20,010.137 ms wall**, about **3.045% of one core / 0.254% of the machine**: FAIL for zero idle cost.

- `cargo build -p seek-cli --release --target-dir target/engine`, then `seek-cli self-test --output docs/benchmarks/correctness-current.json`: **3,012 ground-truth entries**, **0 missing / 0 extra / 0 stale** at initial, live-change and stopped-engine/relaunch phases. Live reconciliation **210.377 ms**, walker relaunch catch-up **5.484 ms**. Accuracy **7/7 top-five, 100%**, including missing-letter and transposed-letter examples; no comparison with upstream's 98% claim.

- `seek-cli bench --files 10000 --runs 30 --output docs/benchmarks/10000-opt3.json`: three exact queries, 3 warmups + 30 runs each; medians **0.00675–0.00690 ms**, worst p95 **0.01460 ms**. First index **27.234 ms**, **76.16 bytes/entry**.

- `seek-cli bench --files 1000000 --runs 30 --output docs/benchmarks/1000000-opt3.json`: medians **0.00480–0.00660 ms**, worst p95 **0.02530 ms**. Generation **618.587 s**; first index **3.213 s**; **1,001,008 entries**, **76,176,256 index bytes**, **76.10 bytes/entry**. Backend only; no painting or parity claim. These baseline runs precede the final attribute/cancellation/persistence refinements.

- `seek-cli index --root C:\ --data .bench-data/drive-walker --output docs/benchmarks/drive-walker.json`: **38.847 s** initial ready interval, **2,868,114 entries**, **169,610,112 bytes**, **183 skipped locations**. Includes the generated million-file fixture. Whole-drive verification was not run.

- `cargo test -p seek-engine -p seek-cli --target-dir target/engine-test`: **9 passed, 0 failed** (3 path, 5 query/ranking, 1 filesystem lifecycle). Final `cargo test --workspace`: **9 passed, 0 failed**, including the application test target. Final `cargo build --workspace`: passed, **4.65 s**. Final `cargo clippy --workspace --all-targets -- -D warnings`: passed, **1.76 s**. `cargo fmt --all` applied. Final debug CLI `verify` on the same fixture again reported **0 missing / 0 extra / 0 stale**; `verify-final.json` records the independent comparison. Final debug shell on the 10,000-file dataset opened and WM_CLOSE exited with code 0; see `shell-final-run.json`.



Raw JSONs and methodology: `docs/benchmarks/REPORT.md`. Local intermediate logs remain under ignored `.references`. Fixtures/build output are ignored and retained for reproduction.



## Remaining product acceptance and hardening checks



- The previous warm-cache launch blocker improved. Current saved-index medians are below 300 ms using the Windows process-creation clock, but a 1M sample took 315.502 ms and true OS-cache-cold launch remains unmeasured. Reliable cold-start acceptance remains open; do not claim universal sub-300 ms launch.

- Sustained quiet foreground idle has not been certified: monitored samples lost foreground or received input, and the latest spike also recorded an unattributed CPU-heavy sample. The short debug sample reached zero, but does not settle the hard requirement. Demand scheduling remains under validation.

- Unsupported Windows Acrylic → Mica → solid fallback, GPU loss/recovery, touchpad inertia, multi-monitor/DPI behavior and explicit Mica appearance still need validation. API success alone does not guarantee the visible effect under every Windows transparency setting.

- Full shortcut/context-menu coverage, column drag resizing, stale-file-open races and DPI/monitor migration need broader validation. Real GUI lifecycle, chip removal, sorting, arrows, two-step Escape and ordinary geometry restoration now pass.

- Cooperative cancellation does not interrupt every scoring pass. Broad/fuzzy GUI queries with hidden-view filtering need latency certification; current UI benchmark samples are unique exact-name queries. Unicode/typo highlight ranges have two new unit tests, and quoted chip spans retain their existing parser test.

- Access-denied subtrees and watcher overflow/heavy/in-flight activity need stress testing. Already queued notifications now drain before the catch-up warning clears; this is not a claim of an atomic filesystem snapshot under continuous mutation.

- Cache/mmap/prefault/overlay memory tuning remains unfinished. Stage 2 helper/MFT/USN implementation and its current validation blocker are reported at the top. Later product views/settings/themes/previews remain in their assigned stages.



## Assumptions



- No Finder screenshots were attached in this session, so the partial light shell follows the written layout; visual matching is unverified.

- Stage 1 uses only the unelevated walker. No helper/admin benchmarks can be reported yet.

- Relist **all** indexed directories once on walker launch: Windows directory mtime alone cannot reliably expose closed-app file-content/metadata modifications. This intentionally chooses correctness over the fork's cheaper changed-directory-only heuristic; it is slower for large real drives and is disclosed.

- The core retains second-resolution mtimes and its compact size encoding. Query benchmarks report backend work separately from GUI work; small accuracy fixtures do not reproduce upstream typo statistics.

- Keep GPUI and current Kit. No framework replacement, helper installation, autostart, tray or global hotkeys.



## Exact next step



Finish the Stage 2 administrator update/retest gate described at the top and in docs/STAGE2-ADMIN-TESTS.md. Preserve the verified walker path. Do not start Stage 3 before that gate passes. Retain cold-cache/foreground-idle/broad-query/device/stress acceptance checks for Stage 4.
