# Changelog

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
