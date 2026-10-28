# Preview polish and navigation checks — October 27

The test app now centers the compact Enlarge preview button. Following user review, it reuses the exact Settings button component in both themes: subtle glass gradient, quiet border, matching hover/pressed states and eight-pixel corners. The earlier high-contrast Dark override was removed. The enlarged reader has a dedicated toolbar, fixed page controls and no underlying application controls. Presentation controls retain their position across slides. All locations pagination is at the bottom, with grouped counts and native buttons.

Folder addresses such as `%TEMP%` get an Open folder suggestion in both Name and Content modes. Press Enter to navigate. Address input cancels an existing search and does not start a filename or document-content scan. Current-user environment variables are expanded without executing a shell.

## Preview performance

Windows native imaging now decodes rendered PDF pages directly into the BGRA format used by the UI. One loaded PDF can serve multiple page requests in an isolated renderer. It exits after two idle seconds and is terminated when the panel closes. Existing four-second request, 256 MiB job, 32 MiB input and 16 MiB output limits remain. This renderer does not start with the app or index documents in the background.

A volatile cache retains up to four frames / 24 MiB and prefetches one adjacent page. Filesystem size and precise modification time invalidate changed files; deletion and panel closure invalidate cached entries. No document text or preview files are persisted by this cache.

Debug measurements on the selected 39-page PDF (worker time, excludes UI paint):

| Operation | Before | After |
| --- | ---: | ---: |
| Immediate enlarged next page | 442.586 ms | 105.360 ms |
| Next page already prefetched | 1.224 ms | 1.228 ms |
| Initial PDF load | 631.940 ms | 317.425 ms |

A separate session check rendered four uncached page turns in 109–147 ms using the same process, verified idle exit, then reopened it successfully. Initial load in that separate run was 1,539 ms, so these samples do not establish a cold-start or p95 guarantee. An owned Word fixture measured 213.919 ms initially and 1.759 ms on revisit. These results do not measure general content-search throughput.

## Verification and limits

- 73 regular Rust tests passed serially; four environment-dependent tests ignored. The first concurrent run failed a folder-browsing assertion; the complete serial rerun passed. Strict all-target/all-feature Clippy passed.
- Explicit real-PDF session lifecycle and production-worker cache measurements passed separately.
- Owned-profile desktop checks passed in Light and Dark: complete All locations count, PDF controls, enlarged reader, no underlying controls, Right-arrow paging and Escape. Dark additionally covered the second results page, two PowerPoint slides with stable controls, and address navigation from Name and Content modes. Both test windows closed normally.
- PowerPoint uses saved artwork for the first slide where available and ordered slide text. It does not reproduce every slide's graphical layout. Word depends on available thumbnail providers, with text fallback.
- Test builds only. No release build, installer, version bump, installed-app replacement or publication. Production data is unchanged.

Raw local measurements and screenshots are retained in the ignored `.references` directory. Final visual acceptance remains with the user.
