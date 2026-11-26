---
name: Seek
description: A compact native Windows file finder on continuous neutral glass.
colors:
  accent-default: "#909090"
  light-text: "#202020"
  light-muted: "#505050"
  light-glass: "#ffffff"
  light-solid: "#ffffff"
  light-sidebar-solid: "#f3f3f3"
  light-line: "#00000014"
  light-hover: "#0000000c"
  light-sheet: "#ffffff"
  dark-text: "#f4f4f4"
  dark-muted: "#a0a0a0"
  dark-glass: "#000000b8"
  dark-solid: "#080808"
  dark-chrome-solid: "#111111"
  dark-sidebar-solid: "#0b0b0b"
  dark-line: "#ffffff14"
  dark-hover: "#ffffff10"
  dark-sheet: "#0000001f"
  light-settings-card: "#ffffff70"
  light-control: "#ffffff80"
  light-control-hover: "#00000007"
  light-control-selected: "#00000018"
  dark-settings-card: "#00000000"
  dark-control: "#ffffff08"
  dark-control-hover: "#ffffff10"
  dark-control-selected: "#ffffff2e"
  transparent: "#00000000"
  selected-black: "#000000"
  selected-white: "#ffffff"
  close-hover: "#c42b1c"
  light-toolbar-group-start: "#ffffffb0"
  light-toolbar-group-end: "#ffffff45"
  dark-toolbar-group-start: "#ffffff0e"
  dark-toolbar-group-end: "#ffffff03"
  light-popup-glass: "#f7f7f7"
  dark-popup-glass: "#232323"
  menu-accent-wash: "#90909024"
typography:
  toolbar-icon:
    iconFamily: "Hugeicons SVG"
    fontSize: "14px"
    fontWeight: 400
  body:
    fontFamily: "Figtree"
    fontSize: "13px"
  pane-icon:
    iconFamily: "Hugeicons SVG"
    fontSize: "16px"
    fontWeight: 400
  settings-control:
    fontFamily: "Figtree"
    fontSize: "13px"
    fontWeight: 400
    lineHeight: "16px"
  settings-navigation:
    fontFamily: "Figtree"
    fontSize: "13px"
    fontWeight: 400
    lineHeight: "20px"
  settings-title:
    fontFamily: "Figtree"
    fontSize: "19px"
    fontWeight: 600
  section-title:
    fontFamily: "Figtree"
    fontSize: "18px"
    fontWeight: 600
  card-title:
    fontFamily: "Figtree"
    fontSize: "14px"
    fontWeight: 600
  description:
    fontFamily: "Figtree"
    fontSize: "12px"
  sidebar-section:
    fontFamily: "Figtree"
    fontSize: "12px"
    fontWeight: 500
    lineHeight: "20px"
  metadata:
    fontFamily: "Figtree"
    fontSize: "11px"
  text-preview:
    fontFamily: "Consolas"
    fontSize: "11px"
rounded:
  settings-card: "12px"
  settings-sheet: "16px"
  accent-swatch: "6px"
  toolbar-button: "5px"
  toolbar-group: "7px"
  search: "12px"
  control: "8px"
  large: "14px"
spacing:
  compact: "4px"
  control: "8px"
  group: "12px"
  panel: "16px"
  settings: "20px"
components:
  toolbar-button:
    width: "32px"
    height: "30px"
    padding: "0"
    rounded: "{rounded.toolbar-button}"
    backgroundColor: "{colors.transparent}"
    textColor: "{colors.light-muted}"
    typography: "{typography.toolbar-icon}"
  toolbar-group:
    height: "32px"
    rounded: "{rounded.toolbar-group}"
  search:
    height: "32px"
    width: "100%"
    backgroundColor: "{colors.light-hover}"
    textColor: "{colors.light-text}"
    rounded: "{rounded.search}"
    padding: "0 8px"
  search-content-mode:
    height: "22px"
    padding: "0 8px"
    typography: "{typography.metadata}"
  content-status:
    height: "30px"
    padding: "0 12px"
    textColor: "{colors.light-muted}"
    typography: "{typography.metadata}"
  sidebar-row:
    height: "30px"
    backgroundColor: "{colors.light-hover}"
    textColor: "{colors.light-text}"
    typography: "{typography.body}"
  settings-category:
    height: "40px"
    typography: "{typography.settings-control}"
  settings-category-icon:
    size: "20px"
  settings-category-label:
    height: "20px"
    typography: "{typography.settings-navigation}"
  popup-menu-row:
    height: "28px"
    padding: "0 8px"
    textColor: "{colors.light-text}"
    typography: "{typography.body}"
  popup-menu-light-glass:
    backgroundColor: "{colors.light-popup-glass}"
    textColor: "{colors.light-text}"
  popup-menu-dark-glass:
    backgroundColor: "{colors.dark-popup-glass}"
    textColor: "{colors.dark-text}"
  popup-menu-row-active:
    backgroundColor: "{colors.menu-accent-wash}"
    textColor: "{colors.light-text}"
  accent-swatch:
    size: "24px"
    rounded: "{rounded.accent-swatch}"
  settings-card:
    backgroundColor: "{colors.light-settings-card}"
    rounded: "{rounded.settings-card}"
    padding: "16px"
  settings-action:
    height: "34px"
    backgroundColor: "{colors.light-control}"
    textColor: "{colors.light-text}"
    typography: "{typography.settings-control}"
  settings-view-segment:
    typography: "{typography.settings-control}"
    width: "242px"
    height: "34px"
    rounded: "{rounded.control}"
  footer:
    height: "30px"
    typography: "{typography.metadata}"
---

# Design System: Seek

## Overview

**Creative North Star: "Windows Finder on Neutral Glass"**

Finder proportions meet Windows file artwork and caption behavior, with Figtree interface text and Hugeicons controls. The interface is compact, quiet and practical: neutral black and white surfaces keep filenames and real file previews dominant. System, Light and Dark are explicit appearance choices.

This document records the current interface, including the bounded Stage 4 selection and Settings corrections, principally `app/src/appearance.rs`, `interface.rs`, `settings_view.rs`, `settings.rs` and `assets.rs`. It describes implementation and the user's selected direction; it does not certify screenshot fidelity, backdrop appearance on every device, or final visual acceptance.

The Stage A extension retains this Operate interface: left navigation, centered search and grouped right-side controls. Approved additions cover native GPUI menus for Type chips, search suggestions, file actions and Largest modes, centered toolbar SVGs, aligned Settings navigation, and appropriately sized selected previews. Their implementation is in `app/src/appearance.rs`, `interface.rs`, `daily.rs`, `settings_view.rs` and `media.rs`, with a small local popup-options patch in the pinned Kit source; no dependency version change is part of this extension.

The Stage B extension keeps the same Operate layout and themes. Content search adds a removable search-prefix control, a Matching line column, a compact location/status row and a locations card in Files and search. The Info action joins the centered toolbar SVG pattern, and light glass popups use the updated popup token. These additions are recorded from `app/src/appearance.rs`, `interface.rs`, `daily.rs`, `settings_view.rs` and `shell.rs`.

**Key Characteristics:**
- Continuous whole-window glass.
- Thin icons, aligned controls and fine dividers.
- Restrained categorized settings with separate cards.
- One large Gallery preview with a metadata inspector.

## Colors

### Primary

The default accent is Graphite. Reading saved settings migrates the former default (#707070) to the current Graphite token; other valid custom accents are preserved. A saved custom accent replaces it for result selection, the active view's (14%) wash, and the caret. The selected toolbar view glyph uses normal foreground text, retaining contrast even with a pale custom accent. Sidebar selection, Settings navigation and secondary controls remain neutral. Within Settings, accent is limited to selected indicators, the enabled glass switch and the primary Save locations action. Table selection uses an (18%) accent wash and normal foreground text. Grid tiles use a (16%) wash with an (18%) filename strip and normal foreground text. Opaque primary controls retain luminance-based contrasting text; do not apply that inversion to washed result selection.

### Neutral

The light palette uses charcoal text on white; the dark palette uses near-white text on black. Muted text carries metadata and explanatory copy. Lines and hover fills are low-opacity neutral overlays. In solid mode, sidebar and chrome gain their opaque palette surfaces. Sheets have their own denser surface for readable settings and menus.

Glass-mode menus use the separate light/dark popup tokens in the frontmatter. Menu hover and selection use the saved accent at alpha (0.14), with normal foreground text; `menu-accent-wash` records the Graphite default. Solid-mode menus use the existing sheet surface. Menu and popover surfaces are opaque even in glass mode (dark #232323, light #f7f7f7): popups sit over busy file lists and their text must never show rows through it. The window itself keeps its glass.

**The Continuous Glass Rule.** With glass enabled, apply the root tint once. Keep sidebar, toolbar background, table and footer transparent over it. Small toolbar groups and the search field add their specified neutral gradient surfaces without tinting the entire pane again.

## Typography

Figtree is the interface text family. Controls use the shared Hugeicons SVG family at 1.75 stroke in muted gray; native file-association artwork remains unchanged. Body text uses the compact base size. Toolbar glyphs are (14px), except pane-toggle and search glyphs (16px). Hidden-file visibility, Info and Settings use (14px) SVGs centered in full-size wrappers. The header has no location title. Settings title and section headings provide a restrained hierarchy. Metadata and footer text use the smallest role. Consolas is reserved for plain-text previews. Preserve readable casing; do not introduce display typography.

## Layout

The unified toolbar/caption is (52px) high, with left/right padding (16px / 12px) and (8px) gaps. A leading slot (160px) holds the standalone sidebar button and grouped back/forward controls, separated by (16px). The search field fills the flexible middle slot, ranging from (180px) to (560px) wide at (32px) high with a centered (30px) input. There is no location title, Ctrl+K badge or history-dropdown arrow.

After search, thin separators (1px x 20px) divide the view, action and caption groups. View buttons are three (32px x 30px) targets. Actions are five (30px x 30px) targets in this order: hidden-file visibility, sort, info, settings, file actions. Captions are three (26px x 30px) Windows control-area targets. Groups have no internal gap, a (32px) outer height and fine borders.

The Favorites sidebar content and its expanded animated slot both measure (204px); Favorites artwork and file-list icons are (16px). Results occupy the flexible center. The optional information panel is (280px), appears only with a selection, and scrolls independently. A single (30px) footer combines the breadcrumb path, result/selection count and index/access status. List rows are virtualized and unstriped. The application minimum window is (960px x 550px); preserve shrinking and truncation rather than introducing web breakpoints.

Content mode adds its own compact status row above results, using the frontmatter height and metadata role with (12px) horizontal gaps. Location names and search status truncate within the available width. Results use List view; the Matching line column replaces Date Modified at (360px) wide. Icons and Gallery controls are disabled while Content mode is active.

Settings use a (900px x 650px) popup capped at (94%) of window width and (92%) of height, a (52px) header and a (188px) category rail. Search is separated from navigation. Seven existing categories retain their options; labels and descriptions use 24px separation from controls and 20px vertical row padding. Long descriptions stack above controls. Background softening has its own row; content-location Save is compact; cache controls are stacked. The Settings surface follows window opacity live, with no file-list text painted beneath it.

Settings navigation retains (40px) rows with a (20px) centered icon slot, (18px) artwork and a (20px) text slot separated by (12px). Native popup rows are (28px) logical pixels. The Type menu is compact, bounded from (144px) to (180px) wide. Popup menus omit separator rows and visible scrollbars; wheel scrolling and keyboard navigation remain available when content exceeds the viewport.

## Elevation & Depth

Glass uses backend-owned Windows system acrylic with full-client DWM frame extension, retaining one material across native window states. New profiles use a neutral black root tint at 72%; window opacity is adjustable from 0-100%. Glass content/chrome paint no second fill; sidebar tint follows a cubic response and the popup adds 12% black tint. Light mode, high contrast and disabled system transparency remain opaque. Native Windows blur still exists at zero app tint. Idle performance and transient animation appearance require separate verification.

Main surfaces have no decorative shadows. The toolbar's compact rounded groups use a neutral vertical gradient, with distinct light/dark endpoints in the frontmatter; search fades its hover surface toward (65%) opacity. Settings and the sort menu use a border and a restrained shadow, recorded in the sidecar. Settings has its own nearly opaque neutral sheet, softly layered cards, and fine-border gradient controls; theme selection and category selection use neutral control fills. Native window rounding is requested through DWM. The sidebar width transition uses the motion module's critically damped spring (see Motion). Reversal starts at its current width. Request animation frames only while transitioning; no idle animation loop or decorative perpetual motion is part of this system.

## Shapes

Controls have small rounded corners; shared theme radii are in the frontmatter. Toolbar buttons, groups and search use distinct radii. Search remains a rounded rectangle rather than a capsule. Accent swatches are square silhouettes with subtly rounded corners, never circles. Selected swatches carry a contrasting outline. Settings use a rounded outer sheet with separate rounded cards; do not nest cards within cards.

## Components

Quick Look uses a 34px eye action aligned at the inspector's top right for images and supported PDF/DOCX/XLSX documents; the Enlarge text button is removed. Space from the file list or context-menu Quick Look opens the existing native reader; Ctrl+I toggles the inspector. Open uses the 200 ms entrance and close the 150 ms exit from Motion, opacity/position only with constant bounds, a cancellable finite close task and generation guard. Image Fit and 100% refer to the active decoded pixels, identified as original or preview resolution, with Ctrl+scroll zoom. Original decoded image pixels now supply 100% within explicit source/dimension/allocation bounds, with a labelled preview-only fallback. Final GUI acceptance remains recorded in the ledger. The latest user request reinstates the single Quick Look eye/menu/Space entry pattern.


- **Toolbar:** controls use the shared Hugeicons SVG family with native Windows caption behavior and file artwork. Icons stay centered in fixed-size targets; selected controls use a restrained accent wash. Labels and tooltips identify actions. The results-list frame focus indicator is locally suppressed while keyboard selection remains visible. Preserve whole-window native glass and system minimize/restore transitions; do not suppress animation or force runtime backdrop activation.
- **Toolbar state:** view and action buttons share one custom state style: a 14% accent wash for pressed/selected states, normal text for active icons and muted text otherwise. Hidden items, sort, Info and Settings use the same fill as the view switcher. Info is selected only when its inspector is visible.
- **Search:** a flexible rounded rectangle with a subtle neutral gradient, fine border and muted search icon; focus strengthens the border. Query chips live inside the search flow. Content mode adds a compact ghost button beside the search glyph in the prefix, using the `search-content-mode` height and padding, an (11px) Content label and an (11px) SVG X. Its accessible action is Exit content search; activating it returns to filename search.
- **Type and native menus:** the Type chip opens the compact native GPUI type menu and retains its remove action. Search suggestions distinguish Name contains, Content contains and file-type interpretations. The ellipsis control groups All items, Largest files, Largest folders, Search file contents, Type and file actions in the incumbent toolbar. Menus share the approved popup surfaces, (28px) rows, (14%) accent hover/selection and normal foreground text; use no separators or visible scrollbar. The local Kit options preserve wheel and keyboard interaction.
- **Content status:** keep a compact status row with configured locations, checked/matching/skipped/unreadable counts and explicit quick-result or completion state. The complete direct scan is cancellable and authoritative; do not reinstate the old global time or 200-match cutoff. Per-file input and expanded-text limits remain explicit. Match excerpts show up to five numbered lines in the inspector with visible truncation.
- **Navigation:** compact selected sidebar rows use a subdued neutral hover wash and normal foreground text. Generic sidebar line icons use muted foreground when unselected. Windows supplies Favorites artwork, file association icons and generic yellow folders. Folder rows share one native yellow folder icon from the atlas; do not request separate shell images for each folder. Bundled yellow folder and file-kind artwork remain fallbacks.
- **List and previews:** unstriped rows use hover/selection for state. Bounded visible-file shell requests replace fallback artwork asynchronously; supported images and videos request real thumbnails. Folder rows reuse the shared atlas icon. Large selected previews request a (768px) edge; this is source resolution, not a fixed display size. Large folder previews use native Windows artwork of at least (256px), preserving the small atlas icon for rows rather than stretching (32px) artwork. Preserve image aspect ratio. Gallery has one large central preview and a bottom thumbnail strip (155px); its inspector omits the duplicate image. Hide the inspector without a selected item.
- **Settings:** 13px Figtree controls, 32px flat targets (4% foreground fill, 1px line, no gradient), centered action labels and 8px rounded rectangles. Appearance and default-view choices use text-only buttons; dropdown labels remain left aligned with an independent chevron. Eleven functional pages share searchable, independently scrollable navigation/content. Previews controls change real requests and rendering; text auto-save/backups remain under Text editing. Saved programs are configured as an executable and separate arguments and run only through explicit actions. F1 opens the shared keyboard reference.
- **Content locations:** Files and search adds a separate Content search locations card using the incumbent settings card and input styling. Downloads and Documents are the defaults. A semicolon-separated full-folder-path input and neutral Save content locations action configure the scope; leaving it empty disables content search. Saving applies the content locations to the current search. The existing Locations category continues to configure filename indexing separately.
- **Footer:** one thin combined path/status band. Keep inaccessible-location and catch-up information legible there; Content mode carries its separate search summary above results.

## Do's and Don'ts

### Do:
- **Do** retain native Windows behavior, shell artwork and the pinned GPUI/Kit stack.
- **Do** keep theme colors neutral and reserve custom color for meaningful selection and actions.
- **Do** keep one large Gallery preview and show the inspector only for a selected item.
- **Do** preserve image channel correctness: GPUI RenderImage consumes BGRA; Windows DIB output already uses BGRA, while rasterized SVG output needs conversion.

### Don't:
- **Don't** add colored glass, stripes, nested settings cards, a Ctrl+K badge or a history-dropdown arrow.
- **Don't** duplicate the Gallery preview in the inspector or leave an empty inspector taking space.
- **Don't** infer visual acceptance or universal blur support from code, tests or API success.
- **Don't** expand the authorized narrow interface corrections into indexing redesign, dependency upgrades or additional features.


## Unreleased interaction follow-up (2026-10-20)

Source implementation awaiting an approved app build: suggestions belong to the actual search-field layout, rather than a screen-coordinate offset. Tab content appears independently of motion: tab insertion, active-tab wash and closing-slot contraction use the Motion durations; no result-list fade or stagger. Windows client-area animation preference disables spatial effects. No motion dependency or framework update. Up to eight inactive view snapshots share immutable rendered rows, bounded by 8 MiB per view and 24 MiB overall; evicted views reload normally.

List/grid multi-selection uses a stable range anchor and an independent caret. Selected rows use the existing accent wash; Ctrl-moving the caret leaves the selection unchanged. Permanent deletion now initially focuses the red Delete permanently button, explicitly requested by the user; Left/Right or Tab toggles Cancel/Delete, Escape cancels, and the captured selection remains one-shot. Rename/new-folder buttons also support arrows while their text fields retain text navigation.

Windows 11 system Desktop Acrylic replaces legacy accent blur where supported, with the existing fallback on older systems. Native API acceptance does not establish visual correctness during transitions. Painted-frame timing, restored/maximized popup alignment, Light/Dark focus styling, wheel routing and transition appearance still require the user's approved build. Do not label these visually verified from source tests.

## Revamp 1.2 — preview recovery pattern

Preserve the preview stage and page controls. Show the native failure with a human-readable operation prefix; long details scroll within 120 DIP. Retry and Copy diagnostics remain labelled secondary actions. These controls use the incumbent palette until the separate glass/inspector redesign. Native shape and critique are in docs/revamp/. No raster-layout change is part of this reliability item.


### Startup entrance
The native window stays hidden until saved appearance and a successful first presentation. With Windows client animations enabled, content enters at 88% opacity and reaches full opacity over 120 ms. No layout size changes and no persistent frame timer. Native first-paint checks do not substitute for transition video inspection.


### Interface typography update
Figtree is bundled locally (upright variable, regular/medium/semibold), with Windows fallback for other glyphs. Preserve document/code fonts and native PDF rendering. See docs/revamp/TYPOGRAPHY.md and the bundled SIL OFL. Do not scale text as a bitmap.


Revamp tab plates are 224 by 28px in a 38px strip with 2px gaps and 20px icon tiles. Selected/resting/hover fills are 10/4/7% foreground. Plus sits beside the last tab. Monocode-style open/close slots use the Motion entrance/exit and clip fixed-width content. Windows animation preference is retained. Full motion acceptance remains pending.

Information panel motion uses a fixed-width slot with the Motion entrance (open) and exit (close) slide and fade. Its width is reserved once on opening and released after closing, never animated per frame. Reversing a transition uses its current progress; cancellable close tasks have generation checks. Windows animation-off mode applies immediately. Spreadsheet framing has table content width plus 2px border, capped by its viewport, retaining horizontal scrolling. Image zoom adds Ctrl +/- and Ctrl+0/1, plus 32px zoom-out/in controls; mouse wheel behavior stays.

Quick Look image panning uses a fixed clipped viewport and explicit centered-image offsets with symmetric axis bounds. An overflowing image has an open-hand cursor, changing to a closed hand during a left-button drag. Drag deltas are clamped independently to each axis; non-overflowing axes stay fixed. Shift+arrow keys pan by 40 logical pixels. Fit, file changes and new Quick Look sessions reset position; release outside and loss of activation cancel a held grab. Panning adds no idle task or ticker.

Word Quick Look uses a centered max-900px text reader, 16px Figtree with 28px line height, 24px inset spacing and a clearly labelled excerpt cap. The outer reader scrolls continuously; the former nested short text scroller is bypassed in Quick Look. Excel Quick Look centers its content, uses 13px data text, 20px line height, 10px vertical/16px horizontal cell padding and 112-220px columns, while the compact inspector table retains its density. Quick Look eye artwork is the Monocode Hugeicons variant, rendered directly at 13px so ButtonIcon sizing cannot override it. Reset / Fit is an explicit labelled action with a reset icon. Windows maps grab styles to its single native hand resource.

Image toolbar refinement: 48px row, 32px non-shrinking +/- controls, a separate group with 120px Fit image and 80px 100% text buttons, a 52px percentage readout, then flexible truncated help text. Labels are actual rendered children rather than accessibility labels alone. The reset glyph is rendered directly at 14px. Fit can upscale to the available frame (bounded at 16x); Actual is 1x preview pixels. Selected mode is visibly highlighted. Image viewport geometry subtracts the new toolbar height.

### Phase-two refinement

Favorites and Locations occupy a 220 logical-pixel sidebar; shortcut hints and query-history blocks stay in Jump. Contextual menus group everyday item actions; the three-dot menu groups search modes and global tools. The palette contains the full command set and saved/recent management. Saved commands uses aligned labels, a compact multiline arguments editor with separate token help, one footer row and a program-window/play outline. History uses compact Undo/Redo and a short capacity/archive footer. Keyboard descriptions start after a proportional 36% key column and a 32-pixel gap.

## Motion

All animation timing lives in `app/src/motion.rs`; `scripts/check-motion.ps1` fails when an `Animation`, easing curve or elapsed-time motion comparison is written anywhere else. Adapted from Monodrive's motion vocabulary.

| Token | Value | Use |
|---|---|---|
| `ENTER` | 200 ms | Surfaces appearing: panels, panes, drop previews, new tabs, Quick Look open |
| `EXIT` | 150 ms | Surfaces leaving; always faster than entrances |
| `STATE` | 120 ms | In-place state changes: active-tab wash, settings page/sheet fade, window fade-in |
| `PREVIEW` | 150 ms | Drop previews (drag-to-split) growing and fading in |
| `SETTLE` | 180 ms | Short settle after a drop lands |
| `SPRING` | 260 ms | Reversible movement: sidebar width, split drop zones, column scrolling |

Curves: `ease_out` is `cubic-bezier(0.22, 1, 0.36, 1)`, a fast start with a long gentle landing. `spring` is a critically damped response `1 - (1 + wt)e^(-wt)` (w = 9), normalised to land exactly with no overshoot. Nothing eases in.

Rules:
- Animate opacity, position and size only; never animate content or row lists.
- Reversible movement starts from the current on-screen value, so changing direction partway never jumps.
- When Windows "Show animations in Windows" is off (`motion::enabled()` is false), every transition renders its end state at once and finite close tasks complete immediately.
- No animation loops while idle: frames are requested only while a transition is running, so idle CPU returns to zero.

## Split panes

Panes are rounded (8 px) cards on the chrome surface, separated by a 6 px gap that is also the divider's hit area. The focused pane has a 1 px accent border; other panes use the hairline line colour. A divider tints to 28% accent on hover, shows a resize cursor, and double-click shares the space equally. Panes keep at least 260 px width side by side or 180 px height when stacked. Each pane has its own 38 px tab strip and 30 px footer (location, item count, state). The search field in the top bar always belongs to the focused pane. A new pane fades in over the Motion entrance. Dragging files over an unfocused pane outlines it in accent with an 8% accent wash.

Tabs can be dragged: an accent 2 px insertion line marks the slot while reordering or moving a tab to another pane's strip.

## Drag-to-split

While a tab or sidebar folder is dragged over a pane's body, an accent rectangle (16% fill, 2 px border at 65%, 8 px radius, 6 px inset) covers the half nearest the pointer: left, right, top or bottom. It grows from 92% of its half and fades in with `PREVIEW`, slides between halves with the spring, and disappears when the drag leaves the pane or Esc is pressed. The tab strip itself keeps reorder/move behaviour and never shows the preview. On drop the new pane fades in from 55% opacity with `SETTLE`. A pane cannot be split by dragging its only tab onto itself.

## Column view

Columns are 240 px by default (150-640 px, dragged at a 6 px edge that tints to 28% accent), separated by hairlines. Rows are 28 px with a 16 px icon, the name, and a muted chevron for folders. The focused column marks selected items with an 18% accent fill and the caret with an 8% foreground wash; ancestor columns mark the folder on the path with the 8% wash. The last column previews the selected folder or shows a summary (preview, name, kind, size, modified) at least 300 px wide. When the column set changes, the strip scrolls with the motion spring so the focused column and preview stay in view; Windows animation-off moves it at once.

## Document pane

A document pane is a pane card like any other, with a 44 px header on the chrome surface: a muted file glyph, the name (medium weight) over "kind · size · modified" (11 px muted), then 28 px Reload and Close icon buttons. The body is the Quick Look reader, the document preview, a contained image, or the file icon with a short note for unsupported kinds. Its border takes the accent while its reader holds keyboard focus.

## Related folders

A 36 px row on the chrome surface under the focused tab strip. Chips share the tab plates' shape language: 26 px tall, 6 px radius, 4% foreground at rest, 7% on hover, 10% with full-strength text for the open folder, a 13 px folder glyph and a label up to 180 px; the tooltip shows the full path. Pills were rejected so the two adjacent rows keep one radius scale. A ghost "+" button with the same radius opens the menu. Dropping folders or tabs tints the row with an 8% accent wash. Empty rows show a one-line muted hint.

## Sidebar groups

While dragging in the sidebar, the row under the pointer (or the whole group, for a dragged group) takes the accent wash; there is no insertion line. Files dragged onto the middle of a sidebar folder row move into it; onto its edges, or anywhere else in a group, the folder is added to that group.

Group headers are 30 px rows: a 12 px chevron and the name in 12 px medium muted text, like the section headings. Naming a group replaces the header with a full-width standard-height field whose placeholder is "Group name"; Enter saves, Esc keeps the current name. An empty group shows no sentence; a blank drop row appears only while something is dragged. Items inside a group indent to 22 px. The info panel shows "Open in document pane" (panel-right glyph) beside Quick Look for any selected file.

## Organize review sheet

A 520 px modal sheet (14 px radius, modal surface and border). Title "Organize <folder>" at 15 px semibold. Each group is one row: a small switch whose on colour is the text colour, the folder name at 13 px semibold with its count right-aligned, the rule in one 12 px muted line, then up to three file names "and N more". A switched-off row mutes its text. The footer holds "N files stay where they are" (muted, left), Cancel and the primary Organize N. The primary button is the inverse of the surface (text-colour fill, surface-colour label), used for the one committing action of a sheet. Keyboard: Up/Down move between rows and buttons, Space toggles, Enter organizes, Esc cancels.

## Toast

A 36 px, 10 px radius pill centred 40 px above the window's bottom edge on the modal surface with a soft shadow: one line of 13 px text and, when the action can be reversed, a medium-weight Undo text button. It enters with the motion entrance and leaves on its own after six seconds.

## Split button

A primary action with an attached 22 px chevron segment inside one 6 px radius outline, divided by a 1 px rule. The chevron opens a small menu of variants. First use: the editors' Save, whose menu offers Save a Copy.

## Empty-space menu

Right-clicking empty space opens one compact menu: create and organize actions, Properties, Paste, the view choices and Show Hidden Files, then Sort by. Shortcuts are right-aligned in 12 px muted text; the current view and sort carry the menu's check mark.

## Update indicator

A 22 px ghost icon at the right of the sidebar footer, shown only when a newer release exists. Its popover is a 300 px card: "Seek x.y.z" with Download on one line (a ring and percentage replace the button while downloading, then Restart), a divider, and the release notes as short lines with 4 px dots. Errors are one short line; details go to the log.

## Cloud badge

Files that are cloud placeholders not stored on this PC carry a 13 px muted cloud-download glyph after the name (list) or before the size (grid), with a "Not downloaded" tooltip.

## Phase 5 interface remap

Presentation only; every control, setting, menu entry and shortcut remains (docs/revamp/5-INVENTORY.md).

- **One label, no helper line.** A control is named once. Explanations that are not needed to act live in a tooltip on the label; placeholders replace labels that repeated them. No taglines, key-hint footers or status words that say nothing ("Folder contents", "Up to date").
- **Selection:** list rows, grid tiles and column rows share a 26% accent wash, visible with the default graphite accent in both themes.
- **Controls:** flat 32 px buttons, 8 px radius, a 4% foreground fill and a 1 px line; no gradients. Toolbar groups use the same 4% fill with no border.
- **Switches:** the on track is the text colour (white in dark, near-black in light); off is the theme's neutral track. No On/Off text.
- **Settings:** a 56 px row per setting, label left (13 px medium) and control right; section titles 17 px semibold; Done on the page title line.
- **Popups:** opaque menu and popover surfaces (dark #232323, light #f7f7f7).
- **Jump:** the field is the header ("Jump to a folder or run a command"); results follow, no title or footer.

## Redesign (November 22)

- **Type:** Geist for the interface, Geist Mono for keys, sizes, dates, paths and code (OFL, bundled). Scale and radii live in `app/src/tokens.rs`: text 11 / 12 / 13 / 15 / 20; radii 7 row, 8 control, 12 card, 4 thumbnail; menu rows 30, buttons 28, fields 32.
- **Menus:** one floating card style everywhere (12 radius, 5 padding, 30 px rows with 7 px rounded neutral highlights, hairline separators, Geist Mono shortcuts). Jump: a 48 px field, a glyph per group, short labels.
- **Rows:** list, column, sidebar and grid highlights are inset and rounded with the row radius; selection is a 26% accent wash; hover is a group hover on the row.
- **Panes:** the focused pane alone shows a 32% text-colour outline; panes share space without hard minimums at render time.
- **Scrollbars:** overlay, 4 px soft thumb in the text colour, visible while scrolling or hovered, fading after 3 s.
- **Settings:** each setting is a rounded card with a 10% border; Theme, Default view and Saved commands' Required selection are sliding segmented controls; descriptions live in label tooltips. Keyboard reference: a card per group, short action labels, Geist Mono key caps.
- **Readers:** an icon toolbar (search, pencil, more; eye/document for Read/Source), a slightly darker line-number column with centred numbers, and a footer with only encoding and line endings. Preview failures show the file icon, "Can't preview this file", one plain reason and Open / Try again.
- **Icons:** Hugeicons throughout; Icons view, Gallery and Sort glyphs drawn in the same style (24 grid, 1.75 stroke, round caps).

## Phase 11 preview polish, secret protection & picture editing

- **Syntax palette:** 8 syntax roles (`keyword`, `string`, `number`, `comment`, `function`, `type`, `constant`, `punctuation`) derive directly from `Palette.tints` and meet >= 3:1 contrast across all 8 palettes in both dark and light modes; code blocks in Markdown use the same palette highlighter.
- **Readable previews:** Minified JSON and XML open formatted with a `Formatted` / `Raw` toggle (and an explicit undoable `Format document` action in the editor). Text, code and Markdown previews support selection and right-click `Copy` across info panel, Quick Look and Document panes, with an independent `Wrap` toggle persisted per surface.
- **Secret masking:** `.env` files mask values with deterministic BLAKE3-derived letters of independent length, offer per-value click-to-reveal and whole-file Reveal/Hide, auto re-hide on close or file change, and mask content search snippets and MCP `read_text` by default.
- **Quiet toasts & Locations:** Brief 2.5 s non-stacking toasts (`motion::TOAST_QUIET`, controlled by `Show confirmations`) confirm quiet actions. Sidebar `Favorites` and `Locations` fold independently, and `Locations` offers an inline `Edit` / `Done` mode alongside Settings > Locations visibility switches.
- **Picture editing:** Inline editor inside Quick Look and picture Document panes with labelled tools (Select, Crop with live pixel readout & aspect presets, Rotate 90°, Flip, Pen, Highlighter, Arrow, Rectangle, Ellipse, Text, Redact), 4 ink colours, live editable marks, permanent pixel-level black redaction with forced EXIF stripping, and a `Save a Copy` split button (`Replace Original` confirms before atomic replacement).
- **Instant Storage:** Default-drive Storage measurements are pre-warmed once the index is ready and cached per scope so opening Storage renders "By kind" and largest folders immediately while refreshing quietly in the background.
