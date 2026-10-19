# Supplied app icon

2026-10-18. Task 1 only: the supplied transparent artwork is preserved as
`app/assets/seek-icon-source.png`. Aspect-fit square padding, premultiplied-alpha
Lanczos downsampling and restrained sharpening produce seven independent frames:
16, 24, 32, 48, 64, 128 and 256 px. The ICO is **163,591 bytes**, with 32-bit DIB
and AND masks at the smaller sizes and PNG at 256. Reproduction:

```powershell
python scripts/make-app-icon.py app/assets/seek-icon-source.png
```

The executable embeds icon resource 1 using the already locked embed-resource
3.0.12. The window installs small/large shared resource icons at its initial DPI;
Windows uses the latter for the taskbar. No framework or Windows crate version
changed. No other app behavior changed.

## Checks

- All seven frame dimensions decoded and checked.
- Windows `PrivateExtractIconsW` successfully selected the executable's 16/32 px
  resources (IDs 1/3); all four light/dark renders contain visible artwork.
- `WM_GETICON` returned nonzero small and large handles on the actual window.
- Actual Windows taskbars and Explorer inspected in both Light and Dark at the
  connected display's 125% scaling. Windows theme settings were restored in
  `finally`; the temporary app closed normally. Exact 16/32 px native extraction
  proofs complement the physically scaled Windows checks.
- The white symbol remains clear at both small sizes. The black rounded-square
  edge partly blends into dark surfaces; it does not disappear on Light. No
  unrequested outline or alteration of the supplied design was added.
- Debug build and strict all-feature Clippy pass. The subsequent optimized
  baseline and final Task 2 builds also pass native 16/32 px icon extraction in
  Light/Dark. The final release hash is recorded in
  [the content-search report](CONTENT-QUICK-RESULTS.md).

Owned local evidence: `.references/app-icon-proof.png`, `native-icon.png/json`,
`icon-taskbar-dark.png`, `icon-taskbar-light.png`, `icon-explorer-dark.png`,
`icon-explorer-light.png`, and icon build/Clippy logs. Visual proofs use a temporary
copy of the executable, never the real Seek data folder. Other DPI/HDR hardware
was not checked. Task 2 has not started at this commit.
