# Phase-two review checklist

Use disposable fixture files and the separate review profile identified in `.references/phase2-checkpoint-manifest.json`. Keep production files out of mutation tests. The full [shortcut reference](PHASE2-SHORTCUTS.md) is generated from the same registry as Settings > Keyboard shortcuts (F1).

For feedback, name the section, file, keys/actions and expected versus actual result. A screenshot is useful for layout; a short recording is more useful for flashes or focus transitions.

## Navigation, results and search

- Browse a folder, resize/maximize/restore the window, and check that Name, Date Modified, Size and Type share the available width.
- All locations must always use a search tab icon; folder tabs use folder icons. Check the range/total footer and Previous/Next on an owned index with more than 500 entries.
- Try Alt+0 and Alt+1 through Alt+9 for available favorites. Ctrl+1/2/3 should continue to change List/Icons/Gallery.
- Search by filename, add a type/filter chip and remove/change it. Existing engine syntax should retain its meaning.
- Save a query from Ctrl+K and recall it there, restart the review profile, and recall it again. Location and filename/content mode should be retained.
- Search contents for `needle` in the fixture text. Select the result: the list stays compact while the information panel shows up to five numbered matching lines. Longer excerpts and further matches are labelled.

## Selection and commands

- Test Ctrl+click, Shift+click, Shift+arrows and Ctrl+arrows; selection count and the information panel should follow actual selected items.
- In Ctrl+K, check Locations, Searches, Recent files, Saved commands, Views and tabs, Files, Operations and Settings headings. Search a category name to show its actions; headings with no matches disappear. Confirm a filtered action from a later group to check correct routing.
- Open Ctrl+K with no selection. Selection-dependent commands should be unavailable; Escape returns to a stable browser control.
- Open/close the information panel with Ctrl+I. Column headers should not acquire a distracting selected frame.
- Create two tabs, then test Ctrl+Tab and Ctrl+Shift+Tab from the search and file list. Each tab should preserve its path/query/view/selection/scroll.

## Quick Look and documents

- Select text, Rust, Markdown, PNG, PDF, Word and Excel fixtures and press Space. Escape should close Find before closing Quick Look; ordinary spaces typed into Find/Edit must remain text.
- In code/Markdown Source, Ctrl+F should show a compact Find field with a readable Match case control. Clear/close Find, save/finish editing and test Escape again.
- Markdown must render its local image and task list. Toggle a checkbox while editing, save, close and reopen. Test an ordinary link/web image only if you choose to load external content.
- For the large original PNG, check the footer's original dimensions. Ctrl+1 means actual decoded pixels, Ctrl+0 fits, Ctrl++/Ctrl+- zoom, and Shift+arrows or dragging pans within both axes. Switching files/Fit resets pan.
- Check PDF page changes and Word/Excel scrolling. PowerPoint remains details-only. Broken/over-limit documents should show a reason/Retry rather than crash or blacken the app.

## Text saves and recovery

- Edit the owned text/Markdown file and press Ctrl+S. Verify the actual file bytes outside the preview.
- Close a dirty editor with Escape: Save, Discard and Cancel must have distinct results. Cancel keeps the draft; failed/conflicting saves keep the draft too.
- In Text editing settings enable Auto-save, type, wait about a second and inspect the file. Disable it and confirm edits require explicit Save.
- Enable Keep backups before the first change to an owned Markdown file, save its first checkbox/text edit, and inspect the backup. Disable backups for a second file and confirm ordinary successful saves do not leave a backup. Recovery files after a failed save are distinct from optional backups.

## File operations, grouping and batch names

- F2: rename an owned file, then try an existing sibling's exact name. The collision must preserve both files and show an error in the name sheet; leading spaces represent a different name and are disclosed.
- Ctrl+Z/Ctrl+Shift+Z undo/redo an eligible rename or move from the file list. Change a moved item or occupy its original destination before Undo: no selected root should move during failed preflight.
- Copy owned files and a folder. Undo recycles only the verified copies; Redo restores those exact copied versions even if the source changed. Never use this test on a pre-existing merge/overwrite target.
- Recycle an owned file/folder, use History > Put Back, and test redo recycling. Existing original destinations must be preserved.
- New Folder with Selection must refuse an existing folder name. Successful grouping moves the chosen siblings; Undo returns them and keeps the new folder, including anything subsequently added there.
- Batch rename: compare original/proposed names, test text replacement and numbered names, and try duplicate/occupied destinations. Apply is blocked unless conflicts are explicitly skipped. Undo/redo should preserve bytes. Automated tests cover swaps, cancellation, rollback and blocked rollback; do not simulate crashes using real files.
- History > Review interrupted only inspects evidence; it must not replay a mutation. Archive and start fresh preserves the previous journal file. Stop preserves already completed item results.

## Settings, saved programs and drag behavior

- Visit all eleven settings pages and search for a setting. Default-view choices should be text-only rounded rectangles with centered labels. Tab/Shift+Tab and Escape stay within/leave the sheet predictably.
- Previews: disable automatic previews and verify the information panel still shows metadata while content stops loading. Explicit Space Quick Look should still work. Disable thumbnails and check association artwork in browsing; re-enable and check image/video thumbnails return.
- Saved commands: configure an installed `.exe` you intend to run, one argument per line and the selection requirement. Saving must not launch it. Use its explicit Run action from Jump; test zero/one/multiple selections and paths with spaces. `{files}` occupies its own line. The result sheet should show running status, then the actual exit status and output. Closing it leaves a running program running.
- Drag only owned fixtures. Valid folder targets should highlight; self/descendant/no-op targets should refuse the move. Hover a list folder to expand it, move away or press Escape to cancel, and check that no move happens until drop. External drag-out offers Copy.
- Recent entries in Jump and location capacity should appear without periodic activity. A recent entry locates its parent. Favorites keep working Alt shortcuts without sidebar hints; saved/recent search blocks stay out of the sidebar.
- Check bright-background glass in restored/maximized windows, minimize/restore and switching apps. The system transition must remain smooth; a static screenshot cannot certify absence of a transient flash.

Physical second-volume moves, HDR/mixed-DPI and other-machine behavior require appropriate hardware and remain explicit coverage limits when unavailable.
