# Stage 3 product checks

Stage 3 follows the supplied dark and light Finder references, adapted to native Windows caption buttons and Windows' rounded corner radius. GPUI Kit remains **0.7.1**, GPUI remains at **279fe070bb389b79652e52065b2f001edcc0b11b**, and the patched Windows crate is unchanged.

The app has continuous glass, a compact unstriped list, sortable/resizable headers, native Windows file icons with bundled fallbacks, List/Icons/Gallery, and a right-hand selected-item panel. The panel includes the full path, created/modified dates, the most recent successful open through Seek, and bounded text/code/Markdown previews. Space toggles it. Images use real previews; video uses a Windows thumbnail when available. Other file types use Windows association icons with bundled fallbacks. Folder rows share one native yellow icon; image files named folder.png retain their real previews. Unknown extensions request only a shell icon.

Folders expand inline in List view using direct children from the existing base index and live overlay. Right expands, Left collapses, and disclosure clicks toggle. Nested children retain indentation, each sibling group follows the current sort, and hidden/system visibility applies. Expansion is cleared when the search changes. Search matches remain capped at 200; expanded direct children are not truncated. The list and grid draw only visible rows. No folder scan occurs to expand a row.

Previews run on one low-priority COM worker. Queued jobs are canceled when rows leave view; an already-running Windows provider call cannot be forcibly interrupted, and its obsolete reply is discarded. The shared LRU holds at most **96 entries / 32 MiB**; icon cells are rasterized once into a fixed atlas. SVG has a bounded rasterizer fallback when Windows has no provider. HEIC and other optional formats depend on installed Windows codecs/providers.

Theme defaults to System and glass defaults on, with Acrylic → Mica → solid fallback. Settings include manual themes, glass, accent presets/custom hex, visibility, default view, locations, exclusions, reconciliation/rebuild and the existing optional helper controls. Locations apply on the next launch; exclusions hide results and retain shared index metadata. Settings I/O runs off the UI thread and preserves saved geometry. A per-user, per-data-directory instance signal restores the existing window. There is no tray, app autostart or global hotkey.

Build with Rust **1.96.0 x86_64-pc-windows-msvc**, VS 2022 Desktop development with C++ and Windows SDK **10.0.26100.0**, from an x64 Native Tools terminal:

```powershell
cargo build --workspace --release --locked
./target/x86_64-pc-windows-msvc/release/seek.exe
```

The optional helper executable should remain beside Seek. The desktop app runs as an ordinary user. These Stage 3 checks do not update or install the service.

Focused GUI checks use an instrumented build and isolated Seek-owned fixture files:

```powershell
cargo build --workspace --release --all-features --locked
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/check-stage3.ps1 -Executable target/x86_64-pc-windows-msvc/release/seek.exe -Output docs/benchmarks/stage3-release-ui-check.json
```

This local harness uses the installed Orca desktop driver for fresh semantic button clicks and independently asserts their effects. Normal app builds and Rust tests do not depend on Orca. The synthetic Word/Excel/PDF files test icons, not document rendering. The optional MP4 fixture is a one-second H.264 test pattern generated locally with the already-installed ffmpeg.

Check by eye: both themes and glass off; sidebar selection, toolbar, columns and unstriped rows; image/video previews and text details; Icons and Gallery; nested folders using arrows and Right/Left, including sorting and hidden files; breadcrumb and item count. See the Stage 3 report in [benchmarks](benchmarks/stage3-ui-check.json). Sustained idle, broad/large-drive latency distributions, memory tuning and heavy churn remain Stage 4 work. No deferred “Later ideas” were started.


## Targeted appearance refinements, 2026-10-15

The latest user toolbar mockup governs the 52 px header: sidebar and grouped history left, a flexible 180-560 by 32 px rounded-rectangle search in the middle, then thin separators and glass groups for views, actions and native window controls. Location title, history dropdown and Ctrl+K badge are omitted. Toolbar artwork is regular weight and muted gray. Icon-only buttons have zero internal padding, non-shrinking artwork and a 1 px keyboard-focus line. The document-shaped button toggles hidden/system visibility; Info highlights only when its inspector is visible. List icons match 16 px Favorites artwork. Settings have compact headings, square accent swatches and centered small labels. Selection uses an 18% accent wash and normal foreground, including Light Graphite. Sidebar movement lasts 160 ms, reverses from its current width and stops requesting animation frames when finished.

Gallery has one large preview above the filmstrip; the inspector shows metadata. No empty preview/inspector areas are reserved without selection. Empty folder browsing shows every direct indexed child, with sorting and hidden visibility. The 200-hit cap remains for searches/global recents.

Glass follows [Zeron's native GPUI background approach](https://github.com/zeronsh/zeron/blob/54954e4d22bd4d78b2e2c7160a913b1af548dc79/crates/ui/src/appearance.rs). Kit's root stays transparent; one 80% neutral tint and GPUI-owned Acrylic/Mica reveal the desktop across the app. Sidebar/toolbar add no opaque fill; an extra DWM frame was removed. [Segoe Fluent Icons](https://learn.microsoft.com/en-us/windows/apps/design/iconography/segoe-fluent-icons-font) supply native Windows toolbar artwork. GPUI/Kit/Windows patch versions are unchanged.

Evidence: `benchmarks/stage3-toolbar-*.json`, preceding `stage3-targeted-*.json`, the retained 23-check `stage3-taste-ui-final.json` and `scripts/check-stage3-appearance.ps1`. The focused header check records all 14 control bounds and rejects clipped or overlapping targets. These are fixture checks, not new drive benchmarks. Other DPI/HDR displays, Mica fallback and arbitrary wallpaper readability remain manual checks. Earlier 10k benchmarks precede the final visuals; no parity claim is made.

Toolbar release baseline before the final Settings follow-up: 7/7 focused GUI checks pass; private working set 40.62 MiB and private commit 85.95 MiB in the small appearance fixture; close 125.887 ms / exit 0. Current binary evidence is in [the final summary](benchmarks/stage3-summary.json).

## Final Settings mockups

Settings now follows the supplied four-page mockups: a frosted rounded sheet, left navigation, separate cards and neutral glass controls. Fixed 16 px thin monitor/sun/moon icons keep theme choices compact. The rounded default-view frame closes immediately after Gallery. Graphite is #909090, interpreting the user's "90"; legacy #707070 presets migrate, and other custom colors remain. A working color picker complements hex entry. Accent is limited to selected controls, the glass switch and Save locations; secondary actions stay neutral.

Reproduce the focused settings check with the installed Orca desktop driver, leaving its temporary window untouched:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/check-stage3-settings.ps1 -Executable target/x86_64-pc-windows-msvc/release/seek.exe -Output docs/benchmarks/stage3-settings-release-ui.json
```

The check uses owned fixtures, tests real preference writes and restart readback, captures all four pages in both themes, checks the Gallery frame bounds, and closes the process. Helper status is read without installation or removal. Focused Rust tests pass 15/15, and Clippy/formatting/workspace builds pass; detailed evidence is in `benchmarks/stage3-settings-*.json`. Earlier full-product/engine checks remain valid and were not repeated for this Settings-only change. Check by eye: the smaller theme icons, neutral glass buttons, compact rounded view selector and softened Graphite. Other DPI/HDR displays, Mica fallback and arbitrary wallpapers remain visually unchecked.

Final release Settings checks pass **7/7**. Small-fixture private working set is **38.00 MiB**, private commit **91.84 MiB**; close is **89.168 ms**, exit 0. These are interaction-fixture measurements, not sustained-idle or whole-drive benchmarks. The compact Helper action also passes the Light/Dark visual confirmation, with final review disposition **ship**. The ordinary real-profile release app was reopened; [the summary](benchmarks/stage3-summary.json) records its executable hash and retained earlier checks.
