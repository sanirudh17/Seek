# Phase-two keyboard reference

Generated from `app/src/shortcuts.rs`, the same registry used by Settings > Keyboard shortcuts. Run `python scripts/generate-shortcuts.py --check` to check for drift.

These shortcuts act within Seek. The focused reader, text field or sheet owns its editing/navigation keys.

## Browse

| Shortcut | Action |
| --- | --- |
| Ctrl+L | Search or go to folder |
| Enter | Open |
| Ctrl+Enter | Open folder in new tab |
| Ctrl+T / Ctrl+W | New tab / close tab |
| Ctrl+Tab / Ctrl+Shift+Tab | Next / previous tab |
| Alt+0 / Alt+1–9 | All locations / sidebar items |
| Ctrl+1 / Ctrl+2 / Ctrl+3 / Ctrl+4 | List / icons / gallery / columns |
| Ctrl+B | Toggle sidebar |
| Ctrl+I | Toggle info panel |

## Panes

| Shortcut | Action |
| --- | --- |
| Ctrl+\\ / Ctrl+Shift+\\ | Split right / down |
| Ctrl+Shift+W | Close pane |
| F6 / Shift+F6 | Next / previous pane |
| Ctrl+Shift+Enter | Zoom pane |
| Alt+F5 / Alt+F6 | Copy / move to other pane |

## Files

| Shortcut | Action |
| --- | --- |
| Arrow keys | Move; collapse / expand folders |
| Right / Left (Columns) | Into folder / up (columns) |
| Shift+Arrow / Shift+click | Extend selection |
| Ctrl+Arrow / Ctrl+click | Move without selecting / toggle item |
| Ctrl+A | Select all |
| Ctrl+C / Ctrl+X / Ctrl+V | Copy / cut / paste |
| Ctrl+Shift+C | Copy path |
| Ctrl+Shift+N | New folder |
| F2 | Rename |
| Delete / Shift+Delete | Recycle / delete permanently |
| Ctrl+Z / Ctrl+Shift+Z | Undo / redo |

## Quick Look

| Shortcut | Action |
| --- | --- |
| Space | Quick Look |
| Escape | Close |
| Up / Down | Previous / next file |
| Left / Right | Previous / next page |
| Ctrl++ / Ctrl+- | Zoom in / out |
| Ctrl+0 / Ctrl+1 | Fit / actual size |
| Shift+Arrow / drag image | Pan |

## Text and Markdown

| Shortcut | Action |
| --- | --- |
| Ctrl+F | Find |
| Ctrl+S | Save |
| Ctrl+Z | Undo typing |

## Commands and sheets

| Shortcut | Action |
| --- | --- |
| Ctrl+K | Jump |
| F1 | Keyboard shortcuts |
| Tab / Shift+Tab | Next / previous field |
| Escape | Cancel or close |
