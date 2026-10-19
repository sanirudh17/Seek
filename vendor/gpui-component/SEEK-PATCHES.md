# GPUI dependency alignment

Vendored from longbridge/gpui-kit, commit `1ccf011620da24d633f7cd6d3c11ca2e97798dcf`, version 0.7.1. Keep the Apache-2.0 LICENSE and original notices.

Only the four required crates are included. The workspace manifest replaces the packaged `gpui-pre-*` 0.3.8 snapshots with their declared source revision in `zed-industries/zed`: `279fe070bb389b79652e52065b2f001edcc0b11b`. This keeps Seek on the requested pinned git GPUI, rather than the older crates.io GPUI or a third-party framework fork.

The source revision comes from the snapshot's `[package.metadata.gpui-pre] zed-rev`. The `IntoPlot` derive's crate-name resolver also accepts the official `gpui` package before considering the optional `gpui-fast` package.

The additional `gpui-base/seek-static-caret` feature keeps the focused caret visible without the upstream 500 ms blink and 300 ms pause timers. Seek enables it to meet its event-driven idle requirement. The original blinking implementation and its tests remain available when this feature is off. This removes input timers; the separate Windows backend patch handles VSync demand.

## Continuation changes

`InputBaseState::decorate_existing_text` installs token presentation only when
the supplied text equals the current value. It preserves selection, cursor and
undo history. Seek uses it to turn completed quoted filters into chips without
resetting the input on every space.

## Seek v2 Stage A menus

The pinned Kit version is unchanged. `PopupMenu` adds opt-in `item_height` and
`scrollbar_visible` settings. Seek uses 28 px rows and hides the drawn scrollbar
while retaining wheel, trackpad and keyboard scrolling. Defaults for other callers
are unchanged. Submenu chevrons now follow the existing computed opening anchor,
including menus opening left near a window edge. These local rendering changes
avoid replacing GPUI menus with Windows shell menus.
