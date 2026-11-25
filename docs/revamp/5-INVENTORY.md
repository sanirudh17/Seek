# Phase 5 inventory

Every control, setting, menu entry and shortcut, grouped by screen, taken from
the source at the start of Phase 5 (commit d53095f). Phase 5 changes
presentation only: each line below must still exist and work afterwards. The
"After" column was checked at the end of Phase 5 against the running app and
the source; "same" means unchanged in place and behaviour.

## Window and top bar

| Control | Behaviour | After |
|---|---|---|
| Toggle sidebar | Shows/hides the sidebar (spring) | same |
| Back / Forward | History per tab | same |
| Search field | Name search, filter chips (ext:, type:, kind:, in:, size:, mtime:, path:, limit:), folder address, suggestions menu | same |
| Content mode chip | Exit content search (X) | same |
| View switcher | List (Ctrl+1), Icons (Ctrl+2), Gallery (Ctrl+3), Columns (Ctrl+4) | same |
| Show hidden and system files | Toggle, saved | same |
| Sort results | Menu: Name A–Z, Name Z–A, Newest first, Oldest first, Size: largest first, Type | same |
| Info panel | Toggle (Ctrl+I) | same |
| Settings | Opens settings | same |
| File actions (…) | Menu: All items, Largest files, Largest folders, Search file contents, Type ▸, Save current search, Undo last operation, Redo last operation, Stop file operations, Operation history, Keyboard shortcuts, All commands… | same |
| Window controls | Minimize, Maximize/Restore, Close (native caption behaviour) | same |

## Sidebar

| Control | Behaviour | After |
|---|---|---|
| Favorites heading | Drop target (adds folders); menu: New group | same |
| All locations | Opens All locations (Alt+0) | same |
| Favorite items (Downloads, Desktop, Documents, Pictures, Videos, Music, user folders) | Click opens; Alt+1–9; drag to reorder/split; drop files moves into; menu: file actions for the folder, New group, Remove from sidebar (user items) | same |
| Saved searches | Click runs the query | same |
| Groups | Chevron collapse/expand (remembered); inline name editor; drag to reorder; whole group accepts drops; menu: Rename group, Delete group, Add current search, New group | moved, see below |
| Locations | Drive/root rows with free space; click opens; drag to split; drop files moves into; file actions menu | same |
| Jump… (Ctrl+K) | Opens the command palette | moved, see below |
| Footer | Indexed count, catch-up status; update indicator (popover: notes, Download, ring, Restart) | same |

## Tabs, panes and related folders

| Control | Behaviour | After |
|---|---|---|
| Tab | Click switches; drag reorders or moves to another pane; drag onto a pane splits; close button; menu: Show/Hide related folders | same |
| New tab / New tab in this pane | Ctrl+T | same |
| Zoom pane | Ctrl+Shift+Enter; fill pane area / show all | same |
| Pane dividers | Drag; double-click shares equally | same |
| Split right / down | Ctrl+\ / Ctrl+Shift+\ (palette and folder menu "Open folder in new pane") | same |
| Related folders row | Chips open in the same tab; "+" menu: Add related folder…, Add current folder, Hide related folders; chip menu: Rename label, Remove; drop folders or tabs to add | same |

## File list (List, Icons, Gallery, Columns)

| Control | Behaviour | After |
|---|---|---|
| List columns | Name, Date Modified, Size, Type, Folder (search), Matching line (content); sortable headers; resizable | same |
| Rows | Selection (click, Shift, Ctrl), caret, disclosure for folders, inline drag, drop into folders, cloud badge | same |
| Icons/Gallery | Tiles with name and size; Gallery large preview and strip | same |
| Columns | Ancestor columns, preview column, resizable widths, Right/Left/type-ahead | same |
| Row menu | Open folder in new tab, Open folder in new pane, Open, Quick Look, Open in document pane, Open with…, Reveal in Explorer, Copy, Cut, Paste, Copy path, Rename / Batch rename…, Suggest names…, Move to Recycle Bin, New folder, Properties, More actions in Jump… | same |
| Empty-space menu | New Folder (Ctrl+Shift+N), New File, Organize Folder, Properties, Paste (Ctrl+V), As List (Ctrl+1), As Grid (Ctrl+2), As Columns (Ctrl+4), Show Hidden Files, Sort by Name / Date Modified / Size / Kind | same |
| Empty states | Folder has no visible items, No matching files, Indexing…, Press Enter to open this folder | same |

## Status bar

| Control | Behaviour | After |
|---|---|---|
| Path breadcrumb | Clickable segments | same |
| Counts | Items, selected | same |
| Status message | Folder contents, errors, progress | moved, see below |
| ≈ Indexed sizes (Largest) | Tooltip explanation | same |
| Inaccessible locations | Count with tooltip | same |

## Info panel

| Control | Behaviour | After |
|---|---|---|
| Preview | Image/document/text/code/Markdown previews | same |
| Quick Look (eye) | Opens Quick Look (Space) | same |
| Open in document pane | Panel-right button | same |
| Details / Dates / Storage | Size, size on disk, created, modified, last opened, path, volume | same |
| Selection totals | Counts, kinds, bytes, Recalculate | same |
| Actions | Open, Reveal, Copy path, Share | same |
| Width | Draggable, saved | same |

## Quick Look, readers and document pane

| Control | Behaviour | After |
|---|---|---|
| Image controls | Zoom −/+, Fit, 100%, Reset / Fit, percentage, pan | same |
| PDF/Word/Excel | Page turns, readers | same |
| Text/Markdown/code | Read/Source, Edit, Save split button (Save a Copy), More: Undo saved change, Discard changes, Finish editing; Find (Ctrl+F) | same |
| Document pane header | Icon, name, kind, size, modified, Reload, Close | same |

## Sheets and dialogs

| Sheet | Controls | After |
|---|---|---|
| Rename / New folder / New file / New folder with selection | Name field, Cancel, Save | same |
| Batch rename | Replace text, Number names, Suggest names; find/replace or base/start fields; preview rows; Skip conflicting items; Cancel, Apply renames | moved, see below |
| Suggest names | Editable field per file with old name; reason when kept; Cancel, Apply renames | same |
| Delete permanently | Cancel, Delete permanently | same |
| Organize | Switch per group, rule, count, files; files that stay; Cancel, Organize N | same |
| Operation history | Rows with outcomes; Undo, Redo, Done; Stop, Review, Archive history | same |
| Command palette (Jump) | Search, grouped commands, locations, settings pages, saved/recent searches | moved, see below |
| Saved command run | Output, exit status, close | same |
| Unreadable files (content) | List, close | same |
| Unsaved changes | Save, Discard, Cancel | same |

## Toasts and notices

| Item | After |
|---|---|
| Organized N files · Undo | same |
| Saved <name> (Save a Copy) | same |
| Status-bar messages | same |

## Settings

| Page | Controls | After |
|---|---|---|
| Appearance | Theme (System/Light/Dark), accent swatches, custom accent + Apply | same |
| Glass | Glass toggle, Glass in main pane, opacity sliders | same |
| Background | Choose PNG…, Remove background, Soften background | same |
| Files and search | Default view options, Show hidden (in toolbar), Content search locations + Save content locations, Remember document text + Clear cache | same |
| Text editing | Auto-save text files, Keep text file backups | same |
| Previews | Automatic information panel previews, Image and video thumbnails | same |
| Keyboard shortcuts | The generated reference (docs/revamp/PHASE2-SHORTCUTS.md) | same |
| Saved commands | Program, arguments, selection rules, add/edit/remove, run | same |
| Locations | Indexed folders + Save locations, Excluded folders, Rebuild current index | same |
| Helper | Faster indexing (helper status and install guidance) | same |
| Updates | Check for updates on startup, Check now, Download and restart, version and status | moved, see below |
| Chrome | Search settings, page navigation, Done | moved, see below |

## Shortcuts

The 38 entries in `app/src/shortcuts.rs`, rendered in docs/revamp/PHASE2-SHORTCUTS.md and Settings > Keyboard shortcuts (Ctrl+Shift+N added in Phase 4).

## What moved (Phase 5)

Nothing was removed. These items changed place or form:

| Item | Before | After |
|---|---|---|
| Settings row descriptions | A sentence under each setting | Tooltip on the setting's label |
| Settings "Done" | Header bar with a "Settings" title | On the page title line; header bar removed |
| Settings search | "Search settings" label above the field | Field only ("Find a setting") |
| Switch state text | "On"/"Off" beside each switch | Removed; the switch itself shows state (on = text colour); accessible names unchanged |
| Updates version and status | "Installed version" line, default status sentence, buttons below | One "Version" row: status (when any), 1.0.2, Check now, Download/Restart |
| Settings intro paragraphs (Previews, Keyboard shortcuts, Saved commands, Indexed locations) | Paragraphs | Removed; their facts are in row tooltips |
| Jump row "Ctrl+K" badge | Beside Jump in the sidebar | Tooltip, palette and keyboard reference |
| Empty group sentence | "Drag folders or searches here" | Blank drop row appears only while dragging |
| Group naming label and hint | "Group name" label and "Enter to save · Esc…" | Placeholder "Group name"; Enter/Esc unchanged |
| Status bar "Folder contents" / "Up to date" | Always shown | Hidden; errors, progress and results still shown |
| Jump title and footer | "Jump and commands" title, key-hint footer | Placeholder "Jump to a folder or run a command" |
| Batch rename field labels | Labels above each field | Placeholders (Text to find / Replacement text, Base name / Start at) |
