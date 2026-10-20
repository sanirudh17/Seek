# Seek 1.0.1

2026-10-20. Public release [v1.0.1](https://github.com/sanirudh17/Seek/releases/tag/v1.0.1) includes a native Setup EXE. The first EXE falsely reported failure because NSIS's error flag could remain set after an absent optional `/FIXTURE` argument; `Uninstall.exe` had been written, but the native Installed apps registration was skipped. The flag is now cleared before the uninstaller write/check. The replacement EXE passes all 23 installer checks and is the current release download.

## Changes and cause

The content-location edit used the compact Small input. It now uses the full-width Large GPUI input and was inspected in Dark. Light was not rechecked because the open window's folder/selection changed during the visual check; further UI input stopped. Screenshots remain private and are not uploaded.

Without a saved index, All locations previously waited for the full drive namespace to be installed. Sidebar folder views already had finite direct metadata reads. All locations now uses those reads for up to 200 recent files from in-scope current-user priority folders and drive roots while the full scan continues. It explicitly says **Recent files · indexing all locations...**. When the index is ready, normal authoritative indexed results replace the provisional rows. Folder priority, visibility, exclusions and root boundaries are retained. No claim of complete results before indexing finishes.

The EXE installer uses NSIS 3.12 with RequestExecutionLevel user, current-account paths, optional desktop shortcut, Start shortcut, HKCU Installed apps registration and a native uninstaller. Saved data defaults to Keep; explicit deletion is separate. Unexpected program files are retained, and a previously opted-in helper is retained and reported. The installer never installs the helper. Replacing an existing installation requires approval; silent replacement is refused. The updater's own install scripts are recognized during uninstall, and existing desktop-shortcut ownership survives replacement.

A separate ZIP remains for compatibility with the already shipped updater. The recommended download is **Seek-1.0.1-Setup-x64.exe**. The replacement EXE, ZIP and UTF-8/BOM-free SHA256SUMS have matching version/checksums. A final checksum-format check caught and fixed a one-line PowerShell array-unwrapping issue before the original upload.

## Verification

[Measured report](benchmarks/release-1.0.1.json). The clean separate source checkout at `60501e7` built the optimized app/helper in 843 seconds with a static MSVC runtime and remapped paths. Later packaging-only commits did not change the Rust executable. The app is 19,951,104 bytes, with 1.0.1 Windows metadata. App/helper imports need no dynamic MSVC CRT; neither contains the personal build-profile path. No framework or dependency version was updated.

- **65 Rust tests passed**, one live-release test initially ignored; strict all-target/all-feature Clippy and formatting passed.
- Owned unfinished-index fixture: All locations supplied provisional rows after **1.537 ms**, while full-index ready was false.
- Real C:, fresh owned profiles, helper disabled: first UI Automation row **26,606.310 ms before**, **2,793.024 ms after**. These are single-run observations including polling/accessibility and uncontrolled system load, not full-scan or cold-cache timings. The after run used the packaged static-runtime release; do not treat this as a controlled compiler-flag-only comparison or claim sub-second paint.
- Final EXE: **23 native installer checks passed**, including per-user registration, native uninstaller, default data retention, explicit owned-data removal, unexpected-file preservation, silent overwrite refusal, shortcut removal and unchanged helper registration.
- The currently installed account's Installed apps registration was repaired to point to its existing native uninstaller. Saved data was not modified.
- Final package: **21 checks passed**, including metadata/icon, normal owned-profile launch/close, all six current-user Known Folder labels, desktop ownership on replacement, running-app refusal and clean script uninstall.
- **7 update-worker checks passed** for replacement, handshake, locked-file rollback, tampered-payload rejection and saved-data preservation. Native installer tests also cover removal of updater-staged install scripts.
- The full live update staging test failed twice with WinHTTP error **12002** (timeout). Direct HTTPS checks succeeded: GitHub API **0.435 s**; ZIP download **1.013 s**, **9,212,488 bytes**. The app's native WinHTTP download and staging path remains unverified.

Only one enabled local account exists. No second-account test, Light field recheck, physical HDR/mixed-DPI, full old-client Download and restart, full-drive completion benchmark, fresh idle/memory/UI-tail benchmark or cold-system-cache measurement was performed here. Historical measurements remain historical. All destructive checks used marked disposable TEMP fixtures; production Seek data, installed program and helper were untouched.

## Check by eye

- Content-location field is comfortably sized; try Light too.
- First-launch All locations shows useful recent files and the indexing label, then normal results.
- Setup's desktop shortcut choice; Installed apps uninstall's default saved-data retention.

See [CHANGELOG](../CHANGELOG.md), [installer source](../packaging/Seek.nsi) and [NSIS execution-level documentation](https://nsis.sourceforge.io/Reference/RequestExecutionLevel).
