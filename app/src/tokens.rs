//! Design tokens: the one place for type, radii and sizes. Views use these
//! instead of inventing their own values, so every surface matches.
//! Colours live in crate::appearance; motion in crate::motion.
// Views adopt these one surface at a time; unused ones are not dead design.
#![allow(dead_code)]
use gpui::{Pixels, px};

/// Interface text.
pub const FONT_UI: &str = "Geist";
/// Keys, sizes, dates, paths and code.
pub const FONT_MONO: &str = "Geist Mono";

/// Rows in lists, menus, the sidebar; every highlight behind content.
pub const RADIUS_ROW: Pixels = px(7.);
/// Controls: buttons, fields, segmented controls.
pub const RADIUS_CONTROL: Pixels = px(8.);
/// Floating and grouped surfaces: menus, popovers, sheets, cards, toasts.
pub const RADIUS_CARD: Pixels = px(12.);
/// Thumbnails and swatches.
pub const RADIUS_THUMB: Pixels = px(4.);

/// Captions, column headers, key caps.
pub const TEXT_CAPTION: Pixels = px(11.);
/// Secondary text and metadata.
pub const TEXT_SMALL: Pixels = px(12.);
/// Body and rows.
pub const TEXT_BODY: Pixels = px(13.);
/// Sheet and panel titles.
pub const TEXT_TITLE: Pixels = px(15.);
/// Page titles (Settings).
pub const TEXT_PAGE: Pixels = px(20.);

/// Menu and picker rows.
pub const MENU_ROW: Pixels = px(30.);
/// Buttons in sheets and toolbars.
pub const BUTTON_HEIGHT: Pixels = px(28.);
/// Text fields.
pub const FIELD_HEIGHT: Pixels = px(32.);
/// Padding inside floating surfaces.
pub const FLOAT_PADDING: Pixels = px(5.);
