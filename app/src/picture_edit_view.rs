//! Picture editing UI (Phase 11.9), shared between Quick Look and picture
//! Document panes. Uses `crate::picture_edit` for non-destructive recipes,
//! 60 FPS GPU-accelerated live vector marks, 8-handle crop, permanent
//! pixel-level redaction, and cancellable full-resolution export.
use super::*;
use crate::picture_edit::{
    AspectPreset, CropEdge, ExportRequest, Handle, Ink, LoadedPicture, MarkFrame, PointF, RectF,
    SaveMode, Step, StrokeSize, Tool, apply_aspect_preset, apply_recipe, current_dimensions,
    export_full, first_live_mark, has_redaction, hit_test_handle, hit_test_mark, load_for_editing,
    mark_frame, resize_crop_edge, resize_mark_handle, rotate_point, to_render_image,
    translate_mark,
};
use gpui_component::menu::{DropdownMenu, PopupMenuItem};
use gpui_component_assets::IconName as Glyph;
use std::{
    cell::Cell,
    rc::Rc,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU8, Ordering},
    },
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum EditHost {
    QuickLook,
    DocumentPane(crate::pane_layout::PaneId),
}

#[derive(Clone, Copy, Debug)]
enum CropDrag {
    Move { offset_x: f32, offset_y: f32 },
    Corner(usize),
    Edge(CropEdge),
    New(PointF),
}

#[derive(Clone, Debug)]
enum DragAction {
    DrawPen {
        step_index: usize,
    },
    DrawShape {
        step_index: usize,
        origin: PointF,
    },
    MoveMark {
        step_index: usize,
        last: PointF,
    },
    Handle {
        step_index: usize,
        handle: Handle,
    },
    Crop(CropDrag),
}

struct TextPrompt {
    input: Entity<InputState>,
    at: PointF,
    existing_index: Option<usize>,
    ink: Ink,
    size: f32,
}

struct ExportJob {
    cancel: Arc<AtomicBool>,
    progress: Arc<AtomicU8>,
    mode: SaveMode,
    _task: Task<()>,
}

/// Fitted image display box inside the canvas, in window coordinates.
#[derive(Clone, Copy, Debug)]
struct ImageBox {
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    scale: f32,
    canvas_x: f32,
    canvas_y: f32,
}

impl ImageBox {
    fn compute(bounds: Bounds<Pixels>, img_w: u32, img_h: u32) -> Option<Self> {
        let cw = f32::from(bounds.size.width);
        let ch = f32::from(bounds.size.height);
        let pad = 20.0;
        let avail_w = (cw - pad * 2.0).max(16.0);
        let avail_h = (ch - pad * 2.0).max(16.0);
        let iw = img_w.max(1) as f32;
        let ih = img_h.max(1) as f32;
        let scale = (avail_w / iw).min(avail_h / ih).max(1e-4);
        let w = iw * scale;
        let h = ih * scale;
        let canvas_x = f32::from(bounds.origin.x);
        let canvas_y = f32::from(bounds.origin.y);
        let x = canvas_x + (cw - w) * 0.5;
        let y = canvas_y + (ch - h) * 0.5;
        Some(Self {
            x,
            y,
            w,
            h,
            scale,
            canvas_x,
            canvas_y,
        })
    }

    fn screen_to_image(self, pos: Point<Pixels>) -> PointF {
        PointF::new(
            (f32::from(pos.x) - self.x) / self.scale,
            (f32::from(pos.y) - self.y) / self.scale,
        )
    }

    fn image_to_local(self, pt: PointF) -> (f32, f32) {
        (
            (self.x - self.canvas_x) + pt.x * self.scale,
            (self.y - self.canvas_y) + pt.y * self.scale,
        )
    }

    fn image_to_window(self, pt: PointF) -> Point<Pixels> {
        point(
            px(self.x + pt.x * self.scale),
            px(self.y + pt.y * self.scale),
        )
    }
}

fn ink_hsla(ink: Ink, alpha: f32) -> Hsla {
    let [r, g, b, _] = ink.rgba();
    let a = (alpha.clamp(0.0, 1.0) * 255.0).round() as u32;
    rgba(((r as u32) << 24) | ((g as u32) << 16) | ((b as u32) << 8) | a).into()
}

/// Paint all live marks (after the last Crop/Rotate/Flip) directly on the GPU
/// via GPUI `PathBuilder` + shaped text so drawing runs at full 60+ FPS with
/// zero CPU bitmap re-rasterization or texture re-upload.
fn paint_live_marks(marks: &[Step], ibox: ImageBox, window: &mut Window, cx: &mut App) {
    if marks.is_empty() {
        return;
    }
    let clip = Bounds::new(
        point(px(ibox.x), px(ibox.y)),
        size(px(ibox.w), px(ibox.h)),
    );
    window.with_content_mask(Some(ContentMask { bounds: clip }), |window| {
        for step in marks {
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
                        continue;
                    }
                    let is_hl = matches!(step, Step::Highlighter { .. });
                    let color = ink_hsla(*ink, if is_hl { 0.36 } else { 1.0 });
                    let stroke_w = (*width * ibox.scale).max(if is_hl { 4.0 } else { 1.5 });
                    if points.len() == 1 {
                        let c = ibox.image_to_window(points[0]);
                        let r = stroke_w * 0.5;
                        let mut pb = PathBuilder::fill();
                        for i in 0..16 {
                            let a = (i as f32 / 16.0) * std::f32::consts::TAU;
                            let p = point(c.x + px(r * a.cos()), c.y + px(r * a.sin()));
                            if i == 0 {
                                pb.move_to(p);
                            } else {
                                pb.line_to(p);
                            }
                        }
                        pb.close();
                        if let Ok(path) = pb.build() {
                            window.paint_path(path, color);
                        }
                    } else {
                        let center = mark_frame(step)
                            .map(|f| f.center())
                            .unwrap_or(points[0]);
                        let mut pb = PathBuilder::stroke(px(stroke_w));
                        for (i, &pt) in points.iter().enumerate() {
                            let wpt = ibox.image_to_window(rotate_point(pt, center, *rotation));
                            if i == 0 {
                                pb.move_to(wpt);
                            } else {
                                pb.line_to(wpt);
                            }
                        }
                        if let Ok(path) = pb.build() {
                            window.paint_path(path, color);
                        }
                    }
                }
                Step::Arrow {
                    from,
                    to,
                    ink,
                    width,
                } => {
                    let dx = to.x - from.x;
                    let dy = to.y - from.y;
                    let len = dx.hypot(dy);
                    if len < 1.0 {
                        continue;
                    }
                    let color = ink_hsla(*ink, 1.0);
                    let stroke_w = (*width * ibox.scale).max(1.5);
                    let ux = dx / len;
                    let uy = dy / len;
                    let px_u = -uy;
                    let py_u = ux;
                    let head_len = (*width * 4.2).max(14.0).min(len * 0.45);
                    let head_half_w = head_len * 0.55;
                    let shaft_end = PointF::new(
                        to.x - ux * head_len * 0.72,
                        to.y - uy * head_len * 0.72,
                    );
                    let mut shaft = PathBuilder::stroke(px(stroke_w));
                    shaft.move_to(ibox.image_to_window(*from));
                    shaft.line_to(ibox.image_to_window(shaft_end));
                    if let Ok(path) = shaft.build() {
                        window.paint_path(path, color);
                    }
                    let base = PointF::new(to.x - ux * head_len, to.y - uy * head_len);
                    let left = PointF::new(base.x + px_u * head_half_w, base.y + py_u * head_half_w);
                    let right =
                        PointF::new(base.x - px_u * head_half_w, base.y - py_u * head_half_w);
                    let mut head = PathBuilder::fill();
                    head.move_to(ibox.image_to_window(*to));
                    head.line_to(ibox.image_to_window(left));
                    head.line_to(ibox.image_to_window(right));
                    head.close();
                    if let Ok(path) = head.build() {
                        window.paint_path(path, color);
                    }
                }
                Step::Rectangle {
                    rect,
                    ink,
                    width,
                    rotation,
                } => {
                    let color = ink_hsla(*ink, 1.0);
                    let stroke_w = (*width * ibox.scale).max(1.5);
                    let c = rect.center();
                    let corners = rect.corners().map(|pt| rotate_point(pt, c, *rotation));
                    let mut pb = PathBuilder::stroke(px(stroke_w));
                    pb.move_to(ibox.image_to_window(corners[0]));
                    pb.line_to(ibox.image_to_window(corners[1]));
                    pb.line_to(ibox.image_to_window(corners[2]));
                    pb.line_to(ibox.image_to_window(corners[3]));
                    pb.close();
                    if let Ok(path) = pb.build() {
                        window.paint_path(path, color);
                    }
                }
                Step::Ellipse {
                    rect,
                    ink,
                    width,
                    rotation,
                } => {
                    let color = ink_hsla(*ink, 1.0);
                    let stroke_w = (*width * ibox.scale).max(1.5);
                    let c = rect.center();
                    let rx = (rect.w * 0.5).max(1.0);
                    let ry = (rect.h * 0.5).max(1.0);
                    let mut pb = PathBuilder::stroke(px(stroke_w));
                    const SEGS: usize = 48;
                    for i in 0..SEGS {
                        let a = (i as f32 / SEGS as f32) * std::f32::consts::TAU;
                        let pt = PointF::new(c.x + rx * a.cos(), c.y + ry * a.sin());
                        let wpt = ibox.image_to_window(rotate_point(pt, c, *rotation));
                        if i == 0 {
                            pb.move_to(wpt);
                        } else {
                            pb.line_to(wpt);
                        }
                    }
                    pb.close();
                    if let Ok(path) = pb.build() {
                        window.paint_path(path, color);
                    }
                }
                Step::Redact { rect, rotation } => {
                    let c = rect.center();
                    let corners = rect.corners().map(|pt| rotate_point(pt, c, *rotation));
                    let mut pb = PathBuilder::fill();
                    pb.move_to(ibox.image_to_window(corners[0]));
                    pb.line_to(ibox.image_to_window(corners[1]));
                    pb.line_to(ibox.image_to_window(corners[2]));
                    pb.line_to(ibox.image_to_window(corners[3]));
                    pb.close();
                    if let Ok(path) = pb.build() {
                        window.paint_path(path, rgba(0x000000FF));
                    }
                }
                Step::Text {
                    at,
                    text,
                    size,
                    ink,
                    ..
                } => {
                    let color = ink_hsla(*ink, 1.0);
                    let font_size = px((*size * ibox.scale).max(9.0));
                    let line_height = px((*size * 1.25 * ibox.scale).max(11.0));
                    let font = window.text_style().font();
                    for (idx, raw_line) in text.lines().enumerate() {
                        if raw_line.is_empty() {
                            continue;
                        }
                        let shared: SharedString = raw_line.to_owned().into();
                        let run = TextRun {
                            len: shared.len(),
                            font: font.clone(),
                            color,
                            background_color: None,
                            underline: None,
                            strikethrough: None,
                        };
                        let shaped = window
                            .text_system()
                            .shape_line(shared, font_size, &[run], None);
                        let origin = ibox.image_to_window(PointF::new(
                            at.x,
                            at.y + idx as f32 * *size * 1.25,
                        ));
                        let _ = shaped.paint(origin, line_height, TextAlign::Left, None, window, cx);
                    }
                }
                _ => {}
            }
        }
    });
}

pub(super) struct State {
    pub(super) host: EditHost,
    loaded: LoadedPicture,
    steps: Vec<Step>,
    /// The baked prefix (`steps[..first_live_mark]`) currently reflected in `baked_preview`.
    baked_prefix: Vec<Step>,
    undo_stack: Vec<Vec<Step>>,
    redo_stack: Vec<Vec<Step>>,
    dirty: bool,
    tool: Tool,
    ink: Ink,
    stroke_size: StrokeSize,
    selected: Option<usize>,
    baked_preview: Arc<RenderImage>,
    preview_size: (u32, u32),
    canvas_bounds: Rc<Cell<Option<Bounds<Pixels>>>>,
    drag: Option<DragAction>,
    crop_rect: Option<RectF>,
    crop_aspect: AspectPreset,
    text_prompt: Option<TextPrompt>,
    strip_metadata: bool,
    pub(super) confirm_discard: bool,
    confirm_replace: bool,
    exporting: Option<ExportJob>,
    error: Option<String>,
    focus: FocusHandle,
}

impl State {
    pub(super) fn is_dirty(&self) -> bool {
        self.dirty
    }

    fn effective_strip_metadata(&self) -> bool {
        self.strip_metadata || has_redaction(&self.steps)
    }

    fn default_stroke_width(&self) -> f32 {
        let longest = self.preview_size.0.max(self.preview_size.1).max(1) as f32;
        ((longest * 0.0055).clamp(3.0, 28.0)) * self.stroke_size.scale()
    }

    fn default_text_size(&self) -> f32 {
        let longest = self.preview_size.0.max(self.preview_size.1).max(1) as f32;
        ((longest * 0.042).clamp(18.0, 120.0)) * self.stroke_size.scale()
    }

    fn push_undo(&mut self) {
        self.undo_stack.push(self.steps.clone());
        self.redo_stack.clear();
        if self.undo_stack.len() > 64 {
            self.undo_stack.remove(0);
        }
    }

    /// Re-rasterize `baked_preview` ONLY if the transform prefix
    /// (`steps[..first_live_mark]`) changed. Live marks after the last
    /// Crop/Rotate/Flip paint directly on the GPU at 60+ FPS with zero upload.
    fn sync_baked_preview(&mut self, window: &mut Window) {
        let split = first_live_mark(&self.steps);
        let prefix = &self.steps[..split];
        if self.baked_prefix.as_slice() == prefix {
            return;
        }
        let rendered = apply_recipe(
            &self.loaded.working_base,
            prefix,
            self.loaded.work_scale,
            None,
            None,
        );
        match rendered {
            Ok(img) => {
                let next = to_render_image(&img);
                let old = std::mem::replace(&mut self.baked_preview, next);
                let _ = window.drop_image(old);
                self.baked_prefix = prefix.to_vec();
                self.preview_size = current_dimensions(
                    self.loaded.original_size.0,
                    self.loaded.original_size.1,
                    prefix,
                );
            }
            Err(err) => {
                self.error = Some(err);
            }
        }
    }

    fn image_box(&self) -> Option<ImageBox> {
        let bounds = self.canvas_bounds.get()?;
        ImageBox::compute(bounds, self.preview_size.0, self.preview_size.1)
    }

    fn start_crop_rect(&mut self) {
        let (w, h) = self.preview_size;
        let wf = w.max(1) as f32;
        let hf = h.max(1) as f32;
        let inset_x = wf * 0.08;
        let inset_y = hf * 0.08;
        let base = RectF {
            x: inset_x,
            y: inset_y,
            w: (wf - inset_x * 2.0).max(16.0),
            h: (hf - inset_y * 2.0).max(16.0),
        };
        self.crop_rect = Some(apply_aspect_preset(base, self.crop_aspect, wf, hf));
    }
}

impl Seek {
    pub(super) fn picture_edit_for(&self, host: EditHost) -> bool {
        self.picture_edit.as_ref().is_some_and(|e| e.host == host)
    }

    /// Open picture editing inline inside Quick Look or a Document pane.
    pub(super) fn open_picture_edit(
        &mut self,
        host: EditHost,
        path: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(existing) = &self.picture_edit {
            if existing.host == host && existing.loaded.path == path {
                existing.focus.focus(window, cx);
                cx.notify();
                return;
            }
            if existing.is_dirty() {
                self.message = Some("Save or discard the current picture edits first.".into());
                cx.notify();
                return;
            }
        }
        match load_for_editing(&path) {
            Ok(loaded) => {
                let baked_preview = to_render_image(&loaded.working_base);
                let preview_size = loaded.original_size;
                let focus = cx.focus_handle();
                focus.focus(window, cx);
                if let Some(old) = self.picture_edit.replace(State {
                    host,
                    loaded,
                    steps: Vec::new(),
                    baked_prefix: Vec::new(),
                    undo_stack: Vec::new(),
                    redo_stack: Vec::new(),
                    dirty: false,
                    tool: Tool::Select,
                    ink: Ink::Red,
                    stroke_size: StrokeSize::Medium,
                    selected: None,
                    baked_preview,
                    preview_size,
                    canvas_bounds: Rc::new(Cell::new(None)),
                    drag: None,
                    crop_rect: None,
                    crop_aspect: AspectPreset::Free,
                    text_prompt: None,
                    strip_metadata: true,
                    confirm_discard: false,
                    confirm_replace: false,
                    exporting: None,
                    error: None,
                    focus,
                }) {
                    let _ = window.drop_image(old.baked_preview);
                }
                cx.notify();
            }
            Err(err) => {
                self.message = Some(err);
                cx.notify();
            }
        }
    }

    /// Returns `true` if closing `host` should be blocked (shows discard prompt).
    pub(super) fn request_picture_edit_exit(
        &mut self,
        host: EditHost,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(ed) = self.picture_edit.as_mut().filter(|e| e.host == host) else {
            return false;
        };
        if ed.exporting.is_some() {
            ed.error = Some("Wait for export to finish or click Cancel.".into());
            cx.notify();
            return true;
        }
        if ed.is_dirty() {
            ed.confirm_discard = true;
            ed.confirm_replace = false;
            cx.notify();
            return true;
        }
        self.close_picture_edit_now(window, cx);
        false
    }

    pub(super) fn close_picture_edit_now(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(ed) = self.picture_edit.take() {
            if let Some(job) = ed.exporting {
                job.cancel.store(true, Ordering::Relaxed);
            }
            let _ = window.drop_image(ed.baked_preview);
            match ed.host {
                EditHost::QuickLook => self.preview_focus.focus(window, cx),
                EditHost::DocumentPane(id) => {
                    if let Some(doc) = self.doc_panes.iter().find(|d| d.id == id) {
                        doc.focus.focus(window, cx);
                    }
                }
            }
            cx.notify();
        }
    }

    fn picture_edit_select_tool(
        &mut self,
        tool: Tool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(ed) = self.picture_edit.as_mut() else {
            return;
        };
        ed.tool = tool;
        ed.drag = None;
        ed.text_prompt = None;
        ed.confirm_discard = false;
        ed.confirm_replace = false;
        if tool == Tool::Crop {
            ed.selected = None;
            ed.start_crop_rect();
        } else {
            ed.crop_rect = None;
        }
        ed.focus.focus(window, cx);
        cx.notify();
    }

    fn picture_edit_apply_transform(
        &mut self,
        step: Step,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(ed) = self.picture_edit.as_mut() else {
            return;
        };
        ed.push_undo();
        ed.steps.push(step);
        ed.selected = None;
        ed.crop_rect = None;
        if ed.tool == Tool::Crop {
            ed.tool = Tool::Select;
        }
        ed.dirty = true;
        ed.error = None;
        ed.sync_baked_preview(window);
        ed.focus.focus(window, cx);
        cx.notify();
    }

    fn picture_edit_apply_crop(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(rect) = self.picture_edit.as_ref().and_then(|e| e.crop_rect) else {
            return;
        };
        if rect.w < 4.0 || rect.h < 4.0 {
            return;
        }
        self.picture_edit_apply_transform(Step::Crop { rect }, window, cx);
    }

    fn picture_edit_undo(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(ed) = self.picture_edit.as_mut() else {
            return;
        };
        let Some(prev) = ed.undo_stack.pop() else {
            return;
        };
        ed.redo_stack.push(std::mem::replace(&mut ed.steps, prev));
        if ed.redo_stack.len() > 64 {
            ed.redo_stack.remove(0);
        }
        ed.selected = ed
            .selected
            .filter(|&i| i >= first_live_mark(&ed.steps) && i < ed.steps.len());
        ed.dirty = !ed.steps.is_empty();
        ed.crop_rect = None;
        ed.text_prompt = None;
        ed.sync_baked_preview(window);
        cx.notify();
    }

    fn picture_edit_redo(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(ed) = self.picture_edit.as_mut() else {
            return;
        };
        let Some(next) = ed.redo_stack.pop() else {
            return;
        };
        ed.undo_stack.push(std::mem::replace(&mut ed.steps, next));
        if ed.undo_stack.len() > 64 {
            ed.undo_stack.remove(0);
        }
        ed.selected = ed
            .selected
            .filter(|&i| i >= first_live_mark(&ed.steps) && i < ed.steps.len());
        ed.dirty = !ed.steps.is_empty();
        ed.crop_rect = None;
        ed.text_prompt = None;
        ed.sync_baked_preview(window);
        cx.notify();
    }

    fn picture_edit_delete_selected(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        let Some(ed) = self.picture_edit.as_mut() else {
            return;
        };
        let Some(idx) = ed.selected else {
            return;
        };
        if idx < first_live_mark(&ed.steps) || idx >= ed.steps.len() {
            ed.selected = None;
            return;
        }
        ed.push_undo();
        ed.steps.remove(idx);
        ed.selected = None;
        ed.dirty = !ed.steps.is_empty();
        cx.notify();
    }

    fn picture_edit_pick_ink(&mut self, ink: Ink, window: &mut Window, cx: &mut Context<Self>) {
        let Some(ed) = self.picture_edit.as_mut() else {
            return;
        };
        ed.ink = ink;
        if let Some(prompt) = ed.text_prompt.as_mut() {
            prompt.ink = ink;
        }
        if let Some(idx) = ed.selected
            && idx >= first_live_mark(&ed.steps)
            && idx < ed.steps.len()
            && ed.steps[idx].ink().is_some_and(|old| old != ink)
        {
            ed.push_undo();
            ed.steps[idx].set_ink(ink);
            ed.dirty = true;
        }
        ed.focus.focus(window, cx);
        cx.notify();
    }

    fn picture_edit_open_text_prompt(
        &mut self,
        at: PointF,
        existing_index: Option<usize>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(ed) = self.picture_edit.as_mut() else {
            return;
        };
        let (initial, ink, size) = match existing_index.and_then(|i| ed.steps.get(i)) {
            Some(Step::Text {
                text, ink, size, ..
            }) => (text.clone(), *ink, *size),
            _ => (String::new(), ed.ink, ed.default_text_size()),
        };
        let input = cx.new(|cx| {
            let mut state = InputState::new(window, cx).placeholder("Type label text…");
            state.set_value(initial, window, cx);
            state
        });
        input.update(cx, |s, cx| s.focus(window, cx));
        ed.text_prompt = Some(TextPrompt {
            input,
            at,
            existing_index,
            ink,
            size,
        });
        cx.notify();
    }

    fn picture_edit_commit_text(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(ed) = self.picture_edit.as_mut() else {
            return;
        };
        let Some(prompt) = ed.text_prompt.take() else {
            return;
        };
        let value = prompt.input.read(cx).value().trim().to_string();
        if value.is_empty() {
            ed.focus.focus(window, cx);
            cx.notify();
            return;
        }
        ed.push_undo();
        if let Some(idx) = prompt.existing_index
            && let Some(Step::Text { text, ink, .. }) = ed.steps.get_mut(idx)
        {
            *text = value;
            *ink = prompt.ink;
            ed.selected = Some(idx);
        } else {
            ed.steps.push(Step::Text {
                at: prompt.at,
                text: value,
                size: prompt.size,
                ink: prompt.ink,
                rotation: 0.0,
            });
            ed.selected = Some(ed.steps.len() - 1);
            ed.tool = Tool::Select;
        }
        ed.dirty = true;
        ed.focus.focus(window, cx);
        cx.notify();
    }

    fn picture_edit_start_export(
        &mut self,
        mode: SaveMode,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(ed) = self.picture_edit.as_mut() else {
            return;
        };
        if ed.exporting.is_some() {
            return;
        }
        ed.confirm_replace = false;
        ed.confirm_discard = false;
        ed.error = None;
        let req = ExportRequest {
            source: ed.loaded.path.clone(),
            steps: ed.steps.clone(),
            mode,
            strip_metadata: ed.effective_strip_metadata(),
        };
        let cancel = Arc::new(AtomicBool::new(false));
        let progress = Arc::new(AtomicU8::new(0));
        let cancel_bg = cancel.clone();
        let progress_bg = progress.clone();
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::Builder::new()
            .name("seek-picture-export".into())
            .spawn(move || {
                let result = export_full(&req, &cancel_bg, &progress_bg);
                let _ = tx.send(result);
            })
            .ok();

        let progress_ui = progress.clone();
        let task = cx.spawn_in(window, async move |this, cx| {
            loop {
                match rx.try_recv() {
                    Ok(result) => {
                        let _ = this.update_in(cx, |this, window, cx| {
                            if let Some(ed) = this.picture_edit.as_mut() {
                                ed.exporting = None;
                                match result {
                                    Ok(saved_path) => {
                                        let name = saved_path
                                            .file_name()
                                            .map(|n| n.to_string_lossy().into_owned())
                                            .unwrap_or_else(|| "picture".into());
                                        let host = ed.host;
                                        ed.dirty = false;
                                        let msg = match mode {
                                            SaveMode::SaveCopy => format!("Saved copy as {name}"),
                                            SaveMode::ReplaceOriginal => format!("Saved {name}"),
                                        };
                                        this.media.force();
                                        let _ = this.backend.tx.send(Command::Changed);
                                        if mode == SaveMode::ReplaceOriginal
                                            && let EditHost::DocumentPane(id) = host
                                        {
                                            this.reload_document_pane(id, window, cx);
                                        }
                                        this.close_picture_edit_now(window, cx);
                                        this.show_quiet_toast(msg, window, cx);
                                    }
                                    Err(err) => {
                                        ed.error = Some(err);
                                        cx.notify();
                                    }
                                }
                            }
                        });
                        break;
                    }
                    Err(std::sync::mpsc::TryRecvError::Empty) => {
                        let _ = this.update(cx, |_, cx| {
                            let _ = progress_ui.load(Ordering::Relaxed);
                            cx.notify();
                        });
                        cx.background_executor()
                            .timer(std::time::Duration::from_millis(40))
                            .await;
                    }
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                        let _ = this.update(cx, |this, cx| {
                            if let Some(ed) = this.picture_edit.as_mut() {
                                ed.exporting = None;
                                ed.error = Some("Export stopped unexpectedly.".into());
                            }
                            cx.notify();
                        });
                        break;
                    }
                }
            }
        });
        ed.exporting = Some(ExportJob {
            cancel,
            progress,
            mode,
            _task: task,
        });
        cx.notify();
    }

    fn picture_edit_mouse_down(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(ed) = self.picture_edit.as_mut() else {
            return;
        };
        if ed.exporting.is_some() || ed.confirm_discard || ed.confirm_replace {
            return;
        }
        ed.focus.focus(window, cx);
        let Some(ibox) = ed.image_box() else {
            return;
        };
        let pt = ibox.screen_to_image(event.position);
        let (img_w, img_h) = (ed.preview_size.0 as f32, ed.preview_size.1 as f32);
        let handle_r = 12.0 / ibox.scale;
        let turn_off = 24.0 / ibox.scale;
        let tol = 10.0 / ibox.scale;

        match ed.tool {
            Tool::Crop => {
                if let Some(crop) = ed.crop_rect {
                    // 1. Corners first
                    for (i, corner) in crop.corners().into_iter().enumerate() {
                        let dx = pt.x - corner.x;
                        let dy = pt.y - corner.y;
                        if (dx * dx + dy * dy).sqrt() <= handle_r * 1.4 {
                            ed.drag = Some(DragAction::Crop(CropDrag::Corner(i)));
                            cx.notify();
                            return;
                        }
                    }
                    // 2. Edge midpoint handles (and whole edge band)
                    let edge_band = 10.0 / ibox.scale;
                    for (edge, mid) in crop.edge_midpoints() {
                        let dx = pt.x - mid.x;
                        let dy = pt.y - mid.y;
                        let near_mid = (dx * dx + dy * dy).sqrt() <= handle_r * 1.45;
                        let on_edge = match edge {
                            CropEdge::Top => {
                                (pt.y - crop.y).abs() <= edge_band
                                    && pt.x >= crop.x
                                    && pt.x <= crop.x + crop.w
                            }
                            CropEdge::Bottom => {
                                (pt.y - (crop.y + crop.h)).abs() <= edge_band
                                    && pt.x >= crop.x
                                    && pt.x <= crop.x + crop.w
                            }
                            CropEdge::Left => {
                                (pt.x - crop.x).abs() <= edge_band
                                    && pt.y >= crop.y
                                    && pt.y <= crop.y + crop.h
                            }
                            CropEdge::Right => {
                                (pt.x - (crop.x + crop.w)).abs() <= edge_band
                                    && pt.y >= crop.y
                                    && pt.y <= crop.y + crop.h
                            }
                        };
                        if near_mid || on_edge {
                            ed.drag = Some(DragAction::Crop(CropDrag::Edge(edge)));
                            cx.notify();
                            return;
                        }
                    }
                    // 3. Inside crop box -> Move
                    if pt.x >= crop.x
                        && pt.x <= crop.x + crop.w
                        && pt.y >= crop.y
                        && pt.y <= crop.y + crop.h
                    {
                        ed.drag = Some(DragAction::Crop(CropDrag::Move {
                            offset_x: pt.x - crop.x,
                            offset_y: pt.y - crop.y,
                        }));
                        cx.notify();
                        return;
                    }
                }
                let clamped = PointF::new(pt.x.clamp(0.0, img_w), pt.y.clamp(0.0, img_h));
                ed.crop_rect = Some(RectF {
                    x: clamped.x,
                    y: clamped.y,
                    w: 1.0,
                    h: 1.0,
                });
                ed.drag = Some(DragAction::Crop(CropDrag::New(clamped)));
            }
            Tool::Select => {
                if let Some(sel) = ed.selected
                    && sel >= first_live_mark(&ed.steps)
                    && let Some(frame) = ed.steps.get(sel).and_then(mark_frame)
                    && let Some(handle) = hit_test_handle(frame, pt, handle_r, turn_off)
                {
                    ed.push_undo();
                    ed.drag = Some(DragAction::Handle {
                        step_index: sel,
                        handle,
                    });
                    cx.notify();
                    return;
                }
                if let Some(hit) = hit_test_mark(&ed.steps, pt, tol) {
                    ed.selected = Some(hit);
                    if let Some(ink) = ed.steps[hit].ink() {
                        ed.ink = ink;
                    }
                    if event.click_count >= 2 && matches!(ed.steps[hit], Step::Text { .. }) {
                        let at = match &ed.steps[hit] {
                            Step::Text { at, .. } => *at,
                            _ => pt,
                        };
                        self.picture_edit_open_text_prompt(at, Some(hit), window, cx);
                        return;
                    }
                    ed.push_undo();
                    ed.drag = Some(DragAction::MoveMark {
                        step_index: hit,
                        last: pt,
                    });
                } else {
                    ed.selected = None;
                }
            }
            Tool::Pen | Tool::Highlighter => {
                let width = if ed.tool == Tool::Highlighter {
                    ed.default_stroke_width() * 3.4
                } else {
                    ed.default_stroke_width()
                };
                ed.push_undo();
                let step = if ed.tool == Tool::Highlighter {
                    Step::Highlighter {
                        points: vec![pt],
                        ink: ed.ink,
                        width,
                        rotation: 0.0,
                    }
                } else {
                    Step::Pen {
                        points: vec![pt],
                        ink: ed.ink,
                        width,
                        rotation: 0.0,
                    }
                };
                ed.steps.push(step);
                let idx = ed.steps.len() - 1;
                ed.selected = Some(idx);
                ed.dirty = true;
                ed.drag = Some(DragAction::DrawPen { step_index: idx });
            }
            Tool::Arrow => {
                let width = ed.default_stroke_width();
                ed.push_undo();
                ed.steps.push(Step::Arrow {
                    from: pt,
                    to: pt,
                    ink: ed.ink,
                    width,
                });
                let idx = ed.steps.len() - 1;
                ed.selected = Some(idx);
                ed.dirty = true;
                ed.drag = Some(DragAction::DrawShape {
                    step_index: idx,
                    origin: pt,
                });
            }
            Tool::Rectangle | Tool::Ellipse | Tool::Redact => {
                let width = ed.default_stroke_width();
                ed.push_undo();
                let rect = RectF {
                    x: pt.x,
                    y: pt.y,
                    w: 1.0,
                    h: 1.0,
                };
                let step = match ed.tool {
                    Tool::Rectangle => Step::Rectangle {
                        rect,
                        ink: ed.ink,
                        width,
                        rotation: 0.0,
                    },
                    Tool::Ellipse => Step::Ellipse {
                        rect,
                        ink: ed.ink,
                        width,
                        rotation: 0.0,
                    },
                    _ => Step::Redact {
                        rect,
                        rotation: 0.0,
                    },
                };
                ed.steps.push(step);
                let idx = ed.steps.len() - 1;
                ed.selected = Some(idx);
                ed.dirty = true;
                ed.drag = Some(DragAction::DrawShape {
                    step_index: idx,
                    origin: pt,
                });
            }
            Tool::Text => {
                let clamped = PointF::new(
                    pt.x.clamp(8.0, (img_w - 40.0).max(8.0)),
                    pt.y.clamp(8.0, (img_h - 40.0).max(8.0)),
                );
                self.picture_edit_open_text_prompt(clamped, None, window, cx);
                return;
            }
        }
        cx.notify();
    }

    fn picture_edit_mouse_move(
        &mut self,
        event: &MouseMoveEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(ed) = self.picture_edit.as_mut() else {
            return;
        };
        let Some(drag) = ed.drag.clone() else {
            return;
        };
        if !event.dragging() {
            ed.drag = None;
            cx.notify();
            return;
        }
        let Some(ibox) = ed.image_box() else {
            return;
        };
        let pt = ibox.screen_to_image(event.position);
        let (img_w, img_h) = (ed.preview_size.0 as f32, ed.preview_size.1 as f32);

        match drag {
            DragAction::DrawPen { step_index } => {
                if let Some(Step::Pen { points, .. } | Step::Highlighter { points, .. }) =
                    ed.steps.get_mut(step_index)
                {
                    let min_step_sq = (2.0 / ibox.scale).powi(2).max(1.5);
                    let far_enough = points.last().is_none_or(|prev| {
                        let dx = prev.x - pt.x;
                        let dy = prev.y - pt.y;
                        dx * dx + dy * dy >= min_step_sq
                    });
                    if far_enough {
                        points.push(pt);
                    }
                }
            }
            DragAction::DrawShape { step_index, origin } => {
                let rect = RectF::from_points(origin, pt);
                if let Some(step) = ed.steps.get_mut(step_index) {
                    match step {
                        Step::Arrow { to, .. } => *to = pt,
                        Step::Rectangle { rect: r, .. }
                        | Step::Ellipse { rect: r, .. }
                        | Step::Redact { rect: r, .. } => *r = rect,
                        _ => {}
                    }
                }
            }
            DragAction::MoveMark { step_index, last } => {
                let dx = pt.x - last.x;
                let dy = pt.y - last.y;
                if let Some(step) = ed.steps.get_mut(step_index) {
                    translate_mark(step, dx, dy);
                    ed.dirty = true;
                    ed.drag = Some(DragAction::MoveMark {
                        step_index,
                        last: pt,
                    });
                }
            }
            DragAction::Handle { step_index, handle } => {
                if let Some(step) = ed.steps.get_mut(step_index) {
                    resize_mark_handle(step, handle, pt);
                    ed.dirty = true;
                }
            }
            DragAction::Crop(kind) => {
                let clamped = PointF::new(pt.x.clamp(0.0, img_w), pt.y.clamp(0.0, img_h));
                match kind {
                    CropDrag::New(origin) => {
                        let raw = RectF::from_points(origin, clamped);
                        ed.crop_rect = Some(apply_aspect_preset(raw, ed.crop_aspect, img_w, img_h));
                    }
                    CropDrag::Move { offset_x, offset_y } => {
                        if let Some(mut crop) = ed.crop_rect {
                            crop.x = (pt.x - offset_x).clamp(0.0, (img_w - crop.w).max(0.0));
                            crop.y = (pt.y - offset_y).clamp(0.0, (img_h - crop.h).max(0.0));
                            ed.crop_rect = Some(crop);
                        }
                    }
                    CropDrag::Edge(edge) => {
                        if let Some(crop) = ed.crop_rect {
                            ed.crop_rect = Some(resize_crop_edge(
                                crop,
                                edge,
                                clamped,
                                ed.crop_aspect,
                                img_w,
                                img_h,
                            ));
                        }
                    }
                    CropDrag::Corner(corner) => {
                        if let Some(crop) = ed.crop_rect {
                            let mut x0 = crop.x;
                            let mut y0 = crop.y;
                            let mut x1 = crop.x + crop.w;
                            let mut y1 = crop.y + crop.h;
                            match corner {
                                0 => {
                                    x0 = clamped.x;
                                    y0 = clamped.y;
                                }
                                1 => {
                                    x1 = clamped.x;
                                    y0 = clamped.y;
                                }
                                2 => {
                                    x1 = clamped.x;
                                    y1 = clamped.y;
                                }
                                3 => {
                                    x0 = clamped.x;
                                    y1 = clamped.y;
                                }
                                _ => {}
                            }
                            let raw = RectF {
                                x: x0.min(x1),
                                y: y0.min(y1),
                                w: (x1 - x0).abs().max(16.0),
                                h: (y1 - y0).abs().max(16.0),
                            };
                            ed.crop_rect =
                                Some(apply_aspect_preset(raw, ed.crop_aspect, img_w, img_h));
                        }
                    }
                }
            }
        }
        cx.notify();
    }

    fn picture_edit_mouse_up(
        &mut self,
        _event: &MouseUpEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(ed) = self.picture_edit.as_mut() else {
            return;
        };
        if let Some(DragAction::DrawShape { step_index, .. }) = ed.drag.take() {
            // Discard accidental zero-size clicks when drawing shapes/arrows.
            let tiny = ed.steps.get(step_index).is_some_and(|step| match step {
                Step::Arrow { from, to, .. } => (from.x - to.x).hypot(from.y - to.y) < 6.0,
                Step::Rectangle { rect, .. }
                | Step::Ellipse { rect, .. }
                | Step::Redact { rect, .. } => rect.w < 4.0 && rect.h < 4.0,
                _ => false,
            });
            if tiny {
                ed.steps.remove(step_index);
                ed.undo_stack.pop();
                ed.selected = None;
                ed.dirty = !ed.steps.is_empty();
            }
        } else {
            ed.drag = None;
        }
        cx.notify();
    }

    /// Global key handler called from Seek's root `on_key_down` AND
    /// `picture-editor-root` so Ctrl+Z, Ctrl+Shift+Z, Ctrl+Y, Escape, Enter,
    /// and Delete always work regardless of which toolbar button was clicked.
    pub(super) fn picture_edit_handle_key(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let key = event.keystroke.key.as_str();
        let mods = event.keystroke.modifiers;
        let has_prompt = self
            .picture_edit
            .as_ref()
            .is_some_and(|e| e.text_prompt.is_some());
        if has_prompt {
            match key {
                "enter" => {
                    self.picture_edit_commit_text(window, cx);
                    cx.stop_propagation();
                    return true;
                }
                "escape" => {
                    if let Some(ed) = self.picture_edit.as_mut() {
                        ed.text_prompt = None;
                        ed.focus.focus(window, cx);
                    }
                    cx.stop_propagation();
                    cx.notify();
                    return true;
                }
                _ => return false,
            }
        }
        if mods.control && !mods.alt {
            if !mods.shift && key.eq_ignore_ascii_case("z") {
                self.picture_edit_undo(window, cx);
                cx.stop_propagation();
                return true;
            }
            if (mods.shift && key.eq_ignore_ascii_case("z"))
                || (!mods.shift && key.eq_ignore_ascii_case("y"))
            {
                self.picture_edit_redo(window, cx);
                cx.stop_propagation();
                return true;
            }
        }
        match key {
            "delete" | "backspace" => {
                if self
                    .picture_edit
                    .as_ref()
                    .is_some_and(|e| e.selected.is_some())
                {
                    self.picture_edit_delete_selected(window, cx);
                    cx.stop_propagation();
                    return true;
                }
                false
            }
            "enter" => {
                if self
                    .picture_edit
                    .as_ref()
                    .is_some_and(|e| e.tool == Tool::Crop)
                {
                    self.picture_edit_apply_crop(window, cx);
                    cx.stop_propagation();
                    return true;
                }
                false
            }
            "escape" => {
                cx.stop_propagation();
                let Some(ed) = self.picture_edit.as_mut() else {
                    return false;
                };
                if ed.confirm_discard || ed.confirm_replace {
                    ed.confirm_discard = false;
                    ed.confirm_replace = false;
                    cx.notify();
                } else if ed.tool == Tool::Crop {
                    ed.tool = Tool::Select;
                    ed.crop_rect = None;
                    cx.notify();
                } else if ed.selected.is_some() {
                    ed.selected = None;
                    cx.notify();
                } else {
                    let host = ed.host;
                    self.request_picture_edit_exit(host, window, cx);
                }
                true
            }
            _ => false,
        }
    }

    pub(super) fn picture_edit_view(
        &self,
        p: crate::appearance::Palette,
        _window: &Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(ed) = &self.picture_edit else {
            return div().into_any_element();
        };
        let host = ed.host;
        let active_tool = ed.tool;
        let active_ink = ed.ink;
        let active_stroke = ed.stroke_size;
        let can_undo = !ed.undo_stack.is_empty();
        let can_redo = !ed.redo_stack.is_empty();
        let exporting = ed.exporting.is_some();
        let export_pct = ed
            .exporting
            .as_ref()
            .map(|j| j.progress.load(Ordering::Relaxed))
            .unwrap_or(0);
        let redacted = has_redaction(&ed.steps);
        let strip_meta = ed.effective_strip_metadata();
        let is_heif = ed.loaded.is_heif;
        let live_start = first_live_mark(&ed.steps);
        let selected_idx = ed
            .selected
            .filter(|&i| i >= live_start && i < ed.steps.len());
        let selected_is_text = selected_idx
            .and_then(|i| ed.steps.get(i))
            .is_some_and(|s| matches!(s, Step::Text { .. }));
        let selected_frame: Option<MarkFrame> = selected_idx
            .and_then(|i| ed.steps.get(i))
            .and_then(mark_frame);
        let file_name = ed
            .loaded
            .path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "picture".into());

        // Restrained Seek dark-glass destructive pill styling
        let danger_bg = rgba(0xE11D4826);
        let danger_border = rgba(0xE11D4866);
        let danger_text = rgb(0xFDA4AF);

        // Optically centered compact pill button helper
        let pill_btn = |id: &'static str, label: &'static str, active: bool| {
            div()
                .id(id)
                .role(Role::Button)
                .aria_label(label)
                .h(px(28.))
                .px(px(9.))
                .rounded(crate::tokens::RADIUS_CONTROL)
                .flex()
                .items_center()
                .justify_center()
                .cursor_pointer()
                .text_size(px(12.))
                .text_color(if active { p.text } else { p.muted })
                .when(active, |d| d.bg(p.control_selected))
                .hover(|s| s.bg(p.control_hover).text_color(p.text))
                .child(super::interface::button_text(label))
        };

        let mut toolbar = div()
            .flex_shrink_0()
            .flex()
            .flex_wrap()
            .items_center()
            .justify_between()
            .gap_2()
            .px_3()
            .py_2()
            .border_b_1()
            .border_color(p.line)
            .bg(p.chrome);

        // Left group: tools + transforms + 7 inks + S/M/L thickness
        let mut left = div().flex().flex_wrap().items_center().gap_1();
        for tool in [
            Tool::Select,
            Tool::Crop,
            Tool::Pen,
            Tool::Highlighter,
            Tool::Arrow,
            Tool::Rectangle,
            Tool::Ellipse,
            Tool::Text,
            Tool::Redact,
        ] {
            let id: &'static str = match tool {
                Tool::Select => "pic-tool-select",
                Tool::Crop => "pic-tool-crop",
                Tool::Pen => "pic-tool-pen",
                Tool::Highlighter => "pic-tool-highlight",
                Tool::Arrow => "pic-tool-arrow",
                Tool::Rectangle => "pic-tool-rect",
                Tool::Ellipse => "pic-tool-ellipse",
                Tool::Text => "pic-tool-text",
                Tool::Redact => "pic-tool-redact",
            };
            left = left.child(
                pill_btn(id, tool.label(), active_tool == tool)
                    .when(!exporting, |d| {
                        d.on_click(cx.listener(move |this, _, window, cx| {
                            this.picture_edit_select_tool(tool, window, cx);
                        }))
                    }),
            );
        }

        // Quick 90° Rotate and Horizontal Flip buttons
        left = left
            .child(div().w(px(1.)).h(px(16.)).mx_1().bg(p.line))
            .child(
                pill_btn("pic-rotate-cw", "Rotate 90°", false)
                    .when(!exporting, |d| {
                        d.on_click(cx.listener(|this, _, window, cx| {
                            this.picture_edit_apply_transform(
                                Step::Rotate { quarter_turns: 1 },
                                window,
                                cx,
                            );
                        }))
                    }),
            )
            .child(
                pill_btn("pic-flip-h", "Flip", false)
                    .when(!exporting, |d| {
                        d.on_click(cx.listener(|this, _, window, cx| {
                            this.picture_edit_apply_transform(
                                Step::Flip { vertical: false },
                                window,
                                cx,
                            );
                        }))
                    }),
            );

        // 7 curated ink swatches
        let mut inks = div().flex().items_center().gap(px(2.)).ml_1();
        for ink in Ink::ALL {
            let [r, g, b, _] = ink.rgba();
            let swatch_color =
                rgba(((r as u32) << 24) | ((g as u32) << 16) | ((b as u32) << 8) | 0xFF);
            let is_active = active_ink == ink;
            let id: &'static str = match ink {
                Ink::Red => "pic-ink-red",
                Ink::Orange => "pic-ink-orange",
                Ink::Yellow => "pic-ink-yellow",
                Ink::Green => "pic-ink-green",
                Ink::Blue => "pic-ink-blue",
                Ink::White => "pic-ink-white",
                Ink::Black => "pic-ink-black",
            };
            inks = inks.child(
                div()
                    .id(id)
                    .role(Role::Button)
                    .aria_label(format!("Ink color: {}", ink.name()))
                    .size(px(22.))
                    .rounded_full()
                    .flex()
                    .items_center()
                    .justify_center()
                    .cursor_pointer()
                    .border_2()
                    .border_color(if is_active { p.text } else { rgba(0).into() })
                    .child(
                        div()
                            .size(px(13.))
                            .rounded_full()
                            .bg(swatch_color)
                            .border_1()
                            .border_color(p.line),
                    )
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.picture_edit_pick_ink(ink, window, cx);
                    })),
            );
        }
        left = left.child(inks);

        // S / M / L stroke thickness selector
        let mut sizes = div()
            .flex()
            .items_center()
            .gap(px(1.))
            .ml_1()
            .p(px(2.))
            .rounded(crate::tokens::RADIUS_CONTROL)
            .bg(p.pane)
            .border_1()
            .border_color(p.line);
        for sz in StrokeSize::ALL {
            let active = active_stroke == sz;
            let id: &'static str = match sz {
                StrokeSize::Small => "pic-stroke-s",
                StrokeSize::Medium => "pic-stroke-m",
                StrokeSize::Large => "pic-stroke-l",
            };
            sizes = sizes.child(
                div()
                    .id(id)
                    .role(Role::Button)
                    .aria_label(format!("Stroke size {}", sz.label()))
                    .w(px(22.))
                    .h(px(22.))
                    .rounded(px(4.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .cursor_pointer()
                    .text_size(px(11.))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(if active { p.text } else { p.muted })
                    .when(active, |d| d.bg(p.control_selected))
                    .hover(|s| s.text_color(p.text))
                    .child(super::interface::button_text(sz.label()))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        if let Some(ed) = this.picture_edit.as_mut() {
                            ed.stroke_size = sz;
                            ed.focus.focus(window, cx);
                        }
                        cx.notify();
                    })),
            );
        }
        left = left.child(sizes);

        if let Some(idx) = selected_idx {
            left = left.child(div().w(px(1.)).h(px(16.)).mx_1().bg(p.line));
            if selected_is_text {
                left = left.child(
                    pill_btn("pic-edit-text", "Edit text", false).when(!exporting, |d| {
                        d.on_click(cx.listener(move |this, _, window, cx| {
                            let at = this
                                .picture_edit
                                .as_ref()
                                .and_then(|e| e.steps.get(idx))
                                .and_then(|s| match s {
                                    Step::Text { at, .. } => Some(*at),
                                    _ => None,
                                })
                                .unwrap_or(PointF::new(24.0, 24.0));
                            this.picture_edit_open_text_prompt(at, Some(idx), window, cx);
                        }))
                    }),
                );
            }
            left = left.child(
                pill_btn("pic-delete-mark", "Delete", false).when(!exporting, |d| {
                    d.on_click(cx.listener(|this, _, window, cx| {
                        this.picture_edit_delete_selected(window, cx);
                    }))
                }),
            );
        }

        // Right group: Undo, Redo, Save split button, Done
        let owner = cx.entity().downgrade();
        let save_menu_btn = Button::new("pic-save-options")
            .ghost()
            .h(px(28.))
            .w(px(26.))
            .px_0()
            .rounded_r(crate::tokens::RADIUS_CONTROL)
            .accessibility_label("Save options")
            .tooltip("Save options")
            .disabled(exporting)
            .icon(Glyph::ChevronDown)
            .dropdown_menu(move |menu, window, _| {
                let o1 = owner.clone();
                let o2 = owner.clone();
                let o3 = owner.clone();
                let mut m = super::daily::menu_style(menu, window).item(
                    PopupMenuItem::new("Save a Copy").on_click(move |_, window, cx| {
                        let _ = o1.update(cx, |this, cx| {
                            this.picture_edit_start_export(SaveMode::SaveCopy, window, cx);
                        });
                    }),
                );
                if !is_heif {
                    m = m.item(PopupMenuItem::new("Replace Original…").on_click(
                        move |_, _, cx| {
                            let _ = o2.update(cx, |this, cx| {
                                if let Some(ed) = this.picture_edit.as_mut() {
                                    ed.confirm_replace = true;
                                    ed.confirm_discard = false;
                                }
                                cx.notify();
                            });
                        },
                    ));
                }
                let meta_label = if redacted {
                    "Strip metadata (EXIF) · required when redacted"
                } else {
                    "Strip metadata (EXIF)"
                };
                m.separator().item(
                    PopupMenuItem::new(meta_label)
                        .checked(strip_meta)
                        .disabled(redacted)
                        .on_click(move |_, _, cx| {
                            let _ = o3.update(cx, |this, cx| {
                                if let Some(ed) = this.picture_edit.as_mut()
                                    && !has_redaction(&ed.steps)
                                {
                                    ed.strip_metadata = !ed.strip_metadata;
                                }
                                cx.notify();
                            });
                        }),
                )
            });

        let right = div()
            .flex()
            .items_center()
            .gap_1()
            .child(
                pill_btn("pic-undo", "Undo", false)
                    .when(!can_undo || exporting, |d| d.opacity(0.45).cursor_default())
                    .when(can_undo && !exporting, |d| {
                        d.on_click(cx.listener(|this, _, window, cx| {
                            this.picture_edit_undo(window, cx);
                        }))
                    }),
            )
            .child(
                pill_btn("pic-redo", "Redo", false)
                    .when(!can_redo || exporting, |d| d.opacity(0.45).cursor_default())
                    .when(can_redo && !exporting, |d| {
                        d.on_click(cx.listener(|this, _, window, cx| {
                            this.picture_edit_redo(window, cx);
                        }))
                    }),
            )
            .child(
                div()
                    .ml_1()
                    .flex()
                    .items_center()
                    .h(px(28.))
                    .rounded(crate::tokens::RADIUS_CONTROL)
                    .bg(p.accent.opacity(0.16))
                    .border_1()
                    .border_color(p.accent.opacity(0.45))
                    .child(
                        div()
                            .id("pic-save-copy")
                            .role(Role::Button)
                            .aria_label("Save a Copy")
                            .h_full()
                            .px(px(12.))
                            .rounded_l(crate::tokens::RADIUS_CONTROL)
                            .flex()
                            .items_center()
                            .justify_center()
                            .cursor_pointer()
                            .text_size(px(12.))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(p.text)
                            .hover(|s| s.bg(p.accent.opacity(0.24)))
                            .when(exporting, |d| d.opacity(0.5).cursor_default())
                            .child(super::interface::button_text("Save a Copy"))
                            .when(!exporting, |d| {
                                d.on_click(cx.listener(|this, _, window, cx| {
                                    this.picture_edit_start_export(
                                        SaveMode::SaveCopy,
                                        window,
                                        cx,
                                    );
                                }))
                            }),
                    )
                    .child(div().w(px(1.)).h(px(16.)).bg(p.accent.opacity(0.32)))
                    .child(save_menu_btn),
            )
            .child(
                pill_btn("pic-done", "Done", false).on_click(cx.listener(
                    move |this, _, window, cx| {
                        this.request_picture_edit_exit(host, window, cx);
                    },
                )),
            );

        toolbar = toolbar.child(left).child(right);

        // Optional sub-bars: Crop controls / Text prompt / Confirmations / Export / Error
        let mut sub_bars: Vec<AnyElement> = Vec::new();

        if let Some(crop) = ed.crop_rect
            && active_tool == Tool::Crop
        {
            let cw = crop.w.round().max(1.0) as u32;
            let ch = crop.h.round().max(1.0) as u32;
            let current_aspect = ed.crop_aspect;
            let crop_bar = div()
                .flex_shrink_0()
                .px_4()
                .py_2()
                .flex()
                .items_center()
                .justify_between()
                .gap_3()
                .bg(p.pane)
                .border_b_1()
                .border_color(p.line)
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .child(
                            div()
                                .font_family(crate::tokens::FONT_MONO)
                                .text_size(px(12.))
                                .text_color(p.text)
                                .child(format!("{cw} × {ch} px")),
                        )
                        .child(div().w(px(1.)).h(px(14.)).mx_1().bg(p.line))
                        .children(AspectPreset::ALL.into_iter().map(|preset| {
                            let active = current_aspect == preset;
                            let id: &'static str = match preset {
                                AspectPreset::Free => "pic-crop-free",
                                AspectPreset::Square => "pic-crop-1-1",
                                AspectPreset::FourThree => "pic-crop-4-3",
                                AspectPreset::SixteenNine => "pic-crop-16-9",
                            };
                            div()
                                .id(id)
                                .role(Role::Button)
                                .aria_label(preset.label())
                                .h(px(26.))
                                .px(px(10.))
                                .rounded(crate::tokens::RADIUS_CONTROL)
                                .flex()
                                .items_center()
                                .justify_center()
                                .cursor_pointer()
                                .text_size(px(12.))
                                .text_color(if active { p.text } else { p.muted })
                                .when(active, |d| d.bg(p.control_selected))
                                .hover(|s| s.bg(p.control_hover).text_color(p.text))
                                .child(super::interface::button_text(preset.label()))
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    if let Some(ed) = this.picture_edit.as_mut() {
                                        ed.crop_aspect = preset;
                                        if let Some(rect) = ed.crop_rect {
                                            ed.crop_rect = Some(apply_aspect_preset(
                                                rect,
                                                preset,
                                                ed.preview_size.0 as f32,
                                                ed.preview_size.1 as f32,
                                            ));
                                        }
                                        ed.focus.focus(window, cx);
                                    }
                                    cx.notify();
                                }))
                        })),
                )
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .child(
                            pill_btn("pic-crop-cancel", "Cancel", false).on_click(cx.listener(
                                |this, _, window, cx| {
                                    this.picture_edit_select_tool(Tool::Select, window, cx);
                                },
                            )),
                        )
                        .child(
                            div()
                                .id("pic-crop-apply")
                                .role(Role::Button)
                                .aria_label("Apply crop")
                                .h(px(28.))
                                .px(px(12.))
                                .rounded(crate::tokens::RADIUS_CONTROL)
                                .flex()
                                .items_center()
                                .justify_center()
                                .cursor_pointer()
                                .bg(p.accent.opacity(0.22))
                                .border_1()
                                .border_color(p.accent.opacity(0.45))
                                .text_size(px(12.))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(p.text)
                                .hover(|s| s.bg(p.accent.opacity(0.32)))
                                .child(super::interface::button_text("Apply crop"))
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.picture_edit_apply_crop(window, cx);
                                })),
                        ),
                );
            sub_bars.push(crop_bar.into_any_element());
        }

        if let Some(prompt) = &ed.text_prompt {
            sub_bars.push(
                div()
                    .flex_shrink_0()
                    .px_4()
                    .py_2()
                    .flex()
                    .items_center()
                    .gap_2()
                    .bg(p.pane)
                    .border_b_1()
                    .border_color(p.line)
                    .child(div().flex_1().child(Input::new(&prompt.input).h(px(30.))))
                    .child(
                        div()
                            .id("pic-text-commit")
                            .role(Role::Button)
                            .aria_label("Place text")
                            .h(px(28.))
                            .px(px(12.))
                            .rounded(crate::tokens::RADIUS_CONTROL)
                            .flex()
                            .items_center()
                            .justify_center()
                            .cursor_pointer()
                            .bg(p.accent.opacity(0.22))
                            .border_1()
                            .border_color(p.accent.opacity(0.45))
                            .text_size(px(12.))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(p.text)
                            .hover(|s| s.bg(p.accent.opacity(0.32)))
                            .child(super::interface::button_text("Place text"))
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.picture_edit_commit_text(window, cx);
                            })),
                    )
                    .child(
                        pill_btn("pic-text-cancel", "Cancel", false).on_click(cx.listener(
                            |this, _, window, cx| {
                                if let Some(ed) = this.picture_edit.as_mut() {
                                    ed.text_prompt = None;
                                    ed.focus.focus(window, cx);
                                }
                                cx.notify();
                            },
                        )),
                    )
                    .into_any_element(),
            );
        }

        if ed.confirm_discard {
            sub_bars.push(
                div()
                    .flex_shrink_0()
                    .px_4()
                    .py_2()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap_3()
                    .bg(p.pane)
                    .border_b_1()
                    .border_color(p.line)
                    .child(
                        div()
                            .text_size(px(12.))
                            .text_color(p.text)
                            .child("Discard unsaved picture edits?"),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(
                                pill_btn("pic-keep-editing", "Keep editing", false).on_click(
                                    cx.listener(|this, _, window, cx| {
                                        if let Some(ed) = this.picture_edit.as_mut() {
                                            ed.confirm_discard = false;
                                            ed.focus.focus(window, cx);
                                        }
                                        cx.notify();
                                    }),
                                ),
                            )
                            .child(
                                div()
                                    .id("pic-confirm-discard")
                                    .role(Role::Button)
                                    .aria_label("Discard")
                                    .h(px(28.))
                                    .px(px(12.))
                                    .rounded(crate::tokens::RADIUS_CONTROL)
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .cursor_pointer()
                                    .bg(danger_bg)
                                    .border_1()
                                    .border_color(danger_border)
                                    .text_size(px(12.))
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(danger_text)
                                    .hover(|s| s.bg(rgba(0xE11D4840)))
                                    .child(super::interface::button_text("Discard"))
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.close_picture_edit_now(window, cx);
                                    })),
                            ),
                    )
                    .into_any_element(),
            );
        }

        if ed.confirm_replace {
            sub_bars.push(
                div()
                    .flex_shrink_0()
                    .px_4()
                    .py_2()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap_3()
                    .bg(p.pane)
                    .border_b_1()
                    .border_color(p.line)
                    .child(
                        div()
                            .text_size(px(12.))
                            .text_color(p.text)
                            .child(format!(
                                "Replace {file_name}? The original file will be overwritten."
                            )),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(
                                pill_btn("pic-cancel-replace", "Cancel", false).on_click(
                                    cx.listener(|this, _, window, cx| {
                                        if let Some(ed) = this.picture_edit.as_mut() {
                                            ed.confirm_replace = false;
                                            ed.focus.focus(window, cx);
                                        }
                                        cx.notify();
                                    }),
                                ),
                            )
                            .child(
                                div()
                                    .id("pic-confirm-replace")
                                    .role(Role::Button)
                                    .aria_label("Replace Original")
                                    .h(px(28.))
                                    .px(px(12.))
                                    .rounded(crate::tokens::RADIUS_CONTROL)
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .cursor_pointer()
                                    .bg(danger_bg)
                                    .border_1()
                                    .border_color(danger_border)
                                    .text_size(px(12.))
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(danger_text)
                                    .hover(|s| s.bg(rgba(0xE11D4840)))
                                    .child(super::interface::button_text("Replace Original"))
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.picture_edit_start_export(
                                            SaveMode::ReplaceOriginal,
                                            window,
                                            cx,
                                        );
                                    })),
                            ),
                    )
                    .into_any_element(),
            );
        }

        if let Some(job) = &ed.exporting {
            let cancel = job.cancel.clone();
            let label = match job.mode {
                SaveMode::SaveCopy => format!("Saving full-resolution copy… {export_pct}%"),
                SaveMode::ReplaceOriginal => format!("Saving full-resolution file… {export_pct}%"),
            };
            sub_bars.push(
                div()
                    .flex_shrink_0()
                    .px_4()
                    .py_2()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap_3()
                    .bg(p.pane)
                    .border_b_1()
                    .border_color(p.line)
                    .child(div().text_size(px(12.)).text_color(p.text).child(label))
                    .child(
                        pill_btn("pic-cancel-export", "Cancel", false).on_click(cx.listener(
                            move |_, _, _, cx| {
                                cancel.store(true, Ordering::Relaxed);
                                cx.notify();
                            },
                        )),
                    )
                    .into_any_element(),
            );
        }

        if let Some(err) = &ed.error {
            sub_bars.push(
                div()
                    .flex_shrink_0()
                    .px_4()
                    .py_1()
                    .flex()
                    .items_center()
                    .justify_between()
                    .bg(danger_bg)
                    .border_b_1()
                    .border_color(danger_border)
                    .child(div().text_size(px(12.)).text_color(p.text).child(err.clone()))
                    .child(
                        pill_btn("pic-dismiss-err", "Dismiss", false).on_click(cx.listener(
                            |this, _, _, cx| {
                                if let Some(ed) = this.picture_edit.as_mut() {
                                    ed.error = None;
                                }
                                cx.notify();
                            },
                        )),
                    )
                    .into_any_element(),
            );
        }

        // Interactive canvas: static baked image + 60 FPS GPU vector overlay + handles + 8-handle crop
        let bounds_cell = ed.canvas_bounds.clone();
        let preview_image = ed.baked_preview.clone();
        let ibox = ed.image_box();
        let live_marks: Vec<Step> = ed.steps[live_start..].to_vec();
        let crop_rect = (active_tool == Tool::Crop).then_some(ed.crop_rect).flatten();

        let mut overlay = div()
            .absolute()
            .inset_0()
            .child(
                canvas(
                    move |bounds, _, _| {
                        bounds_cell.set(Some(bounds));
                    },
                    |_, _, _, _| {},
                )
                .size_full(),
            );

        if let Some(ibox) = ibox {
            let img_x = ibox.x - ibox.canvas_x;
            let img_y = ibox.y - ibox.canvas_y;
            overlay = overlay
                .child(
                    div()
                        .absolute()
                        .left(px(img_x))
                        .top(px(img_y))
                        .w(px(ibox.w))
                        .h(px(ibox.h))
                        .child(img(preview_image).size_full().object_fit(ObjectFit::Fill)),
                )
                // GPU-accelerated live vector mark layer
                .child(
                    canvas(
                        |_, _, _| {},
                        move |_bounds, _, window, cx| {
                            paint_live_marks(&live_marks, ibox, window, cx);
                        },
                    )
                    .absolute()
                    .inset_0(),
                );

            // Selection handles for live mark
            if let Some(frame) = selected_frame
                && active_tool != Tool::Crop
            {
                let accent = p.accent;
                if let Some((from, to)) = frame.endpoints {
                    for (idx, end_pt) in [from, to].into_iter().enumerate() {
                        let (lx, ly) = ibox.image_to_local(end_pt);
                        overlay = overlay.child(
                            div()
                                .id(("pic-end-handle", idx))
                                .absolute()
                                .left(px(lx - 6.0))
                                .top(px(ly - 6.0))
                                .size(px(12.0))
                                .rounded_full()
                                .bg(rgb(0xFFFFFF))
                                .border_2()
                                .border_color(accent),
                        );
                    }
                } else {
                    let (rx, ry) = ibox.image_to_local(PointF::new(frame.rect.x, frame.rect.y));
                    let rw = (frame.rect.w * ibox.scale).max(8.0);
                    let rh = (frame.rect.h * ibox.scale).max(8.0);
                    if frame.rotation.abs() < 0.01 {
                        overlay = overlay.child(
                            div()
                                .absolute()
                                .left(px(rx))
                                .top(px(ry))
                                .w(px(rw))
                                .h(px(rh))
                                .border_1()
                                .border_color(accent),
                        );
                    }
                    for (idx, corner) in frame.corners_world().into_iter().enumerate() {
                        let (cx_px, cy_px) = ibox.image_to_local(corner);
                        overlay = overlay.child(
                            div()
                                .id(("pic-corner-handle", idx))
                                .absolute()
                                .left(px(cx_px - 5.0))
                                .top(px(cy_px - 5.0))
                                .size(px(10.0))
                                .rounded(px(2.0))
                                .bg(rgb(0xFFFFFF))
                                .border_2()
                                .border_color(accent),
                        );
                    }
                    let (tx, ty) = ibox.image_to_local(frame.turn_handle(24.0 / ibox.scale));
                    overlay = overlay.child(
                        div()
                            .id("pic-turn-handle")
                            .absolute()
                            .left(px(tx - 6.0))
                            .top(px(ty - 6.0))
                            .size(px(12.0))
                            .rounded_full()
                            .bg(accent)
                            .border_2()
                            .border_color(rgb(0xFFFFFF)),
                    );
                }
            }

            // Crop overlay: darkened outer masks + rule-of-thirds guides + 4 corners + 4 edge grab-bars
            if let Some(crop) = crop_rect {
                let (cx0, cy0) = ibox.image_to_local(PointF::new(crop.x, crop.y));
                let cw = (crop.w * ibox.scale).max(4.0);
                let ch = (crop.h * ibox.scale).max(4.0);
                let shade = rgba(0x0000008C);
                let guide = rgba(0xFFFFFF44);
                overlay = overlay
                    .child(
                        div()
                            .absolute()
                            .left(px(img_x))
                            .top(px(img_y))
                            .w(px(ibox.w))
                            .h(px((cy0 - img_y).max(0.0)))
                            .bg(shade),
                    )
                    .child(
                        div()
                            .absolute()
                            .left(px(img_x))
                            .top(px(cy0 + ch))
                            .w(px(ibox.w))
                            .h(px(((img_y + ibox.h) - (cy0 + ch)).max(0.0)))
                            .bg(shade),
                    )
                    .child(
                        div()
                            .absolute()
                            .left(px(img_x))
                            .top(px(cy0))
                            .w(px((cx0 - img_x).max(0.0)))
                            .h(px(ch))
                            .bg(shade),
                    )
                    .child(
                        div()
                            .absolute()
                            .left(px(cx0 + cw))
                            .top(px(cy0))
                            .w(px(((img_x + ibox.w) - (cx0 + cw)).max(0.0)))
                            .h(px(ch))
                            .bg(shade),
                    )
                    // Rule-of-thirds hairline guides
                    .child(
                        div()
                            .absolute()
                            .left(px(cx0 + cw / 3.0))
                            .top(px(cy0))
                            .w(px(1.0))
                            .h(px(ch))
                            .bg(guide),
                    )
                    .child(
                        div()
                            .absolute()
                            .left(px(cx0 + cw * 2.0 / 3.0))
                            .top(px(cy0))
                            .w(px(1.0))
                            .h(px(ch))
                            .bg(guide),
                    )
                    .child(
                        div()
                            .absolute()
                            .left(px(cx0))
                            .top(px(cy0 + ch / 3.0))
                            .w(px(cw))
                            .h(px(1.0))
                            .bg(guide),
                    )
                    .child(
                        div()
                            .absolute()
                            .left(px(cx0))
                            .top(px(cy0 + ch * 2.0 / 3.0))
                            .w(px(cw))
                            .h(px(1.0))
                            .bg(guide),
                    )
                    // Crisp white border
                    .child(
                        div()
                            .absolute()
                            .left(px(cx0))
                            .top(px(cy0))
                            .w(px(cw))
                            .h(px(ch))
                            .border_2()
                            .border_color(rgb(0xFFFFFF)),
                    );
                // 4 corner handles
                for (idx, corner) in crop.corners().into_iter().enumerate() {
                    let (hx, hy) = ibox.image_to_local(corner);
                    overlay = overlay.child(
                        div()
                            .id(("pic-crop-corner", idx))
                            .absolute()
                            .left(px(hx - 6.0))
                            .top(px(hy - 6.0))
                            .size(px(12.0))
                            .rounded(px(2.0))
                            .bg(rgb(0xFFFFFF))
                            .border_1()
                            .border_color(rgb(0x111827)),
                    );
                }
                // 4 edge midpoint grab-bars (Top, Right, Bottom, Left)
                for (idx, (edge, mid)) in crop.edge_midpoints().into_iter().enumerate() {
                    let (mx, my) = ibox.image_to_local(mid);
                    let horizontal = matches!(edge, CropEdge::Top | CropEdge::Bottom);
                    let (bw, bh) = if horizontal { (22.0, 6.0) } else { (6.0, 22.0) };
                    overlay = overlay.child(
                        div()
                            .id(("pic-crop-edge", idx))
                            .absolute()
                            .left(px(mx - bw * 0.5))
                            .top(px(my - bh * 0.5))
                            .w(px(bw))
                            .h(px(bh))
                            .rounded(px(3.0))
                            .bg(rgb(0xFFFFFF))
                            .border_1()
                            .border_color(rgb(0x111827)),
                    );
                }
            }
        } else {
            overlay = overlay.child(
                div()
                    .size_full()
                    .p(px(20.))
                    .child(img(preview_image).size_full().object_fit(ObjectFit::Contain)),
            );
        }

        let footer_note = format!(
            "{} × {} px{}",
            ed.preview_size.0,
            ed.preview_size.1,
            if redacted {
                " · Redacted (pixels overwritten, EXIF stripped on save)"
            } else if strip_meta {
                " · EXIF stripped on save"
            } else {
                ""
            }
        );

        div()
            .id("picture-editor-root")
            .track_focus(&ed.focus)
            .size_full()
            .flex()
            .flex_col()
            .overflow_hidden()
            .on_key_down(cx.listener(|this, event, window, cx| {
                let _ = this.picture_edit_handle_key(event, window, cx);
            }))
            .child(toolbar)
            .children(sub_bars)
            .child(
                div()
                    .id("picture-editor-canvas")
                    .relative()
                    .flex_1()
                    .min_h_0()
                    .w_full()
                    .overflow_hidden()
                    .bg(p.pane)
                    .cursor(if active_tool == Tool::Select {
                        CursorStyle::Arrow
                    } else {
                        CursorStyle::Crosshair
                    })
                    .on_mouse_down(MouseButton::Left, cx.listener(Self::picture_edit_mouse_down))
                    .on_mouse_move(cx.listener(Self::picture_edit_mouse_move))
                    .on_mouse_up(MouseButton::Left, cx.listener(Self::picture_edit_mouse_up))
                    .child(overlay),
            )
            .child(
                div()
                    .flex_shrink_0()
                    .h(px(28.))
                    .px_4()
                    .flex()
                    .items_center()
                    .justify_between()
                    .border_t_1()
                    .border_color(p.line)
                    .bg(p.chrome)
                    .text_size(px(11.))
                    .text_color(p.muted)
                    .child(footer_note)
                    .child(if active_tool == Tool::Select {
                        "Click a mark to move, resize, rotate or recolour · Del removes"
                    } else if active_tool == Tool::Crop {
                        "Drag corners or edges to crop · Enter applies · Esc cancels"
                    } else if active_tool == Tool::Redact {
                        "Drag over sensitive areas to black them out permanently"
                    } else if active_tool == Tool::Text {
                        "Click anywhere on the picture to place text"
                    } else {
                        "Drag on the picture · Ctrl+Z undoes · Ctrl+Shift+Z redoes"
                    }),
            )
            .into_any_element()
    }
}
