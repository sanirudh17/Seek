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
- Glass windows follow Glint's presentation guard (material.rs/window.rs in
  Glint): the material stays active only while visible (minimize and hide are
  tracked from WM_SIZE/WM_SHOWWINDOW); minimizing animates natively; a
  taskbar restore of a minimized window is DWM-cloaked, given its active
  backdrop, restored, and uncloaked after DwmFlush once its first fresh frame
  is presented (3 s fallback timer). Without it Windows briefly showed the
  stale retained frame and the inactive backdrop.
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

- Revamp 1.3: keep initial placement hidden through the first successful rendered presentation; explicitly queue the initial draw because hidden HWNDs do not consume invalidated WM_PAINT. Defer activation, and reveal saved-maximized windows directly. See docs/revamp/1.3-STARTUP.md for evidence and outstanding visual/idle checks.

- Revamp 1.4 (in progress): one backend-owned system acrylic material, full-client DWM frame extension (adapted from Glint), redundant-request suppression, visible inactive-material guard and GDI erasure suppression. Present the resized scene before returning from WM_SIZE. Native OS transitions remain enabled. Three cycles retained type 3 in 13 states; a settled maximized dark capture was checked. Transient frame behavior and controlled idle acceptance remain open. See docs/revamp/1.4-GLASS-BRIEF.md.

- Revamp cursor follow-up: map OpenHand/ClosedHand explicitly to Windows IDC_HAND instead of the arrow fallback. Windows supplies one shared native hand resource for these styles. App build checks pass; the focused vendor test requires test-support dependencies unavailable in the current offline cache.
