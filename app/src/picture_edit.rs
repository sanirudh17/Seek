//! Picture editing engine (Phase 11.9): non-destructive typed edit recipe,
//! editable vector marks (pen, highlighter, arrow, rectangle, ellipse, text,
//! redact), crop/rotate/flip, scaled working copy for large pictures, full-res
//! cancellable export, permanent pixel-level redaction, and safe atomic save.
//!
//! Architecture inspired by Monodrive's `edits.rs` / `marks.rs` (MIT).
use image::{
    DynamicImage, GenericImageView, ImageDecoder, ImageEncoder, ImageFormat, RgbaImage,
};
#[cfg(test)]
use image::Rgba;
use resvg::tiny_skia::{
    BlendMode, Color, FillRule, LineCap, LineJoin, Paint, PathBuilder, Pixmap, Stroke, Transform,
};
use std::{
    fs::{File, OpenOptions},
    io::{BufReader, Write},
    path::{Path, PathBuf},
    sync::{
        Arc, OnceLock,
        atomic::{AtomicBool, AtomicU8, Ordering},
    },
};

/// Hard safety limits so malformed/huge pictures never exhaust memory.
pub const MAX_SIDE: u32 = 20_000;
pub const MAX_PIXELS: u64 = 120_000_000;
/// Longest edge of the interactive working preview bitmap.
pub const WORK_MAX_EDGE: u32 = 2048;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tool {
    Select,
    Crop,
    Pen,
    Highlighter,
    Arrow,
    Rectangle,
    Ellipse,
    Text,
    Redact,
}

impl Tool {
    pub fn label(self) -> &'static str {
        match self {
            Tool::Select => "Select",
            Tool::Crop => "Crop",
            Tool::Pen => "Pen",
            Tool::Highlighter => "Highlight",
            Tool::Arrow => "Arrow",
            Tool::Rectangle => "Rect",
            Tool::Ellipse => "Ellipse",
            Tool::Text => "Text",
            Tool::Redact => "Redact",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ink {
    Red,
    Yellow,
    Blue,
    Black,
}

impl Ink {
    pub const ALL: [Ink; 4] = [Ink::Red, Ink::Yellow, Ink::Blue, Ink::Black];

    pub fn name(self) -> &'static str {
        match self {
            Ink::Red => "Red",
            Ink::Yellow => "Yellow",
            Ink::Blue => "Blue",
            Ink::Black => "Black",
        }
    }

    pub fn rgba(self) -> [u8; 4] {
        match self {
            Ink::Red => [239, 68, 68, 255],
            Ink::Yellow => [250, 204, 21, 255],
            Ink::Blue => [59, 130, 246, 255],
            Ink::Black => [17, 24, 39, 255],
        }
    }

    pub fn hex(self) -> &'static str {
        match self {
            Ink::Red => "#ef4444",
            Ink::Yellow => "#facc15",
            Ink::Blue => "#3b82f6",
            Ink::Black => "#111827",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AspectPreset {
    Free,
    Square,
    FourThree,
    SixteenNine,
}

impl AspectPreset {
    pub const ALL: [AspectPreset; 4] = [
        AspectPreset::Free,
        AspectPreset::Square,
        AspectPreset::FourThree,
        AspectPreset::SixteenNine,
    ];

    pub fn label(self) -> &'static str {
        match self {
            AspectPreset::Free => "Free",
            AspectPreset::Square => "1:1",
            AspectPreset::FourThree => "4:3",
            AspectPreset::SixteenNine => "16:9",
        }
    }

    pub fn ratio(self) -> Option<f32> {
        match self {
            AspectPreset::Free => None,
            AspectPreset::Square => Some(1.0),
            AspectPreset::FourThree => Some(4.0 / 3.0),
            AspectPreset::SixteenNine => Some(16.0 / 9.0),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PointF {
    pub x: f32,
    pub y: f32,
}

impl PointF {
    pub fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RectF {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl RectF {
    pub fn from_points(a: PointF, b: PointF) -> Self {
        let x = a.x.min(b.x);
        let y = a.y.min(b.y);
        let w = (a.x - b.x).abs();
        let h = (a.y - b.y).abs();
        Self { x, y, w, h }
    }

    pub fn center(self) -> PointF {
        PointF::new(self.x + self.w * 0.5, self.y + self.h * 0.5)
    }

    pub fn clamped_to(self, max_w: f32, max_h: f32) -> Self {
        let x = self.x.clamp(0.0, (max_w - 1.0).max(0.0));
        let y = self.y.clamp(0.0, (max_h - 1.0).max(0.0));
        let w = self.w.clamp(1.0, (max_w - x).max(1.0));
        let h = self.h.clamp(1.0, (max_h - y).max(1.0));
        Self { x, y, w, h }
    }

    pub fn corners(self) -> [PointF; 4] {
        [
            PointF::new(self.x, self.y),
            PointF::new(self.x + self.w, self.y),
            PointF::new(self.x + self.w, self.y + self.h),
            PointF::new(self.x, self.y + self.h),
        ]
    }
}

/// A single step in the non-destructive editing recipe. All coordinates are in
/// full-resolution pixel space of the image at the moment this step runs.
#[derive(Clone, Debug, PartialEq)]
pub enum Step {
    Crop {
        rect: RectF,
    },
    /// Clockwise 90-degree turns (normalized mod 4: 1 = 90 CW, 2 = 180, 3 = 90 CCW).
    Rotate {
        quarter_turns: u8,
    },
    Flip {
        vertical: bool,
    },
    Pen {
        points: Vec<PointF>,
        ink: Ink,
        width: f32,
        rotation: f32,
    },
    Highlighter {
        points: Vec<PointF>,
        ink: Ink,
        width: f32,
        rotation: f32,
    },
    Arrow {
        from: PointF,
        to: PointF,
        ink: Ink,
        width: f32,
    },
    Rectangle {
        rect: RectF,
        ink: Ink,
        width: f32,
        rotation: f32,
    },
    Ellipse {
        rect: RectF,
        ink: Ink,
        width: f32,
        rotation: f32,
    },
    Text {
        at: PointF,
        text: String,
        size: f32,
        ink: Ink,
        rotation: f32,
    },
    /// Permanently overwrites underlying pixels with opaque black `[0, 0, 0, 255]`.
    Redact {
        rect: RectF,
        rotation: f32,
    },
}

impl Step {
    pub fn is_transform(&self) -> bool {
        matches!(
            self,
            Step::Crop { .. } | Step::Rotate { .. } | Step::Flip { .. }
        )
    }

    pub fn is_redaction(&self) -> bool {
        matches!(self, Step::Redact { .. })
    }

    pub fn ink(&self) -> Option<Ink> {
        match self {
            Step::Pen { ink, .. }
            | Step::Highlighter { ink, .. }
            | Step::Arrow { ink, .. }
            | Step::Rectangle { ink, .. }
            | Step::Ellipse { ink, .. }
            | Step::Text { ink, .. } => Some(*ink),
            Step::Crop { .. } | Step::Rotate { .. } | Step::Flip { .. } | Step::Redact { .. } => {
                None
            }
        }
    }

    pub fn set_ink(&mut self, next: Ink) -> bool {
        match self {
            Step::Pen { ink, .. }
            | Step::Highlighter { ink, .. }
            | Step::Arrow { ink, .. }
            | Step::Rectangle { ink, .. }
            | Step::Ellipse { ink, .. }
            | Step::Text { ink, .. } => {
                *ink = next;
                true
            }
            _ => false,
        }
    }
}

/// Interactive handle on a selected mark.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Handle {
    /// 0 = TL, 1 = TR, 2 = BR, 3 = BL
    Corner(usize),
    From,
    To,
    Turn,
}

/// Selection frame for a live mark in current full-res pixel coordinates.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MarkFrame {
    pub rect: RectF,
    pub rotation: f32,
    pub endpoints: Option<(PointF, PointF)>,
}

impl MarkFrame {
    pub fn center(self) -> PointF {
        self.rect.center()
    }

    pub fn corners_world(self) -> [PointF; 4] {
        let c = self.center();
        self.rect.corners().map(|pt| rotate_point(pt, c, self.rotation))
    }

    pub fn turn_handle(self, offset: f32) -> PointF {
        let c = self.center();
        let top_mid = PointF::new(self.rect.x + self.rect.w * 0.5, self.rect.y - offset);
        rotate_point(top_mid, c, self.rotation)
    }
}

pub fn rotate_point(pt: PointF, center: PointF, radians: f32) -> PointF {
    if radians.abs() < 1e-6 {
        return pt;
    }
    let (s, c) = radians.sin_cos();
    let dx = pt.x - center.x;
    let dy = pt.y - center.y;
    PointF::new(center.x + dx * c - dy * s, center.y + dx * s + dy * c)
}

/// Index of the first mark that is still live (after the last Crop/Rotate/Flip).
pub fn first_live_mark(steps: &[Step]) -> usize {
    steps
        .iter()
        .rposition(|s| s.is_transform())
        .map_or(0, |idx| idx + 1)
}

pub fn has_redaction(steps: &[Step]) -> bool {
    steps.iter().any(Step::is_redaction)
}

/// Dimensions of the image after applying all transform steps in `steps`.
pub fn current_dimensions(orig_w: u32, orig_h: u32, steps: &[Step]) -> (u32, u32) {
    let mut w = orig_w.max(1);
    let mut h = orig_h.max(1);
    for step in steps {
        match step {
            Step::Crop { rect } => {
                let c = rect.clamped_to(w as f32, h as f32);
                w = (c.w.round() as u32).clamp(1, w);
                h = (c.h.round() as u32).clamp(1, h);
            }
            Step::Rotate { quarter_turns } => {
                if quarter_turns % 2 == 1 {
                    std::mem::swap(&mut w, &mut h);
                }
            }
            Step::Flip { .. } => {}
            _ => {}
        }
    }
    (w, h)
}

/// Estimate the bounding box of a text block in full-res pixels.
pub fn text_bounds(at: PointF, text: &str, size: f32) -> RectF {
    let line_height = size * 1.25;
    let lines: Vec<&str> = if text.is_empty() {
        vec![" "]
    } else {
        text.lines().collect()
    };
    let max_chars = lines
        .iter()
        .map(|l| l.chars().count())
        .max()
        .unwrap_or(1)
        .max(1) as f32;
    let w = (max_chars * size * 0.62 + size * 0.4).max(size * 1.4);
    let h = (lines.len() as f32 * line_height).max(line_height);
    RectF {
        x: at.x,
        y: at.y,
        w,
        h,
    }
}

pub fn mark_frame(step: &Step) -> Option<MarkFrame> {
    match step {
        Step::Pen {
            points,
            width,
            rotation,
            ..
        }
        | Step::Highlighter {
            points,
            width,
            rotation,
            ..
        } => {
            let (mut min_x, mut min_y, mut max_x, mut max_y) =
                (f32::INFINITY, f32::INFINITY, f32::NEG_INFINITY, f32::NEG_INFINITY);
            for pt in points {
                min_x = min_x.min(pt.x);
                min_y = min_y.min(pt.y);
                max_x = max_x.max(pt.x);
                max_y = max_y.max(pt.y);
            }
            if !min_x.is_finite() {
                return None;
            }
            let pad = (*width * 0.6).max(6.0);
            Some(MarkFrame {
                rect: RectF {
                    x: min_x - pad,
                    y: min_y - pad,
                    w: (max_x - min_x + pad * 2.0).max(16.0),
                    h: (max_y - min_y + pad * 2.0).max(16.0),
                },
                rotation: *rotation,
                endpoints: None,
            })
        }
        Step::Arrow { from, to, .. } => Some(MarkFrame {
            rect: RectF::from_points(*from, *to),
            rotation: 0.0,
            endpoints: Some((*from, *to)),
        }),
        Step::Rectangle { rect, rotation, .. }
        | Step::Ellipse { rect, rotation, .. }
        | Step::Redact { rect, rotation } => Some(MarkFrame {
            rect: *rect,
            rotation: *rotation,
            endpoints: None,
        }),
        Step::Text {
            at,
            text,
            size,
            rotation,
            ..
        } => Some(MarkFrame {
            rect: text_bounds(*at, text, *size),
            rotation: *rotation,
            endpoints: None,
        }),
        _ => None,
    }
}

fn dist(a: PointF, b: PointF) -> f32 {
    ((a.x - b.x).powi(2) + (a.y - b.y).powi(2)).sqrt()
}

fn dist_to_segment(p: PointF, a: PointF, b: PointF) -> f32 {
    let dx = b.x - a.x;
    let dy = b.y - a.y;
    let len_sq = dx * dx + dy * dy;
    if len_sq < 1e-6 {
        return dist(p, a);
    }
    let t = (((p.x - a.x) * dx + (p.y - a.y) * dy) / len_sq).clamp(0.0, 1.0);
    dist(p, PointF::new(a.x + t * dx, a.y + t * dy))
}

/// Hit-test live marks (topmost first). `tolerance` is in full-res pixels.
pub fn hit_test_mark(steps: &[Step], pt: PointF, tolerance: f32) -> Option<usize> {
    let start = first_live_mark(steps);
    for idx in (start..steps.len()).rev() {
        let hit = match &steps[idx] {
            Step::Pen {
                points,
                width,
                rotation,
                ..
            }
            | Step::Highlighter {
                points,
                width,
                rotation,
                ..
            } => {
                let Some(frame) = mark_frame(&steps[idx]) else {
                    continue;
                };
                let local = rotate_point(pt, frame.center(), -*rotation);
                let hit_r = (*width * 0.6).max(tolerance);
                if points.len() == 1 {
                    dist(local, points[0]) <= hit_r
                } else {
                    points
                        .windows(2)
                        .any(|w| dist_to_segment(local, w[0], w[1]) <= hit_r)
                }
            }
            Step::Arrow {
                from, to, width, ..
            } => dist_to_segment(pt, *from, *to) <= (*width * 0.8).max(tolerance),
            Step::Rectangle {
                rect,
                width,
                rotation,
                ..
            } => {
                let local = rotate_point(pt, rect.center(), -*rotation);
                let pad = (*width * 0.6).max(tolerance);
                let outer = local.x >= rect.x - pad
                    && local.x <= rect.x + rect.w + pad
                    && local.y >= rect.y - pad
                    && local.y <= rect.y + rect.h + pad;
                let inner = local.x >= rect.x + pad
                    && local.x <= rect.x + rect.w - pad
                    && local.y >= rect.y + pad
                    && local.y <= rect.y + rect.h - pad;
                outer && (!inner || rect.w < pad * 3.0 || rect.h < pad * 3.0)
            }
            Step::Ellipse {
                rect,
                width,
                rotation,
                ..
            } => {
                let c = rect.center();
                let local = rotate_point(pt, c, -*rotation);
                let rx = (rect.w * 0.5).max(1.0);
                let ry = (rect.h * 0.5).max(1.0);
                let pad = (*width * 0.6).max(tolerance);
                let norm_outer = ((local.x - c.x) / (rx + pad)).powi(2)
                    + ((local.y - c.y) / (ry + pad)).powi(2);
                let norm_inner = if rx > pad && ry > pad {
                    ((local.x - c.x) / (rx - pad)).powi(2) + ((local.y - c.y) / (ry - pad)).powi(2)
                } else {
                    0.0
                };
                norm_outer <= 1.0 && norm_inner >= 0.65
            }
            Step::Redact { rect, rotation } => {
                let local = rotate_point(pt, rect.center(), -*rotation);
                local.x >= rect.x - tolerance * 0.5
                    && local.x <= rect.x + rect.w + tolerance * 0.5
                    && local.y >= rect.y - tolerance * 0.5
                    && local.y <= rect.y + rect.h + tolerance * 0.5
            }
            Step::Text {
                at,
                text,
                size,
                rotation,
                ..
            } => {
                let rect = text_bounds(*at, text, *size);
                let local = rotate_point(pt, rect.center(), -*rotation);
                local.x >= rect.x - tolerance
                    && local.x <= rect.x + rect.w + tolerance
                    && local.y >= rect.y - tolerance
                    && local.y <= rect.y + rect.h + tolerance
            }
            _ => false,
        };
        if hit {
            return Some(idx);
        }
    }
    None
}

/// Hit-test resize/rotate handles of a selected mark.
pub fn hit_test_handle(
    frame: MarkFrame,
    pt: PointF,
    handle_radius: f32,
    turn_offset: f32,
) -> Option<Handle> {
    if let Some((from, to)) = frame.endpoints {
        if dist(pt, from) <= handle_radius {
            return Some(Handle::From);
        }
        if dist(pt, to) <= handle_radius {
            return Some(Handle::To);
        }
        return None;
    }
    if dist(pt, frame.turn_handle(turn_offset)) <= handle_radius {
        return Some(Handle::Turn);
    }
    for (i, corner) in frame.corners_world().into_iter().enumerate() {
        if dist(pt, corner) <= handle_radius {
            return Some(Handle::Corner(i));
        }
    }
    None
}

pub fn translate_mark(step: &mut Step, dx: f32, dy: f32) {
    match step {
        Step::Pen { points, .. } | Step::Highlighter { points, .. } => {
            for pt in points {
                pt.x += dx;
                pt.y += dy;
            }
        }
        Step::Arrow { from, to, .. } => {
            from.x += dx;
            from.y += dy;
            to.x += dx;
            to.y += dy;
        }
        Step::Rectangle { rect, .. } | Step::Ellipse { rect, .. } | Step::Redact { rect, .. } => {
            rect.x += dx;
            rect.y += dy;
        }
        Step::Text { at, .. } => {
            at.x += dx;
            at.y += dy;
        }
        _ => {}
    }
}

pub fn resize_mark_handle(step: &mut Step, handle: Handle, pt: PointF) {
    match (step, handle) {
        (Step::Arrow { from, .. }, Handle::From) => *from = pt,
        (Step::Arrow { to, .. }, Handle::To) => *to = pt,
        (
            Step::Rectangle { rect, rotation, .. }
            | Step::Ellipse { rect, rotation, .. }
            | Step::Redact { rect, rotation },
            Handle::Corner(corner),
        ) => {
            let center = rect.center();
            let local = rotate_point(pt, center, -*rotation);
            let mut x0 = rect.x;
            let mut y0 = rect.y;
            let mut x1 = rect.x + rect.w;
            let mut y1 = rect.y + rect.h;
            match corner {
                0 => {
                    x0 = local.x;
                    y0 = local.y;
                }
                1 => {
                    x1 = local.x;
                    y0 = local.y;
                }
                2 => {
                    x1 = local.x;
                    y1 = local.y;
                }
                3 => {
                    x0 = local.x;
                    y1 = local.y;
                }
                _ => {}
            }
            let min_size = 8.0;
            let nx = x0.min(x1);
            let ny = y0.min(y1);
            let nw = (x1 - x0).abs().max(min_size);
            let nh = (y1 - y0).abs().max(min_size);
            *rect = RectF {
                x: nx,
                y: ny,
                w: nw,
                h: nh,
            };
        }
        (
            Step::Text {
                at,
                text,
                size,
                rotation,
                ..
            },
            Handle::Corner(_),
        ) => {
            let bounds = text_bounds(*at, text, *size);
            let local = rotate_point(pt, bounds.center(), -*rotation);
            let new_h = (local.y - at.y).abs().max(16.0);
            let lines = text.lines().count().max(1) as f32;
            *size = (new_h / (lines * 1.25)).clamp(12.0, 240.0);
        }
        (
            Step::Pen {
                points, rotation, ..
            }
            | Step::Highlighter {
                points, rotation, ..
            },
            Handle::Corner(_),
        ) => {
            if points.len() < 2 {
                return;
            }
            let (mut min_x, mut min_y, mut max_x, mut max_y) =
                (f32::INFINITY, f32::INFINITY, f32::NEG_INFINITY, f32::NEG_INFINITY);
            for p in points.iter() {
                min_x = min_x.min(p.x);
                min_y = min_y.min(p.y);
                max_x = max_x.max(p.x);
                max_y = max_y.max(p.y);
            }
            let cx = (min_x + max_x) * 0.5;
            let cy = (min_y + max_y) * 0.5;
            let half_w = ((max_x - min_x) * 0.5).max(4.0);
            let half_h = ((max_y - min_y) * 0.5).max(4.0);
            let local = rotate_point(pt, PointF::new(cx, cy), -*rotation);
            let sx = ((local.x - cx).abs() / half_w).clamp(0.25, 4.0);
            let sy = ((local.y - cy).abs() / half_h).clamp(0.25, 4.0);
            let s = sx.max(sy);
            for p in points.iter_mut() {
                p.x = cx + (p.x - cx) * s;
                p.y = cy + (p.y - cy) * s;
            }
        }
        (step, Handle::Turn) => {
            if let Some(frame) = mark_frame(step) {
                let c = frame.center();
                let angle = (pt.y - c.y).atan2(pt.x - c.x) + std::f32::consts::FRAC_PI_2;
                match step {
                    Step::Pen { rotation, .. }
                    | Step::Highlighter { rotation, .. }
                    | Step::Rectangle { rotation, .. }
                    | Step::Ellipse { rotation, .. }
                    | Step::Text { rotation, .. }
                    | Step::Redact { rotation, .. } => *rotation = angle,
                    _ => {}
                }
            }
        }
        _ => {}
    }
}

/// Output format for saving.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OutputFormat {
    Png,
    Jpeg,
    Webp,
    Bmp,
    Gif,
}

impl OutputFormat {
    pub fn from_path(path: &Path) -> Self {
        match path
            .extension()
            .and_then(|e| e.to_str())
            .map(str::to_ascii_lowercase)
            .as_deref()
        {
            Some("jpg" | "jpeg") => OutputFormat::Jpeg,
            Some("webp") => OutputFormat::Webp,
            Some("bmp") => OutputFormat::Bmp,
            Some("gif") => OutputFormat::Gif,
            // PNG, HEIC/HEIF (export as PNG), or unknown
            _ => OutputFormat::Png,
        }
    }

    pub fn extension(self) -> &'static str {
        match self {
            OutputFormat::Png => "png",
            OutputFormat::Jpeg => "jpg",
            OutputFormat::Webp => "webp",
            OutputFormat::Bmp => "bmp",
            OutputFormat::Gif => "gif",
        }
    }
}

/// Decoded picture metadata + working copy ready for interactive editing.
pub struct LoadedPicture {
    pub path: PathBuf,
    pub original_size: (u32, u32),
    pub working_base: RgbaImage,
    pub work_scale: f32,
    pub is_heif: bool,
}

/// Check if a `.gif` has more than one frame; animated GIFs cannot be edited.
fn check_animated_gif(path: &Path) -> Result<(), String> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase);
    if ext.as_deref() != Some("gif") {
        return Ok(());
    }
    let file = File::open(path).map_err(|_| "This picture can't be opened.".to_string())?;
    let decoder = image::codecs::gif::GifDecoder::new(BufReader::new(file))
        .map_err(|_| "This GIF can't be read.".to_string())?;
    let mut frames = image::AnimationDecoder::into_frames(decoder);
    let _first = frames.next();
    if frames.next().is_some() {
        return Err("Animated GIFs cannot be edited.".into());
    }
    Ok(())
}

pub type DecodedPicture = (RgbaImage, Option<Vec<u8>>, OutputFormat, bool);

/// Decode a picture at full resolution, applying EXIF orientation and checking
/// safety caps and animated GIFs.
pub fn decode_full(path: &Path) -> Result<DecodedPicture, String> {
    if crate::image_tools::placeholder(path) {
        return Err("Download this cloud file before editing it.".into());
    }
    check_animated_gif(path)?;
    let is_heif = crate::image_tools::heif(path);
    let format = OutputFormat::from_path(path);
    if is_heif {
        let img = crate::image_tools::decode_heif(path)?;
        let (w, h) = img.dimensions();
        if w > MAX_SIDE || h > MAX_SIDE || w as u64 * h as u64 > MAX_PIXELS {
            return Err("This picture is too large to edit here.".into());
        }
        return Ok((img.to_rgba8(), None, OutputFormat::Png, true));
    }

    let mut reader = image::ImageReader::open(path)
        .map_err(|_| "This picture can't be opened.".to_string())?
        .with_guessed_format()
        .map_err(|_| "This picture can't be opened.".to_string())?;
    reader.no_limits();
    let mut decoder = reader
        .into_decoder()
        .map_err(|_| "This picture format isn't supported for editing.".to_string())?;
    let (w, h) = decoder.dimensions();
    if w == 0 || h == 0 {
        return Err("This picture is empty.".into());
    }
    if w > MAX_SIDE || h > MAX_SIDE || w as u64 * h as u64 > MAX_PIXELS {
        return Err("This picture is too large to edit here.".into());
    }
    let orientation = decoder
        .orientation()
        .unwrap_or(image::metadata::Orientation::NoTransforms);
    let exif = decoder.exif_metadata().ok().flatten();
    let mut img = DynamicImage::from_decoder(decoder)
        .map_err(|_| "This picture couldn't be decoded.".to_string())?;
    img.apply_orientation(orientation);
    Ok((img.to_rgba8(), exif, format, false))
}

/// Load a picture for interactive editing: scales down large pictures (>2048px)
/// to a snappy working copy while remembering full dimensions for 100% export.
pub fn load_for_editing(path: &Path) -> Result<LoadedPicture, String> {
    let (full, _exif, _format, is_heif) = decode_full(path)?;
    let (w, h) = full.dimensions();
    let longest = w.max(h);
    let (working_base, work_scale) = if longest > WORK_MAX_EDGE {
        let scale = WORK_MAX_EDGE as f32 / longest as f32;
        let nw = ((w as f32 * scale).round() as u32).max(1);
        let nh = ((h as f32 * scale).round() as u32).max(1);
        let resized = image::imageops::resize(&full, nw, nh, image::imageops::FilterType::Triangle);
        (resized, nw as f32 / w as f32)
    } else {
        (full, 1.0)
    };
    Ok(LoadedPicture {
        path: path.to_path_buf(),
        original_size: (w, h),
        working_base,
        work_scale,
        is_heif,
    })
}

fn bundled_fontdb() -> &'static Arc<resvg::usvg::fontdb::Database> {
    static DB: OnceLock<Arc<resvg::usvg::fontdb::Database>> = OnceLock::new();
    DB.get_or_init(|| {
        let mut db = resvg::usvg::fontdb::Database::new();
        db.load_font_data(include_bytes!("../assets/fonts/Geist-SemiBold.ttf").to_vec());
        db.load_font_data(include_bytes!("../assets/fonts/Geist.ttf").to_vec());
        db.set_sans_serif_family("Geist");
        Arc::new(db)
    })
}

fn rgba_to_pixmap(img: &RgbaImage) -> Option<Pixmap> {
    let (w, h) = img.dimensions();
    let mut pm = Pixmap::new(w.max(1), h.max(1))?;
    let dst = pm.data_mut();
    for (src_px, dst_px) in img.as_raw().chunks_exact(4).zip(dst.chunks_exact_mut(4)) {
        let a = src_px[3] as u16;
        if a == 255 {
            dst_px.copy_from_slice(src_px);
        } else if a == 0 {
            dst_px.copy_from_slice(&[0, 0, 0, 0]);
        } else {
            // tiny_skia Pixmap expects premultiplied RGBA.
            dst_px[0] = ((src_px[0] as u16 * a + 127) / 255) as u8;
            dst_px[1] = ((src_px[1] as u16 * a + 127) / 255) as u8;
            dst_px[2] = ((src_px[2] as u16 * a + 127) / 255) as u8;
            dst_px[3] = a as u8;
        }
    }
    Some(pm)
}

fn pixmap_to_rgba(pm: &Pixmap) -> RgbaImage {
    let w = pm.width();
    let h = pm.height();
    let mut out = vec![0u8; (w * h * 4) as usize];
    for (src_px, dst_px) in pm.data().chunks_exact(4).zip(out.chunks_exact_mut(4)) {
        let a = src_px[3];
        if a == 255 {
            dst_px.copy_from_slice(src_px);
        } else if a == 0 {
            dst_px.copy_from_slice(&[0, 0, 0, 0]);
        } else {
            let af = a as f32 / 255.0;
            dst_px[0] = ((src_px[0] as f32 / af).round().clamp(0.0, 255.0)) as u8;
            dst_px[1] = ((src_px[1] as f32 / af).round().clamp(0.0, 255.0)) as u8;
            dst_px[2] = ((src_px[2] as f32 / af).round().clamp(0.0, 255.0)) as u8;
            dst_px[3] = a;
        }
    }
    RgbaImage::from_raw(w, h, out).expect("pixmap dimensions match buffer")
}

fn make_paint(rgba: [u8; 4], alpha_mul: f32, blend_mode: BlendMode, anti_alias: bool) -> Paint<'static> {
    let mut paint = Paint::default();
    let a = ((rgba[3] as f32 * alpha_mul).round().clamp(0.0, 255.0)) as u8;
    paint.set_color(Color::from_rgba8(rgba[0], rgba[1], rgba[2], a));
    paint.blend_mode = blend_mode;
    paint.anti_alias = anti_alias;
    paint
}

fn rotate_transform(cx: f32, cy: f32, radians: f32) -> Transform {
    if radians.abs() < 1e-6 {
        return Transform::identity();
    }
    Transform::from_rotate_at(radians.to_degrees(), cx, cy)
}

fn render_text_mark(pm: &mut Pixmap, at: PointF, text: &str, size: f32, ink: Ink, rotation: f32) {
    if text.trim().is_empty() {
        return;
    }
    let bounds = text_bounds(at, text, size);
    let pad = (size * 0.4).ceil();
    let box_w = ((bounds.w + pad * 2.0).ceil() as u32).clamp(1, pm.width().max(64));
    let box_h = ((bounds.h + pad * 2.0).ceil() as u32).clamp(1, pm.height().max(64));
    let mut tspans = String::new();
    let line_height = size * 1.25;
    for (idx, line) in text.lines().enumerate() {
        let escaped = line
            .replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;");
        let y = pad + size + idx as f32 * line_height;
        tspans.push_str(&format!(
            r#"<text x="{pad:.1}" y="{y:.1}" font-family="Geist, sans-serif" font-size="{size:.1}" font-weight="600" fill="{color}">{escaped}</text>"#,
            color = ink.hex()
        ));
    }
    let svg = format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{box_w}" height="{box_h}" viewBox="0 0 {box_w} {box_h}">{tspans}</svg>"#
    );
    let options = resvg::usvg::Options {
        fontdb: bundled_fontdb().clone(),
        ..Default::default()
    };
    let Ok(tree) = resvg::usvg::Tree::from_str(&svg, &options) else {
        return;
    };
    let cx = bounds.center().x;
    let cy = bounds.center().y;
    let tr = Transform::from_translate(at.x - pad, at.y - pad)
        .post_concat(rotate_transform(cx, cy, rotation));
    resvg::render(&tree, tr, &mut pm.as_mut());
}

fn render_mark(pm: &mut Pixmap, step: &Step, scale: f32) {
    let s = scale.max(1e-4);
    match step {
        Step::Pen {
            points,
            ink,
            width,
            rotation,
        }
        | Step::Highlighter {
            points,
            ink,
            width,
            rotation,
        } => {
            if points.is_empty() {
                return;
            }
            let is_hl = matches!(step, Step::Highlighter { .. });
            let alpha = if is_hl { 0.38 } else { 1.0 };
            let mut pb = PathBuilder::new();
            pb.move_to(points[0].x * s, points[0].y * s);
            if points.len() == 1 {
                pb.line_to(points[0].x * s + 0.1, points[0].y * s);
            } else {
                for pt in &points[1..] {
                    pb.line_to(pt.x * s, pt.y * s);
                }
            }
            let Some(path) = pb.finish() else { return };
            let stroke = Stroke {
                width: (*width * s).max(1.5),
                line_cap: LineCap::Round,
                line_join: LineJoin::Round,
                ..Default::default()
            };
            let frame_center = mark_frame(step)
                .map(|f| PointF::new(f.center().x * s, f.center().y * s))
                .unwrap_or_else(|| PointF::new(points[0].x * s, points[0].y * s));
            let tr = rotate_transform(frame_center.x, frame_center.y, *rotation);
            let paint = make_paint(ink.rgba(), alpha, BlendMode::SourceOver, true);
            pm.stroke_path(&path, &paint, &stroke, tr, None);
        }
        Step::Arrow {
            from,
            to,
            ink,
            width,
        } => {
            let fx = from.x * s;
            let fy = from.y * s;
            let tx = to.x * s;
            let ty = to.y * s;
            let dx = tx - fx;
            let dy = ty - fy;
            let len = (dx * dx + dy * dy).sqrt();
            if len < 1.0 {
                return;
            }
            let ux = dx / len;
            let uy = dy / len;
            let sw = (*width * s).max(2.0);
            let head_len = (sw * 3.8).min(len * 0.55).max(10.0);
            let head_w = head_len * 0.58;
            let base_x = tx - ux * head_len;
            let base_y = ty - uy * head_len;
            let px = -uy;
            let py = ux;

            let mut shaft = PathBuilder::new();
            shaft.move_to(fx, fy);
            shaft.line_to(tx - ux * (head_len * 0.65), ty - uy * (head_len * 0.65));
            if let Some(path) = shaft.finish() {
                let stroke = Stroke {
                    width: sw,
                    line_cap: LineCap::Round,
                    line_join: LineJoin::Round,
                    ..Default::default()
                };
                let paint = make_paint(ink.rgba(), 1.0, BlendMode::SourceOver, true);
                pm.stroke_path(&path, &paint, &stroke, Transform::identity(), None);
            }

            let mut head = PathBuilder::new();
            head.move_to(tx, ty);
            head.line_to(base_x + px * head_w, base_y + py * head_w);
            head.line_to(base_x - px * head_w, base_y - py * head_w);
            head.close();
            if let Some(path) = head.finish() {
                let paint = make_paint(ink.rgba(), 1.0, BlendMode::SourceOver, true);
                pm.fill_path(&path, &paint, FillRule::Winding, Transform::identity(), None);
            }
        }
        Step::Rectangle {
            rect,
            ink,
            width,
            rotation,
        } => {
            let rx = rect.x * s;
            let ry = rect.y * s;
            let rw = (rect.w * s).max(1.0);
            let rh = (rect.h * s).max(1.0);
            let Some(sk_rect) = resvg::tiny_skia::Rect::from_xywh(rx, ry, rw, rh) else {
                return;
            };
            let path = PathBuilder::from_rect(sk_rect);
            let stroke = Stroke {
                width: (*width * s).max(2.0),
                line_join: LineJoin::Round,
                ..Default::default()
            };
            let tr = rotate_transform(rx + rw * 0.5, ry + rh * 0.5, *rotation);
            let paint = make_paint(ink.rgba(), 1.0, BlendMode::SourceOver, true);
            pm.stroke_path(&path, &paint, &stroke, tr, None);
        }
        Step::Ellipse {
            rect,
            ink,
            width,
            rotation,
        } => {
            let rx = rect.x * s;
            let ry = rect.y * s;
            let rw = (rect.w * s).max(1.0);
            let rh = (rect.h * s).max(1.0);
            let Some(sk_rect) = resvg::tiny_skia::Rect::from_xywh(rx, ry, rw, rh) else {
                return;
            };
            let Some(path) = PathBuilder::from_oval(sk_rect) else {
                return;
            };
            let stroke = Stroke {
                width: (*width * s).max(2.0),
                ..Default::default()
            };
            let tr = rotate_transform(rx + rw * 0.5, ry + rh * 0.5, *rotation);
            let paint = make_paint(ink.rgba(), 1.0, BlendMode::SourceOver, true);
            pm.stroke_path(&path, &paint, &stroke, tr, None);
        }
        Step::Text {
            at,
            text,
            size,
            ink,
            rotation,
        } => {
            render_text_mark(
                pm,
                PointF::new(at.x * s, at.y * s),
                text,
                (*size * s).max(8.0),
                *ink,
                *rotation,
            );
        }
        Step::Redact { rect, rotation } => {
            // Permanent pixel-level solid opaque black overwrite (`BlendMode::Source`).
            let rx = rect.x * s;
            let ry = rect.y * s;
            let rw = (rect.w * s).max(1.0);
            let rh = (rect.h * s).max(1.0);
            let Some(sk_rect) = resvg::tiny_skia::Rect::from_xywh(rx, ry, rw, rh) else {
                return;
            };
            let path = PathBuilder::from_rect(sk_rect);
            let tr = rotate_transform(rx + rw * 0.5, ry + rh * 0.5, *rotation);
            let paint = make_paint([0, 0, 0, 255], 1.0, BlendMode::Source, false);
            pm.fill_path(&path, &paint, FillRule::Winding, tr, None);
        }
        _ => {}
    }
}

/// Replay `steps` onto `base`. When rendering the scaled working copy, pass
/// `scale = work_scale`; when exporting at 100% resolution, pass `scale = 1.0`.
/// Optional `cancel` / `progress` allow clean off-thread cancellation.
pub fn apply_recipe(
    base: &RgbaImage,
    steps: &[Step],
    scale: f32,
    cancel: Option<&AtomicBool>,
    progress: Option<&AtomicU8>,
) -> Result<RgbaImage, String> {
    let mut img = base.clone();
    let total = steps.len().max(1);
    let mut pm: Option<Pixmap> = None;

    for (idx, step) in steps.iter().enumerate() {
        if cancel.is_some_and(|c| c.load(Ordering::Relaxed)) {
            return Err("Export cancelled.".into());
        }
        if let Some(p) = progress {
            let pct = 15 + ((idx * 65) / total) as u8;
            p.store(pct.min(80), Ordering::Relaxed);
        }
        match step {
            Step::Crop { rect } => {
                if let Some(active) = pm.take() {
                    img = pixmap_to_rgba(&active);
                }
                let (w, h) = img.dimensions();
                let sx = (rect.x * scale).round().clamp(0.0, (w - 1) as f32) as u32;
                let sy = (rect.y * scale).round().clamp(0.0, (h - 1) as f32) as u32;
                let sw = (rect.w * scale).round().clamp(1.0, (w - sx) as f32) as u32;
                let sh = (rect.h * scale).round().clamp(1.0, (h - sy) as f32) as u32;
                img = image::imageops::crop_imm(&img, sx, sy, sw, sh).to_image();
            }
            Step::Rotate { quarter_turns } => {
                if let Some(active) = pm.take() {
                    img = pixmap_to_rgba(&active);
                }
                img = match quarter_turns % 4 {
                    1 => image::imageops::rotate90(&img),
                    2 => image::imageops::rotate180(&img),
                    3 => image::imageops::rotate270(&img),
                    _ => img,
                };
            }
            Step::Flip { vertical } => {
                if let Some(active) = pm.take() {
                    img = pixmap_to_rgba(&active);
                }
                img = if *vertical {
                    image::imageops::flip_vertical(&img)
                } else {
                    image::imageops::flip_horizontal(&img)
                };
            }
            mark => {
                let target = match pm.as_mut() {
                    Some(existing) => existing,
                    None => {
                        let created = rgba_to_pixmap(&img)
                            .ok_or_else(|| "Could not allocate drawing surface.".to_string())?;
                        pm.insert(created)
                    }
                };
                render_mark(target, mark, scale);
            }
        }
    }

    if let Some(active) = pm.take() {
        img = pixmap_to_rgba(&active);
    }
    Ok(img)
}

/// Convert an `RgbaImage` to a GPUI `RenderImage` (BGRA byte order).
pub fn to_render_image(img: &RgbaImage) -> Arc<gpui::RenderImage> {
    let (w, h) = img.dimensions();
    let mut bgra = RgbaImage::new(w, h);
    for (src, dst) in img
        .as_raw()
        .chunks_exact(4)
        .zip(bgra.as_flat_samples_mut().samples.chunks_exact_mut(4))
    {
        dst[0] = src[2];
        dst[1] = src[1];
        dst[2] = src[0];
        dst[3] = src[3];
    }
    Arc::new(gpui::RenderImage::new(vec![image::Frame::new(bgra)]))
}

fn encode_image_bytes(
    img: &RgbaImage,
    format: OutputFormat,
    exif: Option<&[u8]>,
) -> Result<Vec<u8>, String> {
    let mut buf = Vec::new();
    match format {
        OutputFormat::Png => {
            let mut enc = image::codecs::png::PngEncoder::new(&mut buf);
            if let Some(raw) = exif {
                let _ = enc.set_exif_metadata(raw.to_vec());
            }
            enc.write_image(
                img.as_raw(),
                img.width(),
                img.height(),
                image::ExtendedColorType::Rgba8,
            )
            .map_err(|e| format!("Could not encode PNG: {e}"))?;
        }
        OutputFormat::Jpeg => {
            let rgb = DynamicImage::ImageRgba8(img.clone()).to_rgb8();
            let mut enc = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut buf, 92);
            if let Some(raw) = exif {
                let _ = enc.set_exif_metadata(raw.to_vec());
            }
            enc.write_image(
                rgb.as_raw(),
                rgb.width(),
                rgb.height(),
                image::ExtendedColorType::Rgb8,
            )
            .map_err(|e| format!("Could not encode JPEG: {e}"))?;
        }
        OutputFormat::Webp => {
            let mut enc = image::codecs::webp::WebPEncoder::new_lossless(&mut buf);
            if let Some(raw) = exif {
                let _ = enc.set_exif_metadata(raw.to_vec());
            }
            enc.write_image(
                img.as_raw(),
                img.width(),
                img.height(),
                image::ExtendedColorType::Rgba8,
            )
            .map_err(|e| format!("Could not encode WebP: {e}"))?;
        }
        OutputFormat::Bmp => {
            let mut cursor = std::io::Cursor::new(&mut buf);
            DynamicImage::ImageRgba8(img.clone())
                .write_to(&mut cursor, ImageFormat::Bmp)
                .map_err(|e| format!("Could not encode BMP: {e}"))?;
        }
        OutputFormat::Gif => {
            let mut cursor = std::io::Cursor::new(&mut buf);
            DynamicImage::ImageRgba8(img.clone())
                .write_to(&mut cursor, ImageFormat::Gif)
                .map_err(|e| format!("Could not encode GIF: {e}"))?;
        }
    }
    Ok(buf)
}

/// Reserve a unique copy path beside `source` (`name edited.ext`, `name edited 2.ext`, ...).
pub fn reserve_copy_file(source: &Path, format: OutputFormat) -> Result<(PathBuf, File), String> {
    let parent = source
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .ok_or_else(|| "Cannot determine destination folder.".to_string())?;
    let stem = source
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("picture");
    let ext = format.extension();
    for attempt in 1..=9999u32 {
        let candidate_name = if attempt == 1 {
            format!("{stem} edited.{ext}")
        } else {
            format!("{stem} edited {attempt}.{ext}")
        };
        let candidate = parent.join(candidate_name);
        match OpenOptions::new().write(true).create_new(true).open(&candidate) {
            Ok(file) => return Ok((candidate, file)),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(format!("Could not create copy: {e}")),
        }
    }
    Err("Too many edited copies already exist in this folder.".into())
}

/// Write encoded bytes cleanly to a new copy beside `source`. If writing fails,
/// removes the incomplete copy and leaves `source` untouched.
pub fn write_copy_bytes(
    source: &Path,
    format: OutputFormat,
    bytes: &[u8],
) -> Result<PathBuf, String> {
    let (dest, mut file) = reserve_copy_file(source, format)?;
    let write_res = file
        .write_all(bytes)
        .and_then(|_| file.sync_all())
        .map_err(|e| format!("Could not write copy: {e}"));
    drop(file);
    if let Err(err) = write_res {
        let _ = std::fs::remove_file(&dest);
        return Err(err);
    }
    Ok(dest)
}

/// Atomically replace `target` with `bytes` via a sibling staging temp file.
/// If any step fails, removes the staging file and leaves `target` untouched.
pub fn replace_original_bytes(target: &Path, bytes: &[u8]) -> Result<(), String> {
    if !target.exists() {
        return Err("The original file no longer exists.".into());
    }
    let parent = target
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .ok_or_else(|| "Cannot determine file folder.".to_string())?;
    let name = target
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("picture");
    let pid = std::process::id();
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let temp = parent.join(format!(".{name}.seek-edit-{pid}-{nonce}.tmp"));

    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temp)
        .map_err(|e| format!("Could not stage edited picture: {e}"))?;
    if let Err(e) = file.write_all(bytes).and_then(|_| file.sync_all()) {
        drop(file);
        let _ = std::fs::remove_file(&temp);
        return Err(format!("Could not write staged picture: {e}"));
    }
    drop(file);

    if let Err(err) = atomic_replace_file(&temp, target) {
        let _ = std::fs::remove_file(&temp);
        return Err(err);
    }
    Ok(())
}

fn wide(path: &Path) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    path.as_os_str().encode_wide().chain(Some(0)).collect()
}

fn atomic_replace_file(replacement: &Path, target: &Path) -> Result<(), String> {
    use windows_sys::Win32::Storage::FileSystem::{
        MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW, REPLACEFILE_IGNORE_MERGE_ERRORS,
        ReplaceFileW,
    };
    let dst = wide(target);
    let src = wide(replacement);
    let ok = unsafe {
        ReplaceFileW(
            dst.as_ptr(),
            src.as_ptr(),
            std::ptr::null(),
            REPLACEFILE_IGNORE_MERGE_ERRORS,
            std::ptr::null(),
            std::ptr::null(),
        )
    };
    if ok != 0 {
        return Ok(());
    }
    let fallback = unsafe {
        MoveFileExW(
            src.as_ptr(),
            dst.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    };
    if fallback != 0 {
        return Ok(());
    }
    let err = std::io::Error::last_os_error();
    Err(format!("Could not replace original file: {err}"))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SaveMode {
    SaveCopy,
    ReplaceOriginal,
}

#[derive(Clone, Debug)]
pub struct ExportRequest {
    pub source: PathBuf,
    pub steps: Vec<Step>,
    pub mode: SaveMode,
    pub strip_metadata: bool,
}

/// Full-resolution off-thread export pipeline.
pub fn export_full(
    req: &ExportRequest,
    cancel: &AtomicBool,
    progress: &AtomicU8,
) -> Result<PathBuf, String> {
    progress.store(5, Ordering::Relaxed);
    if cancel.load(Ordering::Relaxed) {
        return Err("Export cancelled.".into());
    }
    let (full, exif, format, is_heif) = decode_full(&req.source)?;
    progress.store(15, Ordering::Relaxed);

    let rendered = apply_recipe(&full, &req.steps, 1.0, Some(cancel), Some(progress))?;
    if cancel.load(Ordering::Relaxed) {
        return Err("Export cancelled.".into());
    }
    progress.store(82, Ordering::Relaxed);

    // Redaction always forces metadata stripping.
    let must_strip = req.strip_metadata || has_redaction(&req.steps);
    let out_exif = if must_strip { None } else { exif.as_deref() };

    let effective_mode = if is_heif && req.mode == SaveMode::ReplaceOriginal {
        // Cannot encode back to HEIC without platform encoder; save a PNG copy safely.
        SaveMode::SaveCopy
    } else {
        req.mode
    };
    let bytes = encode_image_bytes(&rendered, format, out_exif)?;
    progress.store(92, Ordering::Relaxed);

    if cancel.load(Ordering::Relaxed) {
        return Err("Export cancelled.".into());
    }

    let saved_path = match effective_mode {
        SaveMode::SaveCopy => write_copy_bytes(&req.source, format, &bytes)?,
        SaveMode::ReplaceOriginal => {
            replace_original_bytes(&req.source, &bytes)?;
            req.source.clone()
        }
    };
    progress.store(100, Ordering::Relaxed);
    Ok(saved_path)
}

/// Constrain a crop rectangle to an optional aspect ratio within `(img_w, img_h)`.
pub fn apply_aspect_preset(rect: RectF, preset: AspectPreset, img_w: f32, img_h: f32) -> RectF {
    let Some(ratio) = preset.ratio() else {
        return rect.clamped_to(img_w, img_h);
    };
    let cx = rect.x + rect.w * 0.5;
    let cy = rect.y + rect.h * 0.5;
    let mut w = rect.w.max(16.0);
    let mut h = w / ratio;
    if h > img_h {
        h = img_h;
        w = h * ratio;
    }
    if w > img_w {
        w = img_w;
        h = w / ratio;
    }
    let x = (cx - w * 0.5).clamp(0.0, (img_w - w).max(0.0));
    let y = (cy - h * 0.5).clamp(0.0, (img_h - h).max(0.0));
    RectF { x, y, w, h }
}

/// Helper to create a test pixel with known non-black color.
#[cfg(test)]
fn make_bright_image(w: u32, h: u32) -> RgbaImage {
    RgbaImage::from_fn(w, h, |x, y| {
        Rgba([
            ((x * 7 + 80) % 200 + 40) as u8,
            ((y * 11 + 90) % 180 + 50) as u8,
            180,
            255,
        ])
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redaction_permanently_overwrites_underlying_pixels_and_strips_exif() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("secret.jpg");
        let bright = make_bright_image(120, 80);

        // Write a JPEG containing real EXIF APP1 bytes so we verify stripping too.
        let fake_exif: Vec<u8> = b"MM\x00*\x00\x00\x00\x08\x00\x00\x00\x00\x00\x00".to_vec();
        let bytes = encode_image_bytes(&bright, OutputFormat::Jpeg, Some(&fake_exif)).unwrap();
        std::fs::write(&src, &bytes).unwrap();

        // Confirm EXIF is actually present in the source file.
        let (_, src_exif, _, _) = decode_full(&src).unwrap();
        assert!(src_exif.is_some(), "source JPEG should carry EXIF metadata");

        // Export with Redact step (even with strip_metadata=false, redaction forces stripping).
        let cancel = AtomicBool::new(false);
        let progress = AtomicU8::new(0);
        let redact_rect = RectF {
            x: 20.0,
            y: 15.0,
            w: 50.0,
            h: 30.0,
        };
        let out_path = export_full(
            &ExportRequest {
                source: src.clone(),
                steps: vec![Step::Redact {
                    rect: redact_rect,
                    rotation: 0.0,
                }],
                mode: SaveMode::SaveCopy,
                strip_metadata: false,
            },
            &cancel,
            &progress,
        )
        .unwrap();

        // Reload saved file from disk and inspect pixels and metadata.
        let (reloaded, out_exif, _, _) = decode_full(&out_path).unwrap();
        assert!(
            out_exif.is_none(),
            "EXIF metadata MUST be stripped when a picture is redacted"
        );
        // Inside the redacted area (inset past JPEG 8x8 DCT boundary blocks):
        for y in 24..38 {
            for x in 28..62 {
                let px = reloaded.get_pixel(x, y);
                assert!(
                    px[0] <= 2 && px[1] <= 2 && px[2] <= 2,
                    "redacted pixel ({x},{y}) must be black, got {px:?}"
                );
            }
        }
        // Outside the redacted area, original bright pixels remain intact.
        let outside = reloaded.get_pixel(5, 5);
        assert!(
            outside[2] > 140,
            "unredacted pixel should keep original colour, got {outside:?}"
        );

        // Also test lossless PNG redaction for exact [0, 0, 0, 255] on every pixel inside rect.
        let png_src = dir.path().join("secret.png");
        bright.save(&png_src).unwrap();
        let png_out = export_full(
            &ExportRequest {
                source: png_src,
                steps: vec![Step::Redact {
                    rect: redact_rect,
                    rotation: 0.0,
                }],
                mode: SaveMode::SaveCopy,
                strip_metadata: true,
            },
            &cancel,
            &progress,
        )
        .unwrap();
        let (png_reloaded, _, _, _) = decode_full(&png_out).unwrap();
        for y in 15..45 {
            for x in 20..70 {
                assert_eq!(
                    *png_reloaded.get_pixel(x, y),
                    Rgba([0, 0, 0, 255]),
                    "PNG redacted pixel ({x},{y}) must be exact opaque black"
                );
            }
        }
    }

    #[test]
    fn animated_gif_is_refused_and_single_frame_gif_succeeds() {
        let dir = tempfile::tempdir().unwrap();

        // Single-frame GIF succeeds.
        let single = dir.path().join("still.gif");
        make_bright_image(32, 24).save(&single).unwrap();
        let loaded = load_for_editing(&single);
        assert!(loaded.is_ok(), "single-frame GIF should load for editing");

        // Multi-frame animated GIF is refused with a clear message.
        let anim = dir.path().join("anim.gif");
        {
            let file = File::create(&anim).unwrap();
            let mut enc = image::codecs::gif::GifEncoder::new(file);
            let f1 = image::Frame::new(make_bright_image(16, 16));
            let f2 = image::Frame::new(make_bright_image(16, 16));
            enc.encode_frames([f1, f2]).unwrap();
        }
        let err = load_for_editing(&anim).err().unwrap_or_default();
        assert!(
            err.to_ascii_lowercase().contains("animated gif"),
            "expected clear animated GIF refusal, got: {err}"
        );
    }

    #[test]
    fn mark_geometry_hit_test_move_resize_rotate_recolour_and_transforms() {
        let mut steps = vec![
            Step::Rectangle {
                rect: RectF {
                    x: 40.0,
                    y: 40.0,
                    w: 100.0,
                    h: 60.0,
                },
                ink: Ink::Red,
                width: 4.0,
                rotation: 0.0,
            },
            Step::Arrow {
                from: PointF::new(10.0, 10.0),
                to: PointF::new(80.0, 10.0),
                ink: Ink::Blue,
                width: 4.0,
            },
        ];
        // Hit test arrow shaft & rectangle edge.
        assert_eq!(hit_test_mark(&steps, PointF::new(45.0, 10.0), 8.0), Some(1));
        assert_eq!(hit_test_mark(&steps, PointF::new(40.0, 70.0), 8.0), Some(0));

        // Move, resize, rotate, recolour rectangle.
        translate_mark(&mut steps[0], 10.0, -5.0);
        assert!(steps[0].set_ink(Ink::Yellow));
        assert_eq!(steps[0].ink(), Some(Ink::Yellow));
        resize_mark_handle(&mut steps[0], Handle::Corner(2), PointF::new(200.0, 140.0));
        resize_mark_handle(&mut steps[0], Handle::Turn, PointF::new(200.0, 80.0));
        let frame = mark_frame(&steps[0]).unwrap();
        assert!(frame.rect.w > 100.0);
        assert!(frame.rotation.abs() > 0.01);

        // Crop + Rotate + Flip replay cleanly on an image.
        steps.push(Step::Crop {
            rect: RectF {
                x: 10.0,
                y: 10.0,
                w: 80.0,
                h: 50.0,
            },
        });
        steps.push(Step::Rotate { quarter_turns: 1 });
        steps.push(Step::Flip { vertical: false });
        assert_eq!(current_dimensions(200, 150, &steps), (50, 80));
        let out = apply_recipe(&make_bright_image(200, 150), &steps, 1.0, None, None).unwrap();
        assert_eq!(out.dimensions(), (50, 80));
    }

    #[test]
    fn save_copy_and_replace_original_failure_paths_preserve_original_and_clean_temp() {
        let dir = tempfile::tempdir().unwrap();
        let original = dir.path().join("photo.png");
        let original_img = make_bright_image(40, 30);
        original_img.save(&original).unwrap();
        let original_bytes = std::fs::read(&original).unwrap();

        // 1. Save a Copy succeeds without touching original and picks numbered name on second save.
        let copy1 = write_copy_bytes(&original, OutputFormat::Png, &original_bytes).unwrap();
        assert!(copy1.ends_with("photo edited.png"));
        let copy2 = write_copy_bytes(&original, OutputFormat::Png, &original_bytes).unwrap();
        assert!(copy2.ends_with("photo edited 2.png"));
        assert_eq!(std::fs::read(&original).unwrap(), original_bytes);

        // 2. Save a Copy into a non-existent folder fails cleanly.
        let missing_src = dir.path().join("nonexistent-dir").join("photo.png");
        assert!(write_copy_bytes(&missing_src, OutputFormat::Png, &original_bytes).is_err());

        // 3. Replace Original on a missing file fails cleanly and leaves no temp file.
        let missing_target = dir.path().join("missing.png");
        assert!(replace_original_bytes(&missing_target, &original_bytes).is_err());

        // 4. Replace Original succeeds atomically and leaves zero .seek-edit-*.tmp files.
        let mut modified = original_img.clone();
        modified.put_pixel(0, 0, Rgba([1, 2, 3, 255]));
        let mod_bytes = encode_image_bytes(&modified, OutputFormat::Png, None).unwrap();
        replace_original_bytes(&original, &mod_bytes).unwrap();
        assert_eq!(std::fs::read(&original).unwrap(), mod_bytes);
        let leftovers: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().contains(".seek-edit-"))
            .collect();
        assert!(leftovers.is_empty(), "no staging temp files should remain");
    }

    #[test]
    fn text_mark_renders_visible_anti_aliased_pixels() {
        let base = RgbaImage::from_pixel(240, 100, Rgba([255, 255, 255, 255]));
        let out = apply_recipe(
            &base,
            &[Step::Text {
                at: PointF::new(20.0, 20.0),
                text: "Hello Seek".into(),
                size: 32.0,
                ink: Ink::Red,
                rotation: 0.0,
            }],
            1.0,
            None,
            None,
        )
        .unwrap();
        let red_pixels = out
            .pixels()
            .filter(|p| p[0] > 200 && p[1] < 120 && p[2] < 120)
            .count();
        assert!(
            red_pixels > 40,
            "expected visible rasterized text pixels, got {red_pixels}"
        );
    }
}
