<p align="center">
  <img src="app/assets/seek-icon-source.png" width="112" height="112" alt="Seek icon" />
</p>

<h1 align="center">Seek</h1>

<p align="center"><strong>Fast, local file search for Windows.</strong><br/>
Find a file, inspect it, and get back to work.</p>

<p align="center">
  <a href="https://github.com/sanirudh17/Seek/releases/latest"><img src="https://img.shields.io/github/v/release/sanirudh17/Seek?label=download" alt="Latest release" /></a>
  <a href="LICENSE"><img src="https://img.shields.io/badge/license-MIT-blue" alt="MIT license" /></a>
  <img src="https://img.shields.io/badge/platform-Windows_x64-0078d6" alt="Windows x64" />
</p>

## Overview

Seek searches filenames using a local index and renders a compact native GPUI interface. It runs as your ordinary Windows account. There is no account sign-in, AI model, telemetry service, tray, global hotkey, app autostart or resident updater process. Closing the last tab quits Seek. The optional indexing helper is a separate, explicitly installed service.

**1.0.2** includes tabs, folder browsing, file actions, filename/type filters, on-demand content search, and previews in the resizable information panel. The default sidebar uses the current account's Windows Known Folders, including redirected OneDrive locations. Your index and settings are never part of the download. See [CHANGELOG](CHANGELOG.md).

## Contents

- [Download and install](#download-and-install)
- [Everyday use](#everyday-use)
- [Keyboard and mouse commands](#keyboard-and-mouse-commands)
- [Search and previews](#search-and-previews)
- [Settings and saved data](#settings-and-saved-data)
- [Optional helper](#optional-helper)
- [Update and uninstall](#update-and-uninstall)
- [Building from source](#building-from-source)
- [Project structure](#project-structure)
- [Known limits and troubleshooting](#known-limits-and-troubleshooting)
- [License and credits](#license-and-credits)

## Download and install

Download **[Seek-1.0.2-Setup-x64.exe](https://github.com/sanirudh17/Seek/releases/download/v1.0.2/Seek-1.0.2-Setup-x64.exe)** from [Releases](https://github.com/sanirudh17/Seek/releases/latest). Requires x64 Windows; tested on Windows 11. Windows 10 and physical HDR/mixed-DPI setups remain unverified.

1. Double-click the installer normally, without **Run as administrator**.
2. Accept the license. If Seek is already installed, close it and approve replacing its program files; saved data is retained.
3. Choose whether to add a desktop shortcut. A Start menu shortcut is always created.
4. Open **Seek** from Start.

The program installs in `%LOCALAPPDATA%\Programs\Seek` and registers in **Settings > Apps > Installed apps** for this account only, with a native uninstaller. It does not install the helper service. Existing program files and shortcuts require replacement approval; saved data is retained. The package is unsigned, so Windows may display reputation warnings, and managed computers may restrict the bundled PowerShell install scripts.

The separate `Seek-1.0.2-windows-x64.zip` asset supports the existing in-app updater and portable use. The normal download is the **Setup EXE**. Portable saved data still goes under your current account's `%LOCALAPPDATA%\Seek`, not beside the executable.

To verify the download against the separately supplied `SHA256SUMS.txt`:

```powershell
Get-FileHash .\Seek-1.0.2-Setup-x64.exe -Algorithm SHA256
```

## Everyday use

Choose Downloads, Desktop, Documents, Pictures, Videos, Music or a drive in the sidebar. On a first launch without a saved index, All locations shows labelled recent files from priority folders while the full drive scan continues. These early results are provisional; filename searches become complete when indexing finishes. Type a filename or a fragment in the search field. Use the **Name contains** suggestion for literal matching, or choose a matching **Type** suggestion. Remove a filter using its chip's cross; the chip dropdown changes its type.

The view controls select List, Icons or Gallery. Click a column heading to sort. Folders can expand inline in List. The toolbar actions menu provides **All items**, **Largest files** and **Largest folders**; select **All items** to return from a largest-items view. Largest views show up to 200 indexed candidates; indexed size values can be approximate, with exact metadata loaded for selected/visible items where available.

Right-click a selected item, or use the toolbar actions menu, for Open, Open with, Reveal in Explorer, Copy/Cut/Paste, path/name copying, Rename, New folder, Recycle Bin and Properties. Menus are GPUI menus; Open with and Properties launch Windows dialogs. Dragging inside Seek moves files into the target folder, using Windows conflict dialogs. External drag-out uses Copy; clipboard file copy/paste is the fallback for targets that do not accept a drag.

## Keyboard and mouse commands

Shortcuts act inside the focused Seek window, not globally. File operations require the results list/grid to have focus. Search-field shortcuts continue to edit text normally. Settings and confirmation dialogs capture their own keys.

| Command | What it does |
| --- | --- |
| **Ctrl+T** or the tab-bar **+** | Duplicate the current folder/search into a new tab. |
| **Ctrl+Enter** | Open the selected folder in a new tab; otherwise duplicate the current search. |
| **Ctrl+Tab** | Switch to the next tab, wrapping at the end. |
| **Ctrl+Shift+Tab** | Switch to the previous tab. |
| **Ctrl+W** or a tab's **x** | Close the active/chosen tab. Closing the last tab quits the app. |
| **Ctrl+L** | Focus the search field. |
| **Ctrl+1** | Switch to List view. |
| **Ctrl+2** | Switch to Icons view. |
| **Ctrl+3** | Switch to Gallery view. Content results use List. |
| **Up / Down** | Move file selection. Down from an open search-suggestions list enters that menu. |
| **Right / Left** | Expand/collapse the selected folder inline in List. |
| **Enter** | Open the selected item; in search suggestions, apply **Name contains**. |
| **Space** | Toggle the selected item's information panel when the file list has focus. |
| **Ctrl+A** | Select all displayed result rows; in the search field, select its text. |
| **Ctrl+C** | Copy selected files to the Windows clipboard, or copy selected search text. |
| **Ctrl+X** | Cut selected files for a move, or cut selected search text. |
| **Ctrl+V** | Paste files into the current folder, or paste text into the search field. |
| **Ctrl+Shift+C** | Copy the selected item's full path. |
| **F2** | Rename the selected file/folder. |
| **Delete** | Move the selected items to the Recycle Bin. |
| **Shift+Delete** | Open permanent-delete confirmation. **Delete permanently** has default focus; Enter confirms. Left/Right or Tab switches to Cancel; Escape cancels. |
| **F5** | Rerun the current content search. |
| **Escape** | Dismiss suggestions/Settings or cancel the current dialog; otherwise clear the active search state. |
| **Tab / Shift+Tab** | Move keyboard focus between dialog controls; Escape cancels permanent deletion. |
| **Ctrl+click / Shift+click** | Toggle a selected row / extend a range from its original anchor. |
| **Shift+arrows / Ctrl+arrows** | Extend a selection / move the caret without changing selected items. In Icons/Gallery, Up/Down moves by a grid row. |
| **Left / Right on buttons** | Move between buttons; in rename/new-folder input fields these keys still edit text. |
| **Double-click** | Open a file or folder. |
| **Drag the info panel's left border** | Resize the panel; its width is remembered. |

Tabs are **not automatic**: typing a different query changes the current tab and its title. Each explicit tab retains its own search, folder scope, view/sort, selection and scroll position for the current session. Tabs are not restored across app restarts. The native folder/sidebar menu also offers **Open folder in new tab**, and the toolbar menu offers **Open search in new tab**.

## Search and previews

**Filename search** uses the local file index. Examples: `invoice`, `photo ext:jpg`, and `report ext:pdf`. Type suggestions cover Folder, Document, Image, Movie, Audio, Code and Archive, plus matching extensions. Hidden-item visibility changes presentation; it does not remove those entries from the index.

**Content search** starts from the **Content contains** suggestion. By default it searches Downloads and Documents, configurable in Settings. It supports text, code, Markdown, DOCX and PDF text via an installed Windows PDF filter. Windows Search can supply **Quick results (may be incomplete)**; the direct scan continues, deduplicates and remains authoritative. Wait for **Complete** and the searched-file counts before treating the result as finished. Editing the query cancels the previous scan. Failed PDF/Word extraction is retried once after the other files, and persistent failures are listed explicitly.

Only configured content folders are read. Secret/key paths and files, offline/unavailable items, unsupported formats and files over the **4 MiB content-search cap** are skipped and reported. These limits also apply when saved document text is enabled. PDF accuracy depends on a usable text layer/filter; scanned-image PDFs are not OCRed.

The information panel previews images/videos, PDF pages with page controls, the first XLSX sheet as a data table, and a large DOCX shell thumbnail or beginning-of-text fallback. Office is not required. XLSX shows **data, not formatting**, with cached formula values and visible row/column/cell limits. Legacy `.doc` and `.xls` formats are not supported as document previews. Document previews have a **32 MiB input cap and four-second worker limit**; broken, encrypted, unsupported and over-limit documents can show **Preview unavailable**. Previews load only for the current selection; list rows keep icons.

### Folder addresses and enlarged previews

Type `%TEMP%`, `%USERPROFILE%`, `%APPDATA%`, a full drive/UNC folder path or `~`, then press **Enter** or select **Open folder**. This works in Name and Content mode and navigates directly instead of scanning document text. The folder must exist and be accessible to your account.

Select a document and choose **Enlarge preview**. **Up/Down** selects the previous/next file, **Left/Right** changes page or slide, and **Escape** returns to the information panel. In the panel, use the page arrows. All locations includes files and folders in **500-item pages**, with the total and Previous/Next controls at the bottom.

Modern PowerPoint (`.pptx`, `.pptm`, `.ppsx`) displays saved first-slide artwork where available and ordered slide text. Other slides show text, not a reproduction of their graphical layouts. Legacy `.ppt` can use a Windows thumbnail provider but has no slide-text parser. PDF raster frames are cached only in memory (four entries / 24 MiB); changing a file or closing the panel invalidates them. The selected-PDF host exits after two idle seconds and has the same four-second request limit. Enlarged PDFs use a larger raster than panel previews.
## Settings and saved data

| Setting | Purpose |
| --- | --- |
| Theme, acrylic glass and accent | Follow Windows or choose Light/Dark; control glass and restrained selection accents. |
| Hidden/system files | Change their visibility while retaining indexed metadata. |
| Default view | Choose the view applied when opening Seek. |
| Indexed/excluded locations | Configure the index; save location changes and follow the displayed restart/rebuild guidance. Blank indexed locations means available drives. |
| Content search locations | Restrict on-demand scans to the folders you choose. |
| Remember document text for faster content search | Default on. Reuse unchanged PDF/Word extraction; **128 MiB** LRU cap, no age expiry. |
| Clear cache | Remove saved document text. Disabling remembering also clears it. |
| Helper | Explicitly install/uninstall the optional elevated NTFS service. |
| Updates | Default-on check once per app launch; Check now, installed version/status, and Download and restart when a newer stable release exists. |

Default app data is `%LOCALAPPDATA%\Seek`: the filename index, `settings.json`, window state, opened-item records and document-text cache. Saved document text is compressed, protected with current-user DPAPI and restricted account ACLs. It is dropped when the file changes or is removed, leaves content scope, or is evicted by the cap. It is never shipped in a release package. Each Windows account gets its own data.

## Optional helper

Seek works without the helper, using ordinary-user directory listings and change tracking. **Settings > Helper > Install helper** requests administrator approval only for installing the opt-in service. The app itself remains unelevated. The helper uses NTFS metadata and journal replay for quicker catch-up; unsupported volumes fall back to the ordinary walker. The helper cannot grant access to protected files that your account cannot read.

The explicitly installed helper can remain running after the app closes and starts as a Windows service. Remove it with **Settings > Helper > Uninstall helper** before uninstalling Seek; this requires administrator approval again. The ordinary-user app installer never creates a service.

If the app is already removed, this PowerShell command locates the separately installed helper for your account and requests UAC to remove it:

```powershell
$sid = [Security.Principal.WindowsIdentity]::GetCurrent().User.Value
$helper = Join-Path $env:ProgramFiles ("Seek Helper\{0}\seek-helper.exe" -f $sid)
Start-Process -FilePath $helper -Verb RunAs -ArgumentList @('uninstall', '--sid', $sid) -Wait
```

It targets the current account's helper, not another user's service. Do not delete its protected Program Files directory manually while the service is installed.

## Update and uninstall

**Settings > Updates** controls automatic startup checks (on by default). Seek checks the public GitHub release once per app launch, after window creation, with no polling or resident updater. **Check now** works on demand even when startup checks are off. No filenames, queries, paths or document text are sent; GitHub receives the normal network request and app-version user agent.

When a newer stable Windows release exists, choose **Download and restart**. This explicitly approves replacing program files and closing all session tabs. Seek downloads via HTTPS, validates the SHA-256 checksum and fixed package contents, prepares a backup, closes, applies the update and restarts without elevation. Saved index/settings/document text and the separately opted-in helper service are preserved. Failed preparation leaves Seek open; apply failures attempt to restore previous files and keep a diagnostic result in the temporary update folder. The unsigned package checksum detects corruption; it is not a publisher signature, so update trust depends on the GitHub repository.

For a manual update, close Seek, download the newer Setup EXE and run it. Approve replacement of program files; saved data stays in your profile.

Uninstall through **Windows Settings > Apps > Installed apps > Seek**, or run **Uninstall.exe** in `%LOCALAPPDATA%\Programs\Seek`. Confirm removal of program files and shortcuts. A separate **default-No** prompt asks whether to delete the saved index, settings and document-text cache. Choose No to keep them. Unexpected extra files in the program folder are preserved. The native uninstaller retains an already opted-in helper and says so; remove that service in Seek Settings first if desired. The legacy Uninstall.cmd remains available for older installations.

For an uninstalled portable copy, remove its extracted program folder yourself. Its profile data is separate; remove `%LOCALAPPDATA%\Seek` only if you intend to discard all saved data. Recycle personal files through Explorer rather than deleting a similarly named document folder.

## Building from source

Creating the Setup EXE also requires [NSIS 3](https://nsis.sourceforge.io/) (`makensis.exe`); the installer/uninstaller use zlib compression.

Use Rust **1.96.0 x86_64-pc-windows-msvc**, Visual Studio 2022 Build Tools with **Desktop development with C++**, and Windows SDK **10.0.26100.0**, including `rc.exe` and `fxc.exe`. The toolchain and framework revisions are pinned; do not update them merely to build a release.

```powershell
# Clone the public source and select the release.
git clone https://github.com/sanirudh17/Seek.git
cd Seek
git checkout v1.0.2

# Install the pinned compiler and developer checks.
rustup toolchain install 1.96.0-x86_64-pc-windows-msvc --component rustfmt --component clippy

# Build the app, helper and developer CLI; check correctness and lints.
cargo build --workspace --locked
cargo test --workspace --locked
cargo clippy --workspace --all-targets --locked -- -D warnings

# Build/package the optimized app and optional helper from a clean checkout.
# Static CRT, stripped release, path remapping, license files and SHA256SUMS included.
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/build-release.ps1
```

The package appears under `target/release-package/`. No signing, helper installation or GitHub upload happens during a build. The `-SkipBuild` option only packages existing release binaries after checking version metadata; use it only after a verified clean build.

A development run with its own disposable index and restricted scope:

```powershell
# --root limits the filename index; --data keeps tests away from your saved profile.
cargo run -p seek -- --walker --root "$env:TEMP" --data .bench-data/development-profile

# Independent directory-listing correctness check using the developer CLI.
cargo run -p seek-cli -- verify --root "$env:TEMP" --data .bench-data/development-profile
```

Content scope is a separate Settings choice: `--root` alone is not a content-search privacy boundary. Owned fixture scripts configure content scope explicitly. Native desktop test scripts optionally use Orca; the installed application does not depend on it. Historical measurements and their scope/limitations are in [PROGRESS.md](PROGRESS.md) and [docs/STAGE_C_POLISH.md](docs/STAGE_C_POLISH.md).

## Project structure

```text
app/                    Native GPUI app, Windows resources and icon
crates/engine/          Adapted fsearch-crossplatform index and Windows drivers
crates/seek-helper/     Opt-in service and explicit admin install/uninstall commands
crates/seek-cli/        Developer verifier and benchmark tools
vendor/gpui-component/  Pinned GPUI Kit source and documented local patches
vendor/gpui-windows/    Pinned Windows backend and documented local patches
packaging/              Per-user install/uninstall scripts and supplemental notices
scripts/                Build, validation and diagnostic tools
docs/                   Evidence, known limits and development reports
THIRD-PARTY-NOTICES.txt Full redistribution notices and license texts
```

## Known limits and troubleshooting

- First indexing and no-helper whole-drive catch-up can take time. The status bar reports progress; cached results appear first. An indexing folder must not be confused with a known-empty folder.
- Protected folders can remain inaccessible to your ordinary account. Counts identify recorded inaccessible paths; there is not yet a built-in path browser for them.
- Content results are complete only within the configured scope, supported formats and stated caps, with skips/unreadable files disclosed. Windows Search alone is not complete.
- External file drag works with tested Explorer/browser targets; individual chat apps and all upload interfaces are not verified. Use Copy/Paste when necessary.
- High contrast/transparency settings, Windows builds, GPU drivers, HDR and monitor scaling can affect glass. Physical HDR/mixed-DPI and other-machine verification remain coverage limits.
- If the app will not start, check that you extracted the whole package and are using x64 Windows. Managed PowerShell policy or antivirus/reputation controls can block the install script. Do not run Seek elevated to work around a policy.
- No second enabled standard account was available on the release machine. To test independently, create one in **Windows Settings > Accounts > Other users > Add account > Add a user without a Microsoft account**, keep its account type **Standard**, then sign into it. Account creation requires an administrator. Install there, check its own sidebar folders, then uninstall once keeping data and once deleting disposable test data. Do not copy another account's Seek data into it.

## License and credits

Seek's own code is [MIT](LICENSE). Redistribution includes [THIRD-PARTY-NOTICES.txt](THIRD-PARTY-NOTICES.txt), with complete license texts and required attributions for shipped components and conservative build dependencies. The filename engine derives from [fsearch-crossplatform](https://github.com/threehymns/fsearch-crossplatform) (Noah Dunnagan / threehymns, MIT); UI uses [GPUI](https://github.com/zed-industries/zed) and [GPUI Kit](https://github.com/longbridge/gpui-component) (Apache-2.0), plus Lucide artwork. Existing licenses and patch/provenance records remain beside vendored sources. Dependency licenses govern their respective components.
