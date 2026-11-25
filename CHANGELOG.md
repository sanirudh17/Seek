# Changelog

## 1.1.1 - 2026-11-25

Seek 1.1.1 makes Cleanup much faster and gives you control over what it removes, lets Storage measure any drive, speeds up All locations, and fixes several window and startup glitches. Saved data from earlier versions is kept.

#### Cleanup

- Each cleanup group has its own checkbox. Clean up selected removes only the groups you ticked, and each group can still be cleaned on its own.
- Caches, temporary files and build folders are now deleted in place instead of being sent to the Recycle Bin. Gigabytes of cache that used to stall in the Recycle Bin dialog now clear in seconds. Downloaded AI models, unfinished downloads and old installers still go to the Recycle Bin.
- Cleaning shows live progress (files and space removed) and a Stop button that takes effect at once. Seek stays responsive while it works.
- Read-only files and links inside caches are handled safely: links are removed themselves, never followed.

#### Storage

- The drive picker lists every drive on the PC and always opens.
- Storage can measure a whole drive, not only indexed folders.
- While the index is still loading, Storage waits and fills in by itself instead of showing an empty page. A drive that is not indexed says so and points to Settings > Locations.

#### Search and browsing

- All locations opens instantly, showing the last results at once while the index answers.
- Jump (Ctrl+K) and the other shortcuts work while the Storage and Worktrees pages are open.

#### Windows and appearance

- Seek's first window appears with its glass already in place, without a see-through or grey flash.
- Column headers and the Settings panel use the glass appearance like the rest of the window.
- The window border is a single line that follows the rounded corners all the way round, with no translucent rim along the edges.
- Relaunching Seek from the taskbar while it is still closing now opens a new window instead of doing nothing.
- Setting descriptions appear in a compact box instead of a long tooltip.

## 1.1.0 - 2026-11-25

Seek 1.1.0 is a large update that turns Seek from a search window into a full file manager: split panes, new views, renaming and organizing tools, a storage page with cleanup and duplicate finding, an MCP server for coding agents, and live folder updates. Saved data from earlier versions is kept.

#### Browsing and windows

- Split panes: split right (Ctrl+\) or down (Ctrl+Shift+\), up to eight panes, each with its own tabs, view, history, selection and scroll. Drag dividers, zoom one pane (Ctrl+Shift+Enter), move focus with F6, and copy or move the selection to the other pane with Alt+F5 and Alt+F6.
- Drag a tab or a sidebar folder onto a pane to split it; clicking a pane focuses it.
- Column view (Ctrl+4) with ancestor and preview columns, alongside List, Icons and Gallery.
- Icon view options (Ctrl+J): icon size, spacing, label position, extra details and thumbnails, remembered per pane.
- Gallery's large preview can be hidden.
- Document panes keep a text, code, Markdown, picture or document file open beside the folder you browse; Ctrl+W closes the focused one.
- Related folders: an optional row of folder chips under the tabs.
- Folders on screen now update the moment they change on disk, including folders outside the indexed locations. A finished download appears at the top of the list at once, and deleted items disappear at once.
- Panes, tabs and folders reopen as you left them; this can be turned off in Settings > Files and search.
- Type a path, an environment variable such as %TEMP%, ~ or cd <folder> in the search field to go straight to a folder.
- ZIP files open like folders (read-only), with Extract here and Compress to ZIP.
- Sidebar groups you create, name, reorder and collapse; drag folders in to pin them. Sidebar icons now carry the accent colour.
- Ctrl+B toggles the sidebar from anywhere; Ctrl+, opens and closes Settings.
- Right-clicking empty space offers New folder, New file, Organize, Paste, view and sort options.

#### Search

- Ranking that favours your own folders: results in Downloads, Desktop, Documents, Pictures and your profile come before matches in app data, Windows, Program Files and build folders, which are demoted but never hidden. Folder weights can be edited in Settings > Files and search > Search ranking.
- New filters mod: and -in: (leave out a folder), and a ? button in the search field that lists the whole syntax.
- Searches work while the index is still being built, and in folders it does not cover.
- Saved and recent searches in the sidebar and in Jump.

#### Files and safety

- Undo and redo for renames, moves, copies and recycling, with an operation history that recovers interrupted work and Put Back for recycled items.
- Batch rename with live preview: Replace text, Number names, and an Advanced mode with a stack of steps (replace with optional regular expressions, add text, overwrite, remove, change case), variables, conflict handling and saved presets. One Undo reverts the whole rename.
- Suggest Names reads a name from each file: a document's title or first heading, a slide deck's first title, an invoice's kind, number and date, music tags, or a photo's date taken. The rules are fixed, so the same file always gets the same suggestion. Names that say nothing (scan0001, untitled, IMG_1234, random IDs) are filled in; a name you chose is only ever offered an alternative. Titles stored in PDF and Office files are used only when the text bears them out, and codes, keys and cover-page headers are never offered as names.
- Organize a folder by type, by date taken (Year and Month folders) or by name (camera sequences and versions of one document), with a review of each group first and one Undo for everything.
- Image tools: convert, resize, rotate and flip copies of pictures, never overwriting the original.
- Consecutive selected rows show as one highlighted block.

#### Storage

- A Storage page with free space, a breakdown by kind and the largest folders with drill-down. Every kind in the key is visible in the bar.
- Cleanup suggestions for caches apps rebuild on their own (developer tools, browsers, editors, chat apps, graphics drivers), old temporary files, crash dumps, app logs, unfinished downloads, old installers and build folders in projects untouched for 90 days. Sign-in tokens, locally installed packages and tracked source folders are never offered, and even small amounts of space are listed. Everything goes to the Recycle Bin and can be undone; emptying the Recycle Bin asks first.
- Find duplicates, in a folder or everywhere: files are confirmed identical by their full contents and checked again just before moving. Hard links, cloud placeholders and system files are left out, and identical folders are found too.
- Similar images, in a folder or everywhere: resized, recompressed and lightly edited copies at three levels, compared upright and by colour and checked at a finer resolution, so different screenshots of one app or unrelated blank pictures are not grouped. Pictures are shown side by side and nothing is selected for you.
- Jump (Ctrl+K) now opens Storage, Find duplicates and Find similar images directly, and starts the search in the folder you are in.

#### Worktrees

- A Worktrees page lists Git repositories and their worktrees with branch, status and rebuildable size. Removing a worktree shows what would be lost and can be undone.

#### Coding agents (MCP)

- seek.exe --mcp makes Seek an MCP server, so agents such as Claude Code, Codex and others can search, read and tidy files through it.
- Settings > Agents: allow agents to change files (off by default), choose the folders they may change, offer every tool, see which agents are connected, connect supported agents, and read an activity log.
- Every agent change stays inside the allowed folders, goes through the same undo history, uses the Recycle Bin and never overwrites; organizing needs a reviewed plan.
- With Seek closed, read-only tools answer from the saved index, which is now saved whenever Seek closes so it includes the latest files.

#### Windows integration

- Seek as the default file manager (optional): folders and drives you open from anywhere, and Win+E, open in Seek. Turning it off restores File Explorer.
- Open in Seek on folders and drives and Show in Seek on files in Explorer's right-click menu.
- Seek is listed when Windows asks which app to open a file with.
- Uninstalling now really restores File Explorer as the folder handler (earlier uninstallers could leave it unset).
- A test or portable copy run with its own data folder never changes Explorer's entries.

#### Appearance

- Glass that reads as glass: one palette-coloured tint, a clear file list and a soft edge. Glass and background settings for tint, blur and a background picture with effects.
- Palettes (Graphite, Harbor, Moss, Ember, Orchid, Tidepool, Dune, Rosewood) set the glass colour, text and accent; Intensity sets how strongly they colour the window, including panes and dialogs.
- One motion system for all animations, which follows the Windows animation setting.
- Checkboxes replace toggles where you choose items; segmented controls show the current mode; the faint duplicate window buttons behind the toolbar are gone.

#### Quick Look and editing

- Quick Look for pictures (zoom, pan, actual size), PDFs, Word and Excel, with keyboard navigation.
- Text, code and Markdown editing with find, rendered Markdown, optional auto-save and backups, and safe saves that keep the file's encoding.

#### Fixes

- The Storage kind bar draws each kind to scale and the key never reads 0% for something that is there.
- Search ranking's Edit opens one Notepad window; its status shows when a restart is needed.
- Documents open beside a wide pane instead of as a narrow column; the status bar path ends in an ellipsis instead of being cut off.
- Many smaller layout, wording and alignment fixes.

#### Performance

- The window no longer redraws continuously while the index catches up.
- Similar images decodes a bounded number of pictures at once, keeping memory to a few hundred MB at most on large folders.

## 1.0.2 - 2026-10-28

- Fixed PDF preview stalls with bounded renderer output collection, cancellation, timeout and Retry.
- Added an enlarged reader with a dedicated toolbar, fixed page controls, Up/Down file navigation, Left/Right page navigation and Escape to close.
- Reduced PDF page-turn overhead with native image decoding, a short-lived selected-PDF renderer and a bounded memory-only cache with adjacent-page prefetch. Cached Word previews are reused too.
- Added modern PowerPoint saved-first-slide artwork and ordered slide-text previews. Full graphical rendering of every slide is not supported.
- All locations now includes files and folders across 500-item pages, with the complete count and pagination controls at the bottom.
- Added Explorer-style folder addresses such as `%TEMP%`, `%USERPROFILE%`, `%APPDATA%`, drive/UNC paths and `~`. Press Enter or choose Open folder; addresses bypass Name and Content scans.
- Centered Enlarge preview and reused the Settings glass-button style in both themes. Kept presentation controls in a fixed position and removed underlying app controls from the enlarged reader.
- Preserved the 1.0.1 installer hotfix, per-user installation, optional desktop shortcut and saved-data retention on uninstall. The recommended download is the Setup EXE; a separate ZIP supports existing in-app updates.

Preview limits remain 32 MiB per document and four seconds per request. Performance depends on the document and system; these changes do not establish a cold-cache startup or 16 ms input-to-present guarantee. No framework version updates.

## 1.0.1 - 2026-10-20

- Added a per-user Setup EXE and native uninstaller, with an optional desktop shortcut and saved-data retention by default. The helper is never installed automatically.
- Corrected the Setup EXE's optional test-argument error handling; the published installer now writes and registers its native uninstaller instead of falsely reporting an installation failure.
- Enlarged the content-search location field.
- All locations now shows labelled recent files during a first index instead of waiting for the full drive index. Early results remain provisional while the complete scan continues.
- Fixed search-suggestion placement relative to the actual search field on resize.
- Tabs retain bounded shared row snapshots while refreshing; list icons no longer wait behind large previews. Added brief tab transitions respecting Windows animation preferences.
- Improved Unicode and legacy Windows text decoding; binary/mixed logs show an explicit unavailable state instead of lossy gibberish.
- Confirmed deletions immediately update active/saved views and cannot return from stale index/cache replies. Enabled Windows operation progress/error dialogs.
- Improved Ctrl/Shift selection, keyboard focus and wheel routing. Permanent deletion defaults to Delete permanently, with arrow/Tab navigation and Escape cancellation, as requested.
- Added supported-Windows system acrylic handling for glass transitions; older systems retain the fallback.
- Existing in-app updates continue through a separate checksummed ZIP asset. No framework upgrade.

Coverage limits include the original mixed-format driver log, HDR/mixed DPI, Windows 10, a second Windows account and visual proof of every transition. Test-profile timings are not release input-to-paint guarantees.

## 1.0.0 - 2026-10-20

Initial public release: filename indexing, tabs, GPUI menus, file operations, scoped content search with saved document text, previews, optional NTFS helper and explicit GitHub updates.
