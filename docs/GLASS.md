# Glass

Phase 7.1 audit: every fill painted between the Windows backdrop and the content, before and after. Dark theme with glass on; the light theme and glass off are opaque and listed at the end.

## Layers, top to bottom

| Layer | Where | Before | After |
| --- | --- | --- | --- |
| Backdrop | `instance` / `appearance::apply` | Acrylic (`Blurred`) | Acrylic, or Mica Alt with Blur: Soft |
| Window | root `seek`, `p.canvas` | **pure black at 72%** (hue 0, saturation 0, lightness 0) | the palette's hue-aware tint (Graphite: hue 222, saturation 10%, lightness 8%) at the window opacity, **at most 85%** |
| Picture | root, under content | at the behind-files opacity, every view | by scope: behind files (0–30%), in empty folders (0–80%); top and bottom 30% fade into the tint |
| Lift | root, under content | none | white 3.5% → 0 over the top 180 px |
| Grain | root, under content | none | optional 512 px noise tile, ±4% luminance; **off** by default and in every preset |
| Edge | root border | line, white 8% | highlight, white 11% |
| Sidebar | `p.sidebar` | the window tint × sidebar strength (default 0, clear) | the same tint, alpha capped so window + sidebar never exceed **92%** combined; default 40 → 3.8% |
| Main pane | `p.pane` | white 2.4% (and again for each split pane) | **clear**; with "Glass in the file list" off, an opaque deeper tint (94%) |
| Toolbar, tab strip, status bar, info panel, table header | `p.chrome` | white 1.2% | white 2% |
| Rows | hover / selection | white 6% / text 10% | unchanged |
| Settings cards | `p.settings_card` | white 2.7% | white 3.5% |
| Settings sheet | `p.sheet` | tint at 12% | tint at 50%, so the sheet reads over the window |
| Menus, popovers | popover token | opaque #232323 | unchanged (menus stay opaque over busy lists) |

Before, the default stack over the backdrop was black 72% + white 2.4% (pane) + white 1.2% (chrome): a nearly black window whose colour came from nothing behind it. After, it is one tinted layer at 62% (Frost) with a clear content pane, so the desktop's colour carries through, lifted slightly at the top and edged with a highlight.

## Presets

| Preset | Window | Saturation | Darkness | Sidebar | Grain |
| --- | ---: | ---: | ---: | ---: | --- |
| Clear | 40% | 6% | 6% | 25 | off |
| Frost (default) | 62% | 10% | 8% | 40 | off |
| Tinted | 70% | 34% | 11% | 50 | off |
| Solid | glass off | — | — | — | — |

Presets keep the palette's hue, and a palette keeps the preset. Changing any slider leaves no preset selected. Grain was on in Frost and Tinted for one build; on review the glass reads better clear, so it is off everywhere and settings from that build are switched off once (`glass_version` 3).

Settings saved before this change with the old pure-black glass (saturation and lightness both 0) are moved once to the Frost tint, with their window opacity capped at 62%; a tint someone chose is kept, and the migration never runs again (`glass_version`).

## When glass is off

Light themes, Windows high contrast and Windows transparency off paint opaque surfaces (`appearance::glass_enabled`). Light themes still take the palette: the sidebar is the palette's light surface, the window and bars a lighter wash of it (35% toward white), the file list nearly white (75%), and the palette's light accent. The dark solid surfaces are hue-aware steps, not black: sidebar 3.8%, window 4.5%, pane 6%, chrome 7.5% lightness at the palette's hue and 60% of its saturation.

## Colour

Six tints and a neutral (`appearance::Tints`), each at least 3:1 against the theme's panes in dark and light (tested): blue, cyan, green, orange, purple, pink. Used for the known places in the sidebar (Downloads green, Desktop cyan, Documents blue, Pictures purple, Videos pink, Music orange), All locations (blue), drives (cyan), saved searches (purple), the Type filter chip by kind, the cloud badge (blue), the update indicator (blue, green when ready) and the check in a finished operation's toast (green). Each is beside a label or a distinct icon; colour is never the only signal.

Eight named palettes (Settings > Appearance > Palette) set the tint, the text colour and the accent together: Graphite (default), Harbor, Moss, Ember, Orchid, Tidepool, Dune, Rosewood. The accent can still be changed afterwards.

## Not verified

The look over bright and dark wallpapers, Mica Alt (Blur: Soft), high contrast and transparency off with the new surfaces are checked at the Phase 7 checkpoint launch; see PROGRESS.md for the outcome.
