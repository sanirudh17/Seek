//! Icons view options (Ctrl+J): tile size, spacing, label position, the
//! details under each name and thumbnails. Each pane keeps its own, saved in
//! settings by pane id so they survive restarts.
use serde::{Deserialize, Serialize};

pub const SIZE_MIN: u16 = 48;
pub const SIZE_MAX: u16 = 160;
pub const SPACING_MAX: u16 = 32;
/// More than this many remembered panes and the oldest ids are dropped.
pub const REMEMBERED: usize = 64;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Labels {
    #[default]
    Below,
    Right,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct IconView {
    pub size: u16,
    pub spacing: u16,
    pub labels: Labels,
    pub show_size: bool,
    pub show_kind: bool,
    pub show_modified: bool,
    pub thumbnails: bool,
}

impl Default for IconView {
    fn default() -> Self {
        Self {
            size: 96,
            spacing: 8,
            labels: Labels::Below,
            show_size: true,
            show_kind: false,
            show_modified: false,
            thumbnails: true,
        }
    }
}

/// Space around the icon and the label inside a tile.
const PAD: f32 = 8.;
const LABEL: f32 = 18.;
const DETAIL: f32 = 15.;
/// Width of the text beside the icon with labels on the right.
const RIGHT_TEXT: f32 = 168.;

impl IconView {
    /// Untrusted values from settings, brought into range.
    pub fn bounded(self) -> Self {
        Self {
            size: self.size.clamp(SIZE_MIN, SIZE_MAX),
            spacing: self.spacing.min(SPACING_MAX),
            ..self
        }
    }
    pub fn details(self) -> usize {
        [self.show_size, self.show_kind, self.show_modified]
            .into_iter()
            .filter(|on| *on)
            .count()
    }
    pub fn tile_width(self) -> f32 {
        let size = self.size as f32;
        match self.labels {
            Labels::Below => (size + PAD * 3.).max(104.),
            Labels::Right => size + PAD * 3. + RIGHT_TEXT,
        }
    }
    /// One line of tiles, including the gap below it.
    pub fn row_height(self) -> f32 {
        let size = self.size as f32;
        let text = LABEL + DETAIL * self.details() as f32;
        let body = match self.labels {
            Labels::Below => size + 4. + text,
            Labels::Right => size.max(text),
        };
        (body + PAD * 2. + self.spacing as f32).ceil()
    }
    /// How many tiles fit across `available` pixels (at least one).
    pub fn columns(self, available: f32) -> usize {
        let gap = self.spacing as f32;
        // The line's own side padding is 12 px each side.
        (((available - 24. + gap) / (self.tile_width() + gap)).floor() as usize).max(1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bounds_and_geometry_follow_the_options() {
        let wild = IconView {
            size: 9000,
            spacing: 900,
            ..IconView::default()
        }
        .bounded();
        assert_eq!((wild.size, wild.spacing), (SIZE_MAX, SPACING_MAX));
        assert_eq!(
            IconView { size: 1, ..IconView::default() }.bounded().size,
            SIZE_MIN
        );
        let base = IconView::default();
        let bigger = IconView { size: 140, ..base };
        assert!(bigger.row_height() > base.row_height());
        assert!(bigger.columns(1000.) < base.columns(1000.));
        let detailed = IconView {
            show_kind: true,
            show_modified: true,
            ..base
        };
        assert_eq!(detailed.details(), 3);
        assert!(detailed.row_height() > base.row_height());
        let right = IconView {
            labels: Labels::Right,
            ..base
        };
        assert!(right.tile_width() > base.tile_width());
        assert!(right.row_height() < base.row_height());
        assert_eq!(base.columns(10.), 1, "never zero columns");
    }

    #[test]
    fn old_or_partial_settings_take_defaults() {
        let partial: IconView = serde_json::from_str(r#"{"size":120}"#).unwrap();
        assert_eq!(partial.size, 120);
        assert_eq!(partial.spacing, IconView::default().spacing);
        assert!(partial.thumbnails);
    }
}
