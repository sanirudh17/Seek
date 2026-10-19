---
name: Seek
description: A compact native Windows file finder on continuous neutral glass.
colors:
  accent-default: "#909090"
  light-text: "#202020"
  light-muted: "#505050"
  light-glass: "#ffffffcc"
  light-solid: "#ffffff"
  light-sidebar-solid: "#f3f3f3"
  light-line: "#00000014"
  light-hover: "#0000000c"
  light-sheet: "#f8f9fcfa"
  dark-text: "#f4f4f4"
  dark-muted: "#a0a0a0"
  dark-glass: "#000000cc"
  dark-solid: "#080808"
  dark-chrome-solid: "#111111"
  dark-sidebar-solid: "#0b0b0b"
  dark-line: "#ffffff14"
  dark-hover: "#ffffff10"
  dark-sheet: "#141414f5"
  light-settings-card: "#ffffff70"
  light-control: "#ffffff80"
  light-control-hover: "#00000007"
  light-control-selected: "#00000018"
  dark-settings-card: "#ffffff04"
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
  light-popup-glass: "#f1f1f1cc"
  dark-popup-glass: "#202020f0"
  menu-accent-wash: "#90909024"
typography:
  toolbar-icon:
    fontFamily: "Segoe Fluent Icons"
    fontSize: "14px"
    fontWeight: 400
  body:
    fontFamily: "Segoe UI Variable Text"
    fontSize: "13px"
  pane-icon:
    fontFamily: "Segoe Fluent Icons"
    fontSize: "16px"
    fontWeight: 400
  settings-control:
    fontFamily: "Segoe UI Variable Text"
    fontSize: "13px"
    fontWeight: 400
    lineHeight: "16px"
  settings-navigation:
    fontFamily: "Segoe UI Variable Text"
    fontSize: "13px"
    fontWeight: 400
    lineHeight: "20px"
  settings-title:
    fontFamily: "Segoe UI Variable Text"
    fontSize: "19px"
    fontWeight: 600
  section-title:
    fontFamily: "Segoe UI Variable Text"
    fontSize: "18px"
    fontWeight: 600
  card-title:
    fontFamily: "Segoe UI Variable Text"
    fontSize: "14px"
    fontWeight: 600
  description:
    fontFamily: "Segoe UI Variable Text"
    fontSize: "12px"
  metadata:
    fontFamily: "Segoe UI Variable Text"
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

Finder proportions meet Windows file artwork, caption behavior and Segoe UI. The interface is compact, quiet and practical: neutral black and white surfaces keep filenames and real file previews dominant. System, Light and Dark are explicit appearance choices.

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

Glass-mode menus use the separate light/dark popup tokens in the frontmatter. Menu hover and selection use the saved accent at alpha (0.14), with normal foreground text; `menu-accent-wash` records the Graphite default. Solid-mode menus use the existing sheet surface. The menu surface stays translucent while the native window supplies acrylic blur.

**The Continuous Glass Rule.** With glass enabled, apply the root tint once. Keep sidebar, toolbar background, table and footer transparent over it. Small toolbar groups and the search field add their specified neutral gradient surfaces without tinting the entire pane again.

## Typography

Segoe UI Variable Text is the interface text family. Most toolbar symbols use Segoe Fluent Icons at normal weight in muted gray. Body text uses the compact base size. Toolbar glyphs are (14px), except pane-toggle and search glyphs (16px). Hidden-file visibility, Info and Settings use (14px) SVGs centered in full-size wrappers. The header has no location title. Settings title and section headings provide a restrained hierarchy. Metadata and footer text use the smallest role. Consolas is reserved for plain-text previews. Preserve readable casing; do not introduce display typography.

## Layout

The unified toolbar/caption is (52px) high, with left/right padding (16px / 12px) and (8px) gaps. A leading slot (160px) holds the standalone sidebar button and grouped back/forward controls, separated by (16px). The search field fills the flexible middle slot, ranging from (180px) to (560px) wide at (32px) high with a centered (30px) input. There is no location title, Ctrl+K badge or history-dropdown arrow.

After search, thin separators (1px x 20px) divide the view, action and caption groups. View buttons are three (32px x 30px) targets. Actions are five (30px x 30px) targets in this order: hidden-file visibility, sort, info, settings, file actions. Captions are three (26px x 30px) Windows control-area targets. Groups have no internal gap, a (32px) outer height and fine borders.

The Favorites sidebar content and its expanded animated slot both measure (204px); Favorites artwork and file-list icons are (16px). Results occupy the flexible center. The optional information panel is (280px), appears only with a selection, and scrolls independently. A single (30px) footer combines the breadcrumb path, result/selection count and index/access status. List rows are virtualized and unstriped. The application minimum window is (960px x 550px); preserve shrinking and truncation rather than introducing web breakpoints.

Content mode adds its own compact status row above results, using the frontmatter height and metadata role with (12px) horizontal gaps. Location names and search status truncate within the available width. Results use List view; the Matching line column replaces Date Modified at (360px) wide. Icons and Gallery controls are disabled while Content mode is active.

Settings use an (820px x 620px) modal capped at (94%) of window width and (92%) of height, a (54px) header and a (200px) category rail. Categories are Appearance, Files and search, Locations, and Helper. The content scrolls independently with (20px) padding. Separate cards use (16px) internal padding and (12px) corners; labels and controls align left. Default view uses a compact (242px) segmented frame with three (80px) buttons that ends immediately after Gallery.

Settings navigation retains (40px) rows with a (20px) centered icon slot, (18px) artwork and a (20px) text slot separated by (12px). Native popup rows are (28px) logical pixels. The Type menu is compact, bounded from (144px) to (180px) wide. Popup menus omit separator rows and visible scrollbars; wheel scrolling and keyboard navigation remain available when content exceeds the viewport.

## Elevation & Depth

Glass uses GPUI's native `Blurred` background with one neutral root tint at (80%) opacity; sidebar and chrome remain transparent. GPUI owns backdrop composition and its platform fallback. Solid mode requests an opaque background. Native blur/Mica appearance depends on Windows and requires visual verification; API configuration alone is not evidence of visible glass.

Main surfaces have no decorative shadows. The toolbar's compact rounded groups use a neutral vertical gradient, with distinct light/dark endpoints in the frontmatter; search fades its hover surface toward (65%) opacity. Settings and the sort menu use a border and a restrained shadow, recorded in the sidecar. Settings has its own nearly opaque neutral sheet, softly layered cards, and fine-border gradient controls; theme selection and category selection use neutral control fills. Native window rounding is requested through DWM. The sidebar has one user-authorized width transition (160ms) using smoothstep easing, `t*t*(3-2*t)`. Reversal starts at its current width. Request animation frames only while transitioning; no idle animation loop or decorative perpetual motion is part of this system.

## Shapes

Controls have small rounded corners; shared theme radii are in the frontmatter. Toolbar buttons, groups and search use distinct radii. Search remains a rounded rectangle rather than a capsule. Accent swatches are square silhouettes with subtly rounded corners, never circles. Selected swatches carry a contrasting outline. Settings use a rounded outer sheet with separate rounded cards; do not nest cards within cards.

## Components

- **Toolbar:** Segoe Fluent Icons supplies the Windows symbols in normal weight and softer muted gray; tooltips and accessible labels name the actions. Other controls retain licensed Kit SVG artwork, adjusted to stroke (1.5) by the application asset wrapper without vendor edits. Caption glyphs are (11px) inside Windows `WindowControlArea` targets, including red close hover. The hidden-file visibility action uses the document-plus SVG through Kit `Icon::default().path(...)`, preserving inherited paint tint. Info appears selected only while its inspector is visible: the panel preference is enabled and an item is selected. The current view uses a saved-accent wash and normal foreground glyph; unused toolbar space remains draggable. Toolbar buttons explicitly use zero horizontal and vertical padding; icon/SVG content does not shrink inside the target. Kit's outer focus halo is disabled (`Theme.focus_ring = false`), retaining its thin keyboard-focus line (1px) without spilling across neighboring controls.
- **Toolbar state:** view and action buttons share one custom state style: a 14% accent wash for pressed/selected states, normal text for active icons and muted text otherwise. Hidden items, sort, Info and Settings use the same fill as the view switcher. Info is selected only when its inspector is visible.
- **Search:** a flexible rounded rectangle with a subtle neutral gradient, fine border and muted search icon; focus strengthens the border. Query chips live inside the search flow. Content mode adds a compact ghost button beside the search glyph in the prefix, using the `search-content-mode` height and padding, an (11px) Content label and an (11px) SVG X. Its accessible action is Exit content search; activating it returns to filename search.
- **Type and native menus:** the Type chip opens the compact native GPUI type menu and retains its remove action. Search suggestions distinguish Name contains, Content contains and file-type interpretations. The ellipsis control groups All items, Largest files, Largest folders, Search file contents, Type and file actions in the incumbent toolbar. Menus share the approved popup surfaces, (28px) rows, (14%) accent hover/selection and normal foreground text; use no separators or visible scrollbar. The local Kit options preserve wheel and keyboard interaction.
- **Content status:** a transparent row above results uses the `content-status` dimensions and muted metadata role. Up to two configured folder names appear directly; larger sets use a location count. The adjacent summary carries search progress, matches, skipped files, errors and limit status. A tooltip supplies full configured paths and the bounded-search limits: (4 MiB) per file, (8 MiB) expanded document text, (3 seconds) per search and (200) matches, plus skipped-file categories and guidance to narrow the folder when a limit is reached.
- **Navigation:** compact selected sidebar rows use a subdued neutral hover wash and normal foreground text. Generic sidebar line icons use muted foreground when unselected. Windows supplies Favorites artwork, file association icons and generic yellow folders. Folder rows share one native yellow folder icon from the atlas; do not request separate shell images for each folder. Bundled yellow folder and file-kind artwork remain fallbacks.
- **List and previews:** unstriped rows use hover/selection for state. Bounded visible-file shell requests replace fallback artwork asynchronously; supported images and videos request real thumbnails. Folder rows reuse the shared atlas icon. Large selected previews request a (768px) edge; this is source resolution, not a fixed display size. Large folder previews use native Windows artwork of at least (256px), preserving the small atlas icon for rows rather than stretching (32px) artwork. Preserve image aspect ratio. Gallery has one large central preview and a bottom thumbnail strip (155px); its inspector omits the duplicate image. Hide the inspector without a selected item.
- **Settings:** regular (13px) control labels, semibold card titles, section headings and modal title use their frontmatter roles. System, Light and Dark use explicit icon-and-label rows inside their (100px) buttons: non-shrinking (16px) SVGs at stroke (1.5), with an (8px) icon/label gap. Custom children bypass automatic button icon sizing. The glass control is a working On/Off switch. Accent swatches are (24px) rounded squares in (32px) targets; the Kit ColorPicker pipette and six-digit hex Apply control both change the saved accent. Hidden/system visibility uses a native Kit dropdown. Default view is the compact List/Icons/Gallery segmented frame; choosing it changes the current app view immediately and saves the future default. Locations has a real All drives / Custom folders dropdown, custom path input and exclusions input; only Save locations is an accent-filled primary button. Secondary controls, Done and navigation use normal foreground text and neutral gradient/selected surfaces. Shared controls are (34px) high with zero vertical padding and a (16px) line height for regular (13px) labels. Choice selectors keep their labels left aligned with an independently positioned right chevron (12px); action buttons retain centered labels. Secondary ghost text also maps to normal foreground through `accent_foreground`; stronger neutral selected fills remain distinct in both themes. Changes to indexed locations take effect on next launch.
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
