# Seek 1.0.2 — release verification

Published October 28, 2026 to [sanirudh17/Seek](https://github.com/sanirudh17/Seek/releases/tag/v1.0.2), using GitHub CLI authenticated as `sanirudh17`. GitHub resolves the release-source commit author to that account with the owner's requested email identity. [Changelog](../CHANGELOG.md), [machine-readable checks](benchmarks/release-1.0.2.json).

Run **Seek-1.0.2-Setup-x64.exe**. Close Seek first, then run it normally. The per-user installer preserves saved data when replacing program files and keeps the helper opt-in. The ZIP is retained for existing in-app update clients; the EXE is the recommended installer.

## Build and artifacts

The optimized, static-runtime app/helper built in 8m 41s from clean commit `179e236b6eeb1899c977618447595815cf108b3f`, starting from published main and preserving the native-uninstaller fix. Tag `v1.0.2` identifies that exact build source. This verification record is a later documentation-only commit. No framework/dependency versions were changed; the existing Windows crate gained its native imaging feature.

- Setup EXE: **9,604,707 bytes**, SHA-256 `46b3df434e789bbed7ccb1ba9dce5c4b493f6de1f004e436ce236b9071d8adf9`.
- Updater ZIP: **9,293,725 bytes**, SHA-256 `898b4c92382686389722c7081d1b33cae617347267678552ea346a10bcc8ab98`.
- App: **20,162,560 bytes**, SHA-256 `392c26da646683be5f110536818c0429ef69c42ea922e85629660bb7d13d3582`; 1.0.2 Windows product/file metadata. The icon is unchanged. Both app/helper binaries were checked for personal build-profile paths; none were present. App import inspection found no dynamic MSVC CRT requirement.

Draft downloads and subsequent **unauthenticated public downloads** of the EXE, ZIP and checksum file matched the local tested artifacts byte for byte. The freshly downloaded public installer passed its native checks again. No personal test documents, screenshots, index, preferences or document-text cache were packaged.

## Checks

**148 regular Rust tests** passed: 73 app, 68 engine and seven helper/CLI. Strict all-target/all-feature app Clippy passed. The folder-browse test now waits for a complete folder response rather than accepting an index-ready progress message. Selected-PDF normal/enlarged worker checks, native PDF/error fixtures, session reuse/idle exit/reopen, cache invalidation/cancellation and the selected presentation first/last-slide checks passed separately.

The **shipping optimized executable** passed **44 desktop checks in each theme**. They covered normal-panel PDF paging, enlarged paging and Escape, Up/Down switching between digital/scanned PDFs, removal of underlying reader controls, genuine maximize/restore with window-width verification, hidden-sidebar search, Word fallback/enlargement, PowerPoint first/second/third slide controls and enlargement, complete All locations pagination, environment-folder navigation in Name/Content modes, Ctrl+T/Ctrl+Tab/Ctrl+W and normal exit. Test-control/window-state assertions were corrected during the pass; final counts refer to the shipping executable hash above. Settled screenshots were inspected privately rather than published with personal folder contents.

**23 native installer checks**, **21 package checks** and **seven updater checks** passed in marked TEMP fixtures and unique HKCU test registrations. A second set of **23 checks** passed against the freshly downloaded public EXE. Coverage includes native uninstaller registration, default saved-data retention, explicit owned-data deletion, unexpected-file preservation, optional shortcuts, refusal to overwrite a running app, current-user Known Folder sidebar entries, update handshake/rollback/tamper rejection and unchanged helper registration. Production installation, saved data and helper were untouched. After publication, Settings > Updates > Check now in the shipping app displayed **Seek 1.0.2 is up to date** against the live GitHub release.

## Latency samples

Three samples per document and size, using the test-harness worker with the optimized shipping renderer. Values exclude UI painting and uncontrolled system-cache effects.

| PDF | Raster edge | Initial load | Immediate uncached next page | Prefetched page |
| --- | ---: | ---: | ---: | ---: |
| Digital | 768 (panel) | 176–943 ms | 32–43 ms | 1.2–1.3 ms |
| Digital | 1536 (enlarged) | 208–227 ms | 66–78 ms | 1.7 ms |
| Scanned | 768 (panel) | 209–239 ms | 77–91 ms | 1.3–1.7 ms |
| Scanned | 1536 (enlarged) | 300–375 ms | 127–187 ms | 1.2–3.1 ms |

A separate enlarged digital-PDF session rendered four uncached page turns in **71–74 ms** using the same host, then verified automatic idle exit and reopening. Word fixture revisits reused cached results; raw timings are in the machine-readable report. These are samples, not a p95 guarantee. The installed-fixture launch observation was **1,071 ms** to an accessible result row, not a cold-cache measurement.

The renderer starts only for a selected PDF, retains the four-second request/256 MiB job bounds and exits after two idle seconds. Closing the panel terminates it and clears the volatile frame cache (four entries / 24 MiB). No resident preview service was added.

## Limits and manual coverage

PowerPoint displays saved first-slide artwork and ordered slide text, not the graphical layout of every slide. Word can use a Windows thumbnail or text fallback. Preview limits remain 32 MiB and four seconds per request; encrypted, broken and unsupported documents can report unavailable with Retry.

Only one enabled Windows account exists on this PC. A second standard-account test remains unverified. To perform it, use Windows Settings > Accounts > Other users > Add account, set its account type to Standard, sign in, and run the EXE. Confirm the sidebar uses that account's folders and uninstall retains its data unless explicitly removed.

Windows 10, physical HDR/mixed DPI, high-speed recordings of all glass transitions, true cold-cache launch, new full input-to-present p95 and the complete old-client automatic download/restart GUI flow remain unverified. The owned updater fixture covers replacement, handshake, rollback and saved-data preservation. This release does not claim to close historical 16 ms input-to-present or cold-cache targets.
