<p align="center">
  <img src="app/assets/seek-icon-source.png" width="112" height="112" alt="Seek icon" />
</p>

<h1 align="center">Seek</h1>

<p align="center"><strong>A fast, local file manager and search for Windows.</strong><br/>
Find a file, look inside it, tidy your folders, and get back to work.</p>

<p align="center">
  <a href="https://github.com/sanirudh17/Seek/releases/latest"><img src="https://img.shields.io/github/v/release/sanirudh17/Seek?label=download" alt="Latest release" /></a>
  <a href="LICENSE"><img src="https://img.shields.io/badge/license-MIT-blue" alt="MIT license" /></a>
  <img src="https://img.shields.io/badge/platform-Windows_x64-0078d6" alt="Windows x64" />
</p>

## Overview

Seek is a native Windows file manager built around a local filename index. It browses folders in split panes and tabs, searches names and contents, previews documents and pictures, renames and organizes files, and shows where your disk space goes. Everything runs on your PC under your ordinary Windows account: there is no sign-in, cloud service, telemetry, tray icon or background updater. An optional helper service speeds up indexing on NTFS drives and is only installed when you ask for it.

Release notes for each version are on the [Releases](https://github.com/sanirudh17/Seek/releases) page.

## Contents

- [Download and install](#download-and-install)
- [Features](#features)
- [Keyboard shortcuts](#keyboard-shortcuts)
- [Settings and saved data](#settings-and-saved-data)
- [Optional helper](#optional-helper)
- [Update and uninstall](#update-and-uninstall)
- [Building from source](#building-from-source)
- [Project structure](#project-structure)
- [Known limits](#known-limits)
- [License and credits](#license-and-credits)

## Download and install

Download the **Setup EXE** from the [latest release](https://github.com/sanirudh17/Seek/releases/latest). Seek needs x64 Windows and is tested on Windows 11.

1. Run the installer normally, without **Run as administrator**.
2. Accept the license. If Seek is already installed, close it and approve replacing its program files; your saved data is kept.
3. Choose whether to add a desktop shortcut. A Start menu shortcut is always created.
4. Open **Seek** from Start.

Seek installs in `%LOCALAPPDATA%\Programs\Seek` for your account only and appears in **Settings > Apps > Installed apps** with its own uninstaller. The installer never installs the helper service. The package is not code-signed, so Windows may show a reputation warning.

Each release also has a portable ZIP (used by the in-app updater) and a `SHA256SUMS.txt` file. To check a download:

```powershell
Get-FileHash .\Seek-<version>-Setup-x64.exe -Algorithm SHA256
```

## Features

### Browse

- **Split panes and tabs.** Split right or down, drag dividers, zoom one pane, and move or copy files between panes. Each pane has its own tabs, and clicking a pane focuses it.
- **Four views.** List (with inline folder expansion), Icons (size, spacing and label position are adjustable), Gallery (with an optional large preview) and Columns.
- **Live folders.** A folder on screen updates the moment something changes on disk: a finished download, a file saved by another app, or a change made by a coding agent. New items appear at the top.
- **Sidebar.** Your Windows folders (including OneDrive-redirected ones), drives, saved searches and your own groups, with icons in your accent colour. Drag folders onto it to pin them.
- **Addresses.** Type a path, `%TEMP%`, `~` or `cd <folder>` in the search field and press Enter to go straight there.
- **Information panel.** Details, dates, size on disk, ownership and a preview of the selection, in a resizable panel.
- **Reopen as you left it.** Panes, tabs and folders come back when you start Seek (can be turned off).

### Search

- **Filename search** over the local index, with type and extension filters (`photo ext:jpg`, `report ext:pdf`) and suggestions as you type.
- **Content search** inside text, code, Markdown, Word documents and PDFs in the folders you choose. Results stream in while the scan runs, and skipped or unreadable files are listed.
- **Largest files and folders** anywhere in the index.
- **Saved and recent searches**, shown in the sidebar and in Jump.

### Look inside files

- **Quick Look** (Space) for pictures, PDFs, Word documents and Excel sheets: zoom, pan, actual size, and page and file navigation from the keyboard.
- **Previews** in the information panel and Gallery, loaded only for what you select.
- **Document panes** open text, code and Markdown beside your files for reading or editing, with find, optional auto-save and optional backups.
- **Image tools** for common picture tasks from the information panel.

### Work with files

- **Everyday actions:** open, open with, reveal, copy, cut, paste, rename, new folder, Recycle Bin, permanent delete and properties, from menus, the keyboard or drag and drop.
- **Undo and redo** for file operations, with an operation history that can recover from interrupted work.
- **Multi-select** with Ctrl and Shift; consecutive selected rows show as one block.

### Rename and organize

- **Batch rename** with live preview: replace text, number names, or build a list of steps (replace, add text, change case and more) with variables and saved presets. Conflicts are shown before anything changes.
- **Suggest names** reads names from what files say about themselves: a document's title or first heading, a slide deck's first title, an invoice's number and date, music tags, or a photo's date taken. The rules are fixed, so the same file always gets the same suggestion. Only names that say nothing (scan0001, untitled, Document1, IMG_1234, random IDs) are replaced automatically; a name you chose is only ever offered an alternative.
- **Organize a folder** by type, by date taken (Year and Month folders) or by name (camera sequences and versions of one document). You review the groups first, and one Undo puts everything back.

### Storage

- **Overview** of any drive or folder, picked from a list of all your drives: space used and free, a breakdown by kind, and the largest folders with drill-down.
- **Cleanup suggestions** for caches apps rebuild on their own (developer tools, browsers, editors, chat apps, graphics drivers), old temporary files, crash dumps, app logs, unfinished downloads, old installers and build folders in long-untouched projects. Sign-in tokens, locally installed packages and tracked source folders are never offered. Tick the groups you want and clean them in one go, with live progress and a Stop button. Caches, temporary files and build folders are deleted in place, since they rebuild on their own; downloaded AI models, unfinished downloads and old installers go to the Recycle Bin and can be undone.
- **Find duplicates** confirms identical files by their full contents and checks each file again just before moving it. Hard links, cloud placeholders and system files are left out, and identical folders are found too.
- **Similar images** finds resized, recompressed or lightly edited copies of a picture at three levels, in one folder or everywhere. Pictures are compared upright and by colour, and shown side by side; nothing is selected for you, and one picture of every group always stays.

### Jump and commands

- **Jump** (Ctrl+K) goes to any folder, recent file, saved search, setting, view or command, including Storage, Find duplicates and Find similar images.
- **Saved commands** run a program of your choice on the selection, with explicit arguments. Saving a command never runs it.
- **Worktrees** lists Git repositories and their worktrees with their status.

### Coding agents

Seek is an MCP server (`seek.exe --mcp`), so coding agents such as Claude Code, Codex and others can search, read and tidy files through it. Agents can only change files inside the folders you allow in **Settings > Agents**, changes go through the same undo history as your own, and organizing needs a reviewed plan first. The page shows which agents are connected and what they did. See [docs/MCP.md](docs/MCP.md).

### Windows integration

- **Default file manager** (optional): folders and drives you open from anywhere, and Win+E, open in Seek. Turning it off gives everything back to File Explorer.
- **Explorer menu:** "Open in Seek" on folders and drives, and "Show in Seek" on files.
- **Open with:** Seek is listed when Windows asks which app to use.

### Appearance

Light, dark or system theme; Windows glass (Mica or Acrylic) with adjustable tint and intensity, or a picture background; an accent colour used sparingly across the window.

## Keyboard shortcuts

Shortcuts work inside Seek's window, not system-wide. Press **F1** in Seek for the full list.

| Keys | Action |
| --- | --- |
| Ctrl+L | Search or go to a folder |
| Ctrl+K | Jump |
| Ctrl+T / Ctrl+W | New tab / close tab |
| Ctrl+Tab / Ctrl+Shift+Tab | Next / previous tab |
| Ctrl+Enter | Open folder in a new tab |
| Alt+0 / Alt+1-9 | All locations / sidebar items |
| Ctrl+1 / 2 / 3 / 4 | List / Icons / Gallery / Columns |
| Ctrl+J | Icon view options |
| Ctrl+B | Toggle sidebar |
| Ctrl+I | Toggle information panel |
| Ctrl+, | Settings |
| Ctrl+\ / Ctrl+Shift+\ | Split right / down |
| Ctrl+Shift+W | Close pane |
| F6 / Shift+F6 | Next / previous pane |
| Ctrl+Shift+Enter | Zoom pane |
| Alt+F5 / Alt+F6 | Copy / move to the other pane |
| Ctrl+A | Select all |
| Ctrl+C / Ctrl+X / Ctrl+V | Copy / cut / paste |
| Ctrl+Shift+O | Show in folder |
| Ctrl+Shift+C | Copy path |
| Ctrl+Shift+N | New folder |
| F2 | Rename |
| Delete / Shift+Delete | Recycle / delete permanently |
| Ctrl+Z / Ctrl+Shift+Z | Undo / redo |
| Space | Quick Look |
| Up / Down (Quick Look) | Previous / next file |
| Left / Right (Quick Look) | Previous / next page |
| Ctrl++ / Ctrl+- / Ctrl+0 / Ctrl+1 | Zoom in / out / fit / actual size |
| Ctrl+F / Ctrl+S | Find / save in a document pane |
| F1 | Keyboard shortcuts |

## Settings and saved data

Settings are grouped into Appearance, Glass and background, Files and search, Text editing, Previews, Keyboard shortcuts, Saved commands, Locations, Helper, Agents and Updates. Among them: hidden and system files, the default view, reopening panes and tabs, Seek as the default file manager, search ranking, indexed and excluded locations, content search locations, auto-save and backups, preview options and update checks.

Saved data lives in `%LOCALAPPDATA%\Seek`: the filename index, settings, window and pane layout, opened-item records and, if enabled, saved document text for faster content search. Saved document text is compressed, encrypted for your Windows account and capped in size. Nothing in your profile is part of a download, and each Windows account has its own data.

## Optional helper

Seek works without the helper, using ordinary folder listings and change tracking. **Settings > Helper > Install helper** asks for administrator approval to install a service that reads NTFS metadata and its change journal for faster indexing; the app itself always runs unelevated, and the helper can't read files your account can't. Remove it with **Settings > Helper > Uninstall helper** before uninstalling Seek.

If Seek is already removed, this removes the helper for your account (it asks for administrator approval):

```powershell
$sid = [Security.Principal.WindowsIdentity]::GetCurrent().User.Value
$helper = Join-Path $env:ProgramFiles ("Seek Helper\{0}\seek-helper.exe" -f $sid)
Start-Process -FilePath $helper -Verb RunAs -ArgumentList @('uninstall', '--sid', $sid) -Wait
```

## Update and uninstall

**Settings > Updates** checks GitHub for a newer release once when Seek starts (on by default) or when you press **Check now**. No file names, searches or paths are sent. **Download and restart** downloads the release over HTTPS, checks its SHA-256 checksum and contents, replaces the program files and restarts Seek; your saved data and the helper are untouched, and a failed update restores the previous files. You can also install a newer Setup EXE over the current one.

Uninstall from **Windows Settings > Apps > Installed apps > Seek**, or run `Uninstall.exe` in `%LOCALAPPDATA%\Programs\Seek`. A separate prompt (default No) asks whether to delete your saved data. The uninstaller also removes Seek's Explorer menu entries and gives folders back to File Explorer.

## Building from source

Requirements: Rust **1.96.0 x86_64-pc-windows-msvc**, Visual Studio 2022 Build Tools with **Desktop development with C++**, Windows SDK **10.0.26100.0** (for `rc.exe` and `fxc.exe`), and [NSIS 3](https://nsis.sourceforge.io/) to build the Setup EXE. The toolchain and framework revisions are pinned.

```powershell
git clone https://github.com/sanirudh17/Seek.git
cd Seek

rustup toolchain install 1.96.0-x86_64-pc-windows-msvc --component rustfmt --component clippy

cargo build --workspace --locked
cargo test --workspace --locked
cargo clippy --workspace --all-targets --locked -- -D warnings

# Optimized app, helper, portable ZIP, Setup EXE and SHA256SUMS.
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/build-release.ps1
```

The package appears under `target/release-package/`. Building never signs, installs the helper or uploads anything.

For development, run Seek with its own disposable index and profile so your saved data is never touched:

```powershell
cargo run -p seek -- --walker --root "$env:TEMP" --data .bench-data/development-profile
```

A copy run with `--data` never changes Explorer's menu entries or default-app settings.

## Project structure

```text
app/                    Native GPUI app, Windows resources and icon
crates/engine/          Filename index, search and Windows file system drivers
crates/seek-helper/     Optional indexing service and its install commands
crates/seek-cli/        Developer verifier and benchmark tools
vendor/gpui-component/  Pinned GPUI Kit source with documented local patches
vendor/gpui-windows/    Pinned Windows backend with documented local patches
packaging/              Installer, uninstaller and supplemental notices
scripts/                Build, packaging and validation tools
docs/                   Design notes, measurements and reference documents
THIRD-PARTY-NOTICES.txt Full redistribution notices and license texts
```

## Known limits

- The first index of a whole drive takes a while without the helper. Cached results appear first and the status bar shows progress.
- Folders your account can't read stay unreadable; Seek never elevates to reach them.
- Content search covers only the folders you choose, supported formats and files up to 4 MiB. Scanned PDFs without a text layer are not read.
- Cloud placeholders (files not downloaded to this PC) are never downloaded by search, previews, duplicates or similar images.
- Glass appearance depends on Windows transparency settings, the GPU driver and display scaling.
- Managed PCs may block unsigned installers; don't run Seek as administrator to work around a policy.

## License and credits

Seek's own code is [MIT](LICENSE). Releases include [THIRD-PARTY-NOTICES.txt](THIRD-PARTY-NOTICES.txt) with the complete license texts of shipped components. The filename engine derives from [fsearch-crossplatform](https://github.com/threehymns/fsearch-crossplatform) (MIT). The interface uses [GPUI](https://github.com/zed-industries/zed) and [GPUI Kit](https://github.com/longbridge/gpui-component) (Apache-2.0), Hugeicons and Lucide artwork, and the Figtree and Geist fonts (SIL OFL). Each dependency's license governs its component.
