# Seek's Windows backend patch

Baseline: `crates/gpui_windows` from the official Zed repository at
`279fe070bb389b79652e52065b2f001edcc0b11b` (2026-10-04). Apache-2.0,
with the complete upstream license retained. GPUI core and its other platform
dependencies still resolve from this exact official git revision. This local
crate is selected through the workspace's git-source patch, not a framework switch.

`scripts/vendor-gpui-windows.py CHECKOUT` reproduces the unmodified source and
resolves workspace dependencies at the same pin; `--manifest-only` refreshes
only the manifest. Local Rust changes must be preserved when updating upstream.

Changes:

- `frame_demand.rs`: predicate-based condition variable, per-window pending flag,
  coalescing requests, explicit stop wake. VSync parks indefinitely with no demand.
- Platform/window integration implements GPUI's existing `frame_waker` and
  `schedule_frame` hooks. Only requested windows are invalidated. Deferred draws
  and renderer resize failures explicitly re-arm demand. GPU health checks run
  when requested; there is no background device-health polling.
- DirectManipulation continues requesting frames while a touchpad gesture or
  inertia is active; stationary viewports return to sleep. Native Win32 expose,
  resize, activation and visibility messages continue using upstream handlers.
- Backdrop calls return their actual API outcome. Acrylic falls back to Mica,
  then opaque rendering. Version structures now declare their size. Explicit
  Mica modes also fall back to opaque on failure.
- Optional `seek-bench` exposes native WM_CHAR receipt and successful renderer
  draw/Present-return timestamps, startup milestones and opt-in frame tracing.
  Frame tracing also records native keyboard/IME, mouse, pointer and focus
  message ids to distinguish activity from idle redraws. It records no input
  text or coordinates. Seek's optional application diagnostics record test
  selection/sort and shell-dispatch outcomes on the same worker-written stream.
  Normal builds do not compile these observers.
  This measures presentation submission, not physical display scanout.
- Initial DirectWrite collection lookup uses the Windows font cache rather than
  forcing an immediate installed-font refresh. The existing refresh on a failed
  family lookup is retained. Microsoft documents this cache behavior in
  [GetSystemFontCollection](https://learn.microsoft.com/en-us/windows/win32/api/dwrite_3/nf-dwrite_3-idwritefactory3-getsystemfontcollection).

The baseline unconditional repaint loop used 609.375 ms CPU in a quiet 20-second
shell sample. With this patch the debug spike measured 0 ms CPU over 11.726 seconds,
and explicit Mica measured 0 ms over 20.003 seconds. Raw evidence is in
`docs/benchmarks/spike-demand-*.json`. These are measured samples, not a proof
for every driver, GPU recovery, monitor change or touchpad device.

No upstream issue, PR or external message has been sent. This is a local patch
that needs upstream review before replacing it with an official fixed revision.
