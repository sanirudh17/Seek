# Seek 1.0.0 packaging and updater verification

2026-10-20. Packaging is explicitly approved; the later request also authorizes startup update checks and a Settings update page. No framework revision, application icon or indexing/content feature was changed for packaging.

## Distribution

The public release is [v1.0.0](https://github.com/sanirudh17/Seek/releases/tag/v1.0.0). Its unsigned x64 ZIP contains the optimized app, optional helper executable, existing icon, per-user install/uninstall/update scripts, README, license and full third-party notices. Extract it and run `Install.cmd` normally. Installation uses `%LOCALAPPDATA%\Programs\Seek`, a current-user Installed apps entry and Start menu shortcut. The desktop shortcut is optional. There is no administrator install, helper service installation, autostart, tray, global hotkey or resident updater process.

Uninstall asks separately before removing saved index/settings/document text; the default keeps data. An already opted-in helper is preserved and reported, with Settings/explicit administrator removal instructions in README. No test altered the real app profile or helper service.

## Update behavior

Settings > Updates uses a circular refresh icon, distinct from the information-panel icon. Startup checks are on by default and run once per app process after window creation. Manual Check now is available. Only Download and restart approves replacing program files and closing session tabs. HTTPS requests are restricted to GitHub API/release hosts. Fixed package contents and SHA-256 are checked before a finite, unelevated PowerShell worker handshakes, waits for Seek to exit, backs up/replaces program files and restarts it. Preparation failure leaves Seek open; replacement failure attempts rollback. Saved user data and the protected service copy of an opt-in helper are untouched.

No publisher signature is provided. Checksums detect corruption but trust still depends on the GitHub repository. A future-version download/restart cannot be tested against the first release alone. Worker replacement/rollback and live package download are separate targeted checks, not a claim that a real future upgrade has been exercised.

## Privacy and provenance

The public repository starts from a fresh, scrubbed source snapshot; private historical Git commits are not uploaded. Existing unrelated local files remain private. Personal profile paths, account identifiers and document identities in historical reports/scripts are replaced by current-user variables/placeholders and stable document aliases. Historical measurements are retained. Two old screenshots containing personal paths/document names are preserved privately and omitted from the public snapshot. An old diagnostic image now embeds its bytes rather than an absolute compile-time build-folder path. Legitimate dependency author/license attribution and the chosen public GitHub handle are retained.

README follows the readable Carbon/Typr structure, with installation, complete keyboard/mouse command table, search/preview limits, helper, update/uninstall, source-build commands and project layout. `THIRD-PARTY-NOTICES.txt` includes full notices for 496 app/helper dependency/build packages plus icon artwork; engine/GPUI/Kit revisions and local patch provenance are identified. No third-party version or framework revision was updated.

## Targeted validation

- 54 app tests passed; the automated live-staging test was not run; its separate test-profile compilation was stopped. The shipped app's Check now result and a fresh GitHub ZIP download/checksum were verified directly. Strict all-target/all-feature Clippy and normal-feature compilation passed. The small diagnostic-image privacy fix also passed normal-feature compilation.
- Installer scripts passed 20 checks with an owned temporary profile, program directory, shortcut directories and unique HKCU test registration. All six current-user Known Folder labels were present. Checks covered metadata/icon, ordinary launch/normal close, refusal to overwrite a running app, uninstall Cancel/default data retention/explicit data removal, optional desktop shortcut, no unexpected fixture loss and unchanged helper registration. The tested candidate and final release smoke are distinguished in the reports.
- Update replacement passed seven checks using disposable copies: success, handshake, locked-file rollback, changed-payload rejection before handshake and data preservation in all three cases. NoRestart and the same version are used to test the worker without touching a real installation.
- Only one enabled local account exists; its current token is unelevated. A separate standard-user installation was not verified. README explains creating a second standard account through Windows Settings with an administrator, then repeating the install/sidebar/uninstall checks there.

The final optimized executable is 19,824,640 bytes; the ZIP is 9,164,598 bytes. The exact final package passed 20 owned-profile install/uninstall checks, and the worker passed seven replacement/recovery checks. The clean release launch/close passed as an ordinary user. Final hashes, check counts and known limits are recorded in PROGRESS and the packaging benchmark reports. Existing Stage C performance figures are historical; packaging does not claim new launch or input latency improvements. The installer smoke time includes UI Automation and is not a startup benchmark.
