# Current revamp build - review checklist

Worktree: the revamp worktree
Branch: revamp/glass-quicklook-panes
Version remains 1.0.2. These are local changes; nothing has been pushed, tagged or released. This checklist describes implemented work, not completion of the full revamp.

## Latest text editing and layout checkpoint (bundled review)

- [ ] Check Name, Date Modified, Size and Type at restored and maximized widths, then toggle sidebar/inspector. Columns should use available space and filenames should have room. Narrow windows may scroll horizontally.
- [ ] Settings > Files and search: 8 px rounded-rectangle, text-only List / Icons / Gallery choices with centered labels. Settings > Text editing: document/script icon, Auto-save and Keep backups.
- [ ] Quick Look Markdown: Read/Source, local/remote/data images, tasks, Find (36 px input), Edit/Save/More. Inspector shows readable text instead of a generic framed file icon.
- [ ] In Find, Space types and Escape closes Find. In editing, Space types. Save, then More > Finish editing, then Escape closes the reader. Escape during a pending save closes after successful completion; errors/newer drafts remain protected.
- [ ] Auto-save: enable in Text editing, edit an owned fixture, wait a second, confirm Saved and bytes on disk. Change the file externally before saving: preserve draft and show conflict. With backups off, no successful-save .bak remains; Undo saved change still works in the session.

## Command palette checkpoint

Final refinements are source/build verified and await the next bundled review; the previously opened review copy predates the final typography/launcher adjustment.

- [ ] Ctrl+K or the keyboard-marked Jump button at the sidebar bottom opens the search/list overlay; type a location, settings page or command. The query is 40 px tall; query, rows and footer share 13 px text. Up/Down selects and Enter executes the matching original command. No matches shows a useful message.
- [ ] Escape, Ctrl+K and outside click close and restore focus. Tab stays in the query. Quick Look editing and rename/native dialogs are protected.
- [ ] With no selection, inspector and selection actions are disabled, including after deselecting the last item with Ctrl+click. Rename requires one selection; Quick Look requires a selected file. Select an owned file, open the palette and check Quick Look/Copy path. If files or scope change while it is open, execution must ask to choose again.

## Operations foundation (next bundled review)

- [ ] On an owned small text file: F2 renames, Ctrl+Z restores the previous name, Ctrl+Shift+Z redoes it. These keys apply when the file list has focus; text editing keeps editor undo.
- [ ] File actions > Operation history or Ctrl+K > Operation history shows actual completed, failed, skipped and interrupted item results. Escape returns focus; Tab stays inside History.
- [ ] A conflicting rename preserves both files. Change the renamed file externally: Undo must refuse while retaining the changed file. Replace it with a different file containing identical bytes: Undo must also refuse.
- [ ] Rename an eligible file, restart the owned review profile, then undo through History. Interrupted jobs must be reviewable and never replay automatically.
- [ ] These undo controls currently apply to eligible regular-file renames up to 16 MiB. Copy/move/directory undo and Put Back are still pending and should not be presented as complete.

## Visible changes to check

- [ ] Inspector metadata foundation: Details shows exact bytes and size on disk; Dates retains existing timestamps; Storage includes the path and Windows attributes. Unknown sizes say Unavailable. Folders and multiple selection show asynchronous totals with Cancel/Recalculate and explicit partial counts. Check owner/volume, Open, Reveal, ordered Copy path and Windows Share. GUI acceptance is pending.
- [ ] Ctrl+I keeps keyboard row selection without outlining the entire results frame. All locations uses Search consistently for active, saved and closing tabs; actual folder tabs retain Folder.
- [ ] Minimize from restored and maximized states: verify the shrinking window retains dark material without a pale flash; also check restore/deactivation and Windows animation-off. Native guard now covers iconic-but-visible windows.
- [ ] Dark glass: resize, maximize, restore and deactivate the window. Dark tint should remain with live blur. Mild Windows backdrop movement is accepted; transient flashes and controlled idle acceptance remain open.
- [ ] Glass sliders: window tint spans 0-100%; sidebar adds a gentler cubic tint. Change both live and reopen Settings to confirm values persist. Keyboard arrows/Home/End work on the slider group; mouse release leaves no focus rectangle.
- [ ] Settings popup: Appearance, Glass, Background, Files and search, Text editing, Locations, Helper and Updates retain their options. Search is separate from navigation. Long descriptions stack above controls. Background softening has its own row. Save content locations is compact, and Clear cache is below the document-text toggle.
- [ ] Optional background: choose a bounded local PNG, toggle softening, adjust image opacity and remove it. File contents are never modified. Complete picker/background acceptance is still pending.
- [ ] Typography/icons/alignment: Figtree UI text, consistent Hugeicons, larger sidebar section headings, centered table headings and search text.
- [ ] Tabs: compact icon tiles, quieter active/inactive fills and Plus beside the last tab. Open/close uses a 200ms slot animation. Closing uses a stable viewport/clipped fading label. Check close behavior at the beginning/middle/end, a crowded strip and with Windows animations disabled.
- [ ] Folder results: double-click or Enter enters a folder in the current tab. Back/Forward should restore the previous search/folder. Ctrl+Enter opens the selected folder in another tab. This routing fix builds but needs live acceptance.
- [ ] Presentations: PowerPoint-family files display normal details only. Double-click or Enter uses the Windows association, just like other files. Dedicated Open in PowerPoint buttons and partial slide/text previews are removed. PDF raster behavior is preserved; Word retains its existing text fallback.
- [ ] Preview recovery: errors show their actual reason, Retry and Copy diagnostics. Host startup has a longer first allowance, a bounded retry, version/protocol checking and rotating diagnostics. The original multi-day incident has not been reproduced or attributed conclusively.
- [ ] Startup: native placement stays hidden until the first painted frame; saved maximized placement is retained. Visual flash and controlled idle acceptance across all states remain pending.

## Tab shortcuts currently implemented

- Ctrl+T: new tab.
- Ctrl+W: close current tab (the last tab closes the window).
- Ctrl+Tab / Ctrl+Shift+Tab: cycle tabs.
- Ctrl+Enter: open the selected folder in a new tab.

Ctrl+Enter opening a new tab is intentional. Ctrl+Plus is not documented as a new-tab shortcut; an exact reproduction is needed if that was the key combination meant.

## Internal fixes

- Update extraction accepts safe forward, backward and mixed ZIP separators while retaining traversal/absolute-path/duplicate/allowlist checks.
- Release ZIP writer emits forward-slash names and validates the finished archive.
- Preview hosts record structured results/stderr, check versions and support a hidden self-test.
- Native backdrop is backend-owned, uses the full client DWM frame and avoids material resets during slider dragging.
- Settings writes coalesce and retry short Windows reader-lock conflicts.

## Still pending from the plan

Quick Look GUI/focus/motion/zoom acceptance and full-resolution image zoom; the full inspector; syntax-highlighted code and rendered/editable Markdown; palette; durable operation undo/progress; batch rename; advanced search and saved searches; remaining sidebar/thumbnail/drag improvements; saved commands; feature-dependent settings/shortcuts pages; split panes, drag-to-split, workspaces, custom/pinned/live groups, column view, statuses, drop shelf and docked documents. Full motion, accessibility and controlled startup/idle/performance acceptance are also outstanding. Optional organize remains unstarted.

## Evidence limits

Before the latest duplicate-button removal: combined debug build, 79 app tests (4 ignored) and strict all-target/all-feature Clippy passed. Native checks covered 13 presentation formats without a host launch, PDF page turns and observed Word fixture text. Concurrent user navigation interrupted further GUI checks; source/API assertions are not proof of every animation frame. See the numbered documents for exact scope and unverified items.

Latest duplicate-button follow-up: build, focused routing test and strict Clippy passed. The direct GUI control check was blocked by desktop focus before input was sent; verify the absence of the extra launch button in the running build.

Enlarge-button restriction: show the inspector action only for supported image types and the document host types PDF, DOCX and XLSX. Folders, presentations, text/code, media, archives and other icon-only/unsupported formats omit it. Legacy DOC/XLS and related formats without a complete preview also omit it. This follows the latest user refinement and keeps the existing approved button styling.

Quick Look foundation (GUI unverified): Space from the file list opens the reader; Space/Esc closes it; context-menu Quick Look opens its target; Ctrl+I toggles the info panel. Up/Down changes files, Left/Right turns pages. Images have Fit/100% of decoded preview pixels and Ctrl+scroll zoom. The latest user request removes the Enlarge text button in favor of one Quick Look eye action. No app launch was performed for this pass.

User feedback confirmed basic Quick Look opening, file switching, PDF page navigation, closing and mouse zoom. Follow-up implementation adds keyboard zoom (Ctrl++/Ctrl+=, Ctrl+-, Ctrl+0 Fit, Ctrl+1 100%), zoom buttons, a 160ms info-panel slide/fade and a tight spreadsheet border. Those new interactions still need GUI acceptance. The former Enlarge button is replaced entirely by the Quick Look eye action on supported preview types.

Panning checkpoint test: open Image.png in Quick Look, zoom in several steps with Ctrl++/Ctrl+=, then hold left click and drag in both axes. Confirm open/closed hand cursors and no movement past image edges. Test Shift+arrows, Fit reset, switching to another file and back, and release outside the image. Also verify the latest Ctrl+I slide, Quick Look eye action and compact Sheet.xlsx border.

Reader/pan checkpoint: Image.png should show a native hand when zoomed past the viewport. Check Shift+Up/Down and drag in both axes, then Reset / Fit. Reader.docx demonstrates the long bounded Word reader; Word.docx demonstrates short content. Sheet.xlsx should appear centered with larger cells; Workbook.xlsx tests the taller table scroller and horizontal overflow. Quick Look eye is now smaller. PDF layout stays unchanged. New GUI acceptance is pending.

Fit/actual-size checkpoint: the sample Image.png should use approximately 118% Fit in the normal review window, depending on its size/DPI. 100% should render at actual preview pixel size. Buttons must show Fit image and 100% with a legible reset icon and clear spacing, and remain uncompressed when the help text runs out of room. Resize the window and check Fit follows it while 100% remains fixed.
