# Changelog

## 1.0.1 - 2026-10-20

- Added a per-user Setup EXE and native uninstaller, with an optional desktop shortcut and saved-data retention by default. The helper is never installed automatically.
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
