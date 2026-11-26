//! Inline video preview, playback, and editing UI shared between Quick Look
//! and Video Document panes.
use super::*;
use crate::picture_edit::{
    AspectPreset, CropEdge, PointF, RectF, SaveMode, apply_aspect_preset, resize_crop_edge,
    to_render_image,
};
use crate::video_edit::{
    PlaybackStream, VideoEdit, VideoExportRequest, VideoInfo, export_video, extract_filmstrip,
    extract_frame, format_precise_time, format_timecode, probe, save_frame_png,
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
    time::{Duration, Instant},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum VideoHost {
    QuickLook,
    DocumentPane(crate::pane_layout::PaneId),
}

#[derive(Clone, Copy, Debug)]
enum TimelineDrag {
    Playhead,
    TrimStart,
    TrimEnd,
}

#[derive(Clone, Copy, Debug)]
enum VideoCropDrag {
    Move { offset_x: f32, offset_y: f32 },
    Corner(usize),
    Edge(CropEdge),
    New(PointF),
}

struct PlaybackJob {
    stop: Arc<AtomicBool>,
    _task: Task<()>,
}

struct VideoExportJob {
    cancel: Arc<AtomicBool>,
    progress: Arc<AtomicU8>,
    mode: SaveMode,
    _task: Task<()>,
}

#[derive(Clone, Copy, Debug)]
struct VideoBox {
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    scale: f32,
    canvas_x: f32,
    canvas_y: f32,
}

impl VideoBox {
    fn compute(bounds: Bounds<Pixels>, vid_w: u32, vid_h: u32) -> Option<Self> {
        let cw = f32::from(bounds.size.width);
        let ch = f32::from(bounds.size.height);
        let pad = 16.0;
        let avail_w = (cw - pad * 2.0).max(16.0);
        let avail_h = (ch - pad * 2.0).max(16.0);
        let vw = vid_w.max(1) as f32;
        let vh = vid_h.max(1) as f32;
        let scale = (avail_w / vw).min(avail_h / vh).max(1e-4);
        let w = vw * scale;
        let h = vh * scale;
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

    fn screen_to_video(self, pos: Point<Pixels>) -> PointF {
        PointF::new(
            (f32::from(pos.x) - self.x) / self.scale,
            (f32::from(pos.y) - self.y) / self.scale,
        )
    }

    fn video_to_local(self, pt: PointF) -> (f32, f32) {
        (
            (self.x - self.canvas_x) + pt.x * self.scale,
            (self.y - self.canvas_y) + pt.y * self.scale,
        )
    }
}

pub(super) struct State {
    pub(super) host: VideoHost,
    pub(super) path: PathBuf,
    info: Option<VideoInfo>,
    loading: bool,
    error: Option<String>,
    frame: Option<Arc<RenderImage>>,
    filmstrip: Vec<Arc<RenderImage>>,
    current_secs: f64,
    editing: bool,
    edit: VideoEdit,
    cropping: bool,
    crop_draft: Option<RectF>,
    crop_aspect: AspectPreset,
    crop_drag: Option<VideoCropDrag>,
    timeline_drag: Option<TimelineDrag>,
    canvas_bounds: Rc<Cell<Option<Bounds<Pixels>>>>,
    timeline_bounds: Rc<Cell<Option<Bounds<Pixels>>>>,
    playback: Option<PlaybackJob>,
    exporting: Option<VideoExportJob>,
    pub(super) confirm_discard: bool,
    confirm_replace: bool,
    load_gen: u64,
    scrub_gen: u64,
    focus: FocusHandle,
}

impl State {
    pub(super) fn is_dirty(&self) -> bool {
        self.info
            .as_ref()
            .is_some_and(|info| self.edit.is_dirty(info.duration))
    }

    fn rotated_dims(&self) -> (u32, u32) {
        self.info
            .as_ref()
            .map(|i| i.rotated_dimensions(self.edit.quarter_turns))
            .unwrap_or((16, 9))
    }

    fn video_box(&self) -> Option<VideoBox> {
        let bounds = self.canvas_bounds.get()?;
        let (w, h) = self.rotated_dims();
        VideoBox::compute(bounds, w, h)
    }

    fn start_crop(&mut self) {
        let (w, h) = self.rotated_dims();
        let wf = w.max(1) as f32;
        let hf = h.max(1) as f32;
        let base = self.edit.crop.unwrap_or_else(|| {
            let ix = wf * 0.08;
            let iy = hf * 0.08;
            RectF {
                x: ix,
                y: iy,
                w: (wf - ix * 2.0).max(16.0),
                h: (hf - iy * 2.0).max(16.0),
            }
        });
        self.crop_draft = Some(apply_aspect_preset(base, self.crop_aspect, wf, hf));
        self.cropping = true;
    }

    fn stop_playback(&mut self) {
        if let Some(job) = self.playback.take() {
            job.stop.store(true, Ordering::Relaxed);
        }
    }
}

impl Seek {
    pub(super) fn video_edit_for(&self, host: VideoHost, path: &std::path::Path) -> bool {
        self.video_edit
            .as_ref()
            .is_some_and(|v| v.host == host && v.path == path)
    }

    /// Ensure inline video player/editor state is initialized for `(host, path)`.
    pub(super) fn ensure_video_view(
        &mut self,
        host: VideoHost,
        path: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.video_edit_for(host, &path) {
            return;
        }
        if let Some(old) = self.video_edit.take() {
            if let Some(job) = old.playback {
                job.stop.store(true, Ordering::Relaxed);
            }
            if let Some(job) = old.exporting {
                job.cancel.store(true, Ordering::Relaxed);
            }
            if let Some(img) = old.frame {
                let _ = window.drop_image(img);
            }
            for thumb in old.filmstrip {
                let _ = window.drop_image(thumb);
            }
        }
        let load_gen = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(1);
        let focus = cx.focus_handle();
        self.video_edit = Some(State {
            host,
            path: path.clone(),
            info: None,
            loading: true,
            error: None,
            frame: None,
            filmstrip: Vec::new(),
            current_secs: 0.0,
            editing: false,
            edit: VideoEdit::identity(0.0),
            cropping: false,
            crop_draft: None,
            crop_aspect: AspectPreset::Free,
            crop_drag: None,
            timeline_drag: None,
            canvas_bounds: Rc::new(Cell::new(None)),
            timeline_bounds: Rc::new(Cell::new(None)),
            playback: None,
            exporting: None,
            confirm_discard: false,
            confirm_replace: false,
            load_gen,
            scrub_gen: 0,
            focus,
        });

        let (tx, rx) = std::sync::mpsc::channel();
        let path_bg = path.clone();
        std::thread::Builder::new()
            .name("seek-video-probe".into())
            .spawn(move || {
                let result = probe(&path_bg).and_then(|info| {
                    let first = extract_frame(&path_bg, 0.0, 1280, 0).map(|img| to_render_image(&img))?;
                    let strip = extract_filmstrip(&path_bg, info.duration, 8, 0);
                    Ok((info, first, strip))
                });
                let _ = tx.send(result);
            })
            .ok();

        cx.spawn_in(window, async move |this, cx| {
            loop {
                match rx.try_recv() {
                    Ok(result) => {
                        let _ = this.update_in(cx, |this, window, cx| {
                            if let Some(v) = this.video_edit.as_mut()
                                && v.load_gen == load_gen
                            {
                                v.loading = false;
                                match result {
                                    Ok((info, first, strip)) => {
                                        v.edit = VideoEdit::identity(info.duration);
                                        v.info = Some(info);
                                        if let Some(old) = v.frame.replace(first) {
                                            let _ = window.drop_image(old);
                                        }
                                        for old in v.filmstrip.drain(..) {
                                            let _ = window.drop_image(old);
                                        }
                                        v.filmstrip = strip;
                                    }
                                    Err(err) => {
                                        v.error = Some(err);
                                    }
                                }
                                cx.notify();
                            }
                        });
                        break;
                    }
                    Err(std::sync::mpsc::TryRecvError::Empty) => {
                        cx.background_executor()
                            .timer(Duration::from_millis(25))
                            .await;
                    }
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => break,
                }
            }
        })
        .detach();
    }

    pub(super) fn request_video_edit_exit(
        &mut self,
        host: VideoHost,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(v) = self.video_edit.as_mut().filter(|v| v.host == host) else {
            return false;
        };
        if v.exporting.is_some() {
            v.error = Some("Wait for video export to finish or click Cancel.".into());
            cx.notify();
            return true;
        }
        if v.editing && v.is_dirty() {
            v.stop_playback();
            v.confirm_discard = true;
            v.confirm_replace = false;
            cx.notify();
            return true;
        }
        self.close_video_edit_now(window, cx);
        false
    }

    pub(super) fn close_video_edit_now(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(mut v) = self.video_edit.take() {
            v.stop_playback();
            if let Some(job) = v.exporting.take() {
                job.cancel.store(true, Ordering::Relaxed);
            }
            if let Some(img) = v.frame.take() {
                let _ = window.drop_image(img);
            }
            for thumb in v.filmstrip.drain(..) {
                let _ = window.drop_image(thumb);
            }
            cx.notify();
        }
    }

    fn video_toggle_play(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(v) = self.video_edit.as_mut() else {
            return;
        };
        if v.playback.is_some() {
            v.stop_playback();
            cx.notify();
            return;
        }
        let Some(info) = v.info.clone() else {
            return;
        };
        if info.duration <= 0.05 {
            return;
        }
        let range_start = if v.editing { v.edit.trim_start } else { 0.0 };
        let range_end = if v.editing {
            v.edit.trim_end.max(range_start + 0.1)
        } else {
            info.duration
        };
        let mut start = v.current_secs;
        if start < range_start || start >= range_end - 0.08 {
            start = range_start;
            v.current_secs = start;
        }

        let stop = Arc::new(AtomicBool::new(false));
        let stop_bg = stop.clone();
        let path_bg = v.path.clone();
        let turns = v.edit.quarter_turns;
        let load_gen = v.load_gen;
        let (tx, rx) = std::sync::mpsc::sync_channel::<(f64, Arc<RenderImage>)>(2);

        std::thread::Builder::new()
            .name("seek-video-play".into())
            .spawn(move || {
                let Ok(mut stream) =
                    PlaybackStream::spawn(&path_bg, start, range_end, &info, turns, 960)
                else {
                    return;
                };
                let frame_dt = Duration::from_secs_f64(1.0 / stream.fps.max(1.0));
                let origin = Instant::now();
                let mut frame_idx: u64 = 0;
                while !stop_bg.load(Ordering::Relaxed) {
                    let Some(frame) = stream.next_frame() else {
                        break;
                    };
                    let ts = (start + frame_idx as f64 / stream.fps).min(range_end);
                    frame_idx += 1;
                    let target = origin + frame_dt.saturating_mul(frame_idx as u32);
                    let now = Instant::now();
                    if target > now {
                        std::thread::sleep(target - now);
                    }
                    if stop_bg.load(Ordering::Relaxed) || tx.send((ts, frame)).is_err() {
                        break;
                    }
                }
            })
            .ok();

        let stop_ui = stop.clone();
        let task = cx.spawn_in(window, async move |this, cx| {
            loop {
                if stop_ui.load(Ordering::Relaxed) {
                    break;
                }
                match rx.try_recv() {
                    Ok((ts, frame)) => {
                        let _ = this.update_in(cx, |this, window, cx| {
                            if let Some(v) = this.video_edit.as_mut()
                                && v.load_gen == load_gen
                                && v.playback.is_some()
                            {
                                v.current_secs = ts;
                                if let Some(old) = v.frame.replace(frame) {
                                    let _ = window.drop_image(old);
                                }
                                cx.notify();
                            }
                        });
                    }
                    Err(std::sync::mpsc::TryRecvError::Empty) => {
                        cx.background_executor()
                            .timer(Duration::from_millis(12))
                            .await;
                    }
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                        let _ = this.update(cx, |this, cx| {
                            if let Some(v) = this.video_edit.as_mut()
                                && v.load_gen == load_gen
                            {
                                v.playback = None;
                                cx.notify();
                            }
                        });
                        break;
                    }
                }
            }
        });
        v.playback = Some(PlaybackJob { stop, _task: task });
        cx.notify();
    }

    fn video_seek_to(&mut self, secs: f64, window: &mut Window, cx: &mut Context<Self>) {
        let Some(v) = self.video_edit.as_mut() else {
            return;
        };
        let duration = v.info.as_ref().map(|i| i.duration).unwrap_or(0.0);
        let clamped = secs.clamp(0.0, duration.max(0.0));
        v.stop_playback();
        v.current_secs = clamped;
        v.scrub_gen = v.scrub_gen.wrapping_add(1);
        let scrub_gen = v.scrub_gen;
        let load_gen = v.load_gen;
        let path = v.path.clone();
        let turns = v.edit.quarter_turns;
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::Builder::new()
            .name("seek-video-seek".into())
            .spawn(move || {
                let res = extract_frame(&path, clamped, 1280, turns).map(|img| to_render_image(&img));
                let _ = tx.send(res);
            })
            .ok();

        cx.spawn_in(window, async move |this, cx| {
            loop {
                match rx.try_recv() {
                    Ok(Ok(img)) => {
                        let _ = this.update_in(cx, |this, window, cx| {
                            if let Some(v) = this.video_edit.as_mut()
                                && v.load_gen == load_gen
                                && v.scrub_gen == scrub_gen
                            {
                                if let Some(old) = v.frame.replace(img) {
                                    let _ = window.drop_image(old);
                                }
                                cx.notify();
                            }
                        });
                        break;
                    }
                    Ok(Err(_)) | Err(std::sync::mpsc::TryRecvError::Disconnected) => break,
                    Err(std::sync::mpsc::TryRecvError::Empty) => {
                        cx.background_executor()
                            .timer(Duration::from_millis(16))
                            .await;
                    }
                }
            }
        })
        .detach();
        cx.notify();
    }

    fn video_save_frame(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(v) = self.video_edit.as_ref() else {
            return;
        };
        let Some(info) = v.info.clone() else {
            return;
        };
        let path = v.path.clone();
        let secs = v.current_secs;
        let turns = v.edit.quarter_turns;
        let crop = v.edit.crop;
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::Builder::new()
            .name("seek-video-frame".into())
            .spawn(move || {
                let _ = tx.send(save_frame_png(&path, secs, &info, turns, crop));
            })
            .ok();

        cx.spawn_in(window, async move |this, cx| {
            loop {
                match rx.try_recv() {
                    Ok(result) => {
                        let _ = this.update_in(cx, |this, window, cx| match result {
                            Ok(saved) => {
                                let name = saved
                                    .file_name()
                                    .map(|n| n.to_string_lossy().into_owned())
                                    .unwrap_or_else(|| "frame.png".into());
                                let _ = this.backend.tx.send(Command::Changed);
                                this.show_quiet_toast(format!("Saved frame as {name}"), window, cx);
                            }
                            Err(err) => {
                                if let Some(v) = this.video_edit.as_mut() {
                                    v.error = Some(err);
                                }
                                cx.notify();
                            }
                        });
                        break;
                    }
                    Err(std::sync::mpsc::TryRecvError::Empty) => {
                        cx.background_executor()
                            .timer(Duration::from_millis(25))
                            .await;
                    }
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => break,
                }
            }
        })
        .detach();
    }

    fn video_start_export(
        &mut self,
        mode: SaveMode,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(v) = self.video_edit.as_mut() else {
            return;
        };
        if v.exporting.is_some() {
            return;
        }
        let Some(info) = v.info.clone() else {
            return;
        };
        v.stop_playback();
        v.confirm_replace = false;
        v.confirm_discard = false;
        v.error = None;
        let req = VideoExportRequest {
            source: v.path.clone(),
            info,
            edit: v.edit.clone(),
            mode,
        };
        let cancel = Arc::new(AtomicBool::new(false));
        let progress = Arc::new(AtomicU8::new(0));
        let cancel_bg = cancel.clone();
        let progress_bg = progress.clone();
        let load_gen = v.load_gen;
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::Builder::new()
            .name("seek-video-export".into())
            .spawn(move || {
                let _ = tx.send(export_video(&req, &cancel_bg, &progress_bg));
            })
            .ok();

        let progress_ui = progress.clone();
        let task = cx.spawn_in(window, async move |this, cx| {
            loop {
                match rx.try_recv() {
                    Ok(result) => {
                        let _ = this.update_in(cx, |this, window, cx| {
                            if let Some(v) = this.video_edit.as_mut()
                                && v.load_gen == load_gen
                            {
                                v.exporting = None;
                                match result {
                                    Ok(saved) => {
                                        let name = saved
                                            .file_name()
                                            .map(|n| n.to_string_lossy().into_owned())
                                            .unwrap_or_else(|| "video".into());
                                        let msg = match mode {
                                            SaveMode::SaveCopy => format!("Saved copy as {name}"),
                                            SaveMode::ReplaceOriginal => format!("Saved {name}"),
                                        };
                                        let host = v.host;
                                        let path = v.path.clone();
                                        v.editing = false;
                                        v.cropping = false;
                                        v.crop_draft = None;
                                        this.media.force();
                                        let _ = this.backend.tx.send(Command::Changed);
                                        if mode == SaveMode::ReplaceOriginal {
                                            this.close_video_edit_now(window, cx);
                                            this.ensure_video_view(host, path, window, cx);
                                        }
                                        this.show_quiet_toast(msg, window, cx);
                                    }
                                    Err(err) => {
                                        v.error = Some(err);
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
                            .timer(Duration::from_millis(50))
                            .await;
                    }
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => break,
                }
            }
        });
        v.exporting = Some(VideoExportJob {
            cancel,
            progress,
            mode,
            _task: task,
        });
        cx.notify();
    }

    pub(super) fn video_handle_key(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(v) = self.video_edit.as_ref() else {
            return false;
        };
        let key = event.keystroke.key.as_str();
        let mods = event.keystroke.modifiers;
        if mods.control || mods.alt {
            return false;
        }
        match key {
            "space" => {
                self.video_toggle_play(window, cx);
                cx.stop_propagation();
                true
            }
            "left" => {
                let t = (v.current_secs - 1.0).max(0.0);
                self.video_seek_to(t, window, cx);
                cx.stop_propagation();
                true
            }
            "right" => {
                let dur = v.info.as_ref().map(|i| i.duration).unwrap_or(0.0);
                let t = (v.current_secs + 1.0).min(dur);
                self.video_seek_to(t, window, cx);
                cx.stop_propagation();
                true
            }
            "escape" => {
                if let Some(v) = self.video_edit.as_mut() {
                    if v.confirm_discard || v.confirm_replace {
                        v.confirm_discard = false;
                        v.confirm_replace = false;
                        cx.stop_propagation();
                        cx.notify();
                        return true;
                    }
                    if v.cropping {
                        v.cropping = false;
                        v.crop_draft = None;
                        cx.stop_propagation();
                        cx.notify();
                        return true;
                    }
                }
                false
            }
            _ => false,
        }
    }

    fn video_timeline_mouse_down(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(v) = self.video_edit.as_mut() else {
            return;
        };
        let Some(bounds) = v.timeline_bounds.get() else {
            return;
        };
        let Some(info) = v.info.as_ref() else {
            return;
        };
        let dur = info.duration.max(0.001);
        let w = f32::from(bounds.size.width).max(1.0);
        let rel_x = (f32::from(event.position.x) - f32::from(bounds.origin.x)).clamp(0.0, w);
        let clicked_secs = (rel_x / w) as f64 * dur;

        if v.editing {
            let start_px = (v.edit.trim_start / dur) as f32 * w;
            let end_px = (v.edit.trim_end / dur) as f32 * w;
            if (rel_x - start_px).abs() <= 14.0 {
                v.timeline_drag = Some(TimelineDrag::TrimStart);
                v.stop_playback();
                cx.notify();
                return;
            }
            if (rel_x - end_px).abs() <= 14.0 {
                v.timeline_drag = Some(TimelineDrag::TrimEnd);
                v.stop_playback();
                cx.notify();
                return;
            }
        }
        v.timeline_drag = Some(TimelineDrag::Playhead);
        self.video_seek_to(clicked_secs, window, cx);
    }

    fn video_timeline_mouse_move(
        &mut self,
        event: &MouseMoveEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(drag) = self.video_edit.as_ref().and_then(|v| v.timeline_drag) else {
            return;
        };
        if !event.dragging() {
            if let Some(v) = self.video_edit.as_mut() {
                v.timeline_drag = None;
            }
            return;
        }
        let Some((bounds, dur)) = self
            .video_edit
            .as_ref()
            .and_then(|v| Some((v.timeline_bounds.get()?, v.info.as_ref()?.duration)))
        else {
            return;
        };
        let w = f32::from(bounds.size.width).max(1.0);
        let rel_x = (f32::from(event.position.x) - f32::from(bounds.origin.x)).clamp(0.0, w);
        let secs = ((rel_x / w) as f64 * dur).clamp(0.0, dur);

        match drag {
            TimelineDrag::Playhead => {
                self.video_seek_to(secs, window, cx);
            }
            TimelineDrag::TrimStart => {
                if let Some(v) = self.video_edit.as_mut() {
                    let max_start = (v.edit.trim_end - 0.1).max(0.0);
                    v.edit.trim_start = secs.clamp(0.0, max_start);
                    let target = v.edit.trim_start;
                    self.video_seek_to(target, window, cx);
                }
            }
            TimelineDrag::TrimEnd => {
                if let Some(v) = self.video_edit.as_mut() {
                    let min_end = (v.edit.trim_start + 0.1).min(dur);
                    v.edit.trim_end = secs.clamp(min_end, dur);
                    let target = v.edit.trim_end;
                    self.video_seek_to(target, window, cx);
                }
            }
        }
    }

    fn video_canvas_mouse_down(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(v) = self.video_edit.as_mut() else {
            return;
        };
        if !v.cropping {
            self.video_toggle_play(window, cx);
            return;
        }
        let Some(vbox) = v.video_box() else {
            return;
        };
        let (vw, vh) = v.rotated_dims();
        let (vw, vh) = (vw as f32, vh as f32);
        let pt = vbox.screen_to_video(event.position);
        let handle_r = 12.0 / vbox.scale;
        let edge_band = 10.0 / vbox.scale;

        if let Some(crop) = v.crop_draft {
            for (i, corner) in crop.corners().into_iter().enumerate() {
                if ((pt.x - corner.x).powi(2) + (pt.y - corner.y).powi(2)).sqrt() <= handle_r * 1.4 {
                    v.crop_drag = Some(VideoCropDrag::Corner(i));
                    cx.notify();
                    return;
                }
            }
            for (edge, mid) in crop.edge_midpoints() {
                let near_mid =
                    ((pt.x - mid.x).powi(2) + (pt.y - mid.y).powi(2)).sqrt() <= handle_r * 1.45;
                let on_edge = match edge {
                    CropEdge::Top => {
                        (pt.y - crop.y).abs() <= edge_band && pt.x >= crop.x && pt.x <= crop.x + crop.w
                    }
                    CropEdge::Bottom => {
                        (pt.y - (crop.y + crop.h)).abs() <= edge_band
                            && pt.x >= crop.x
                            && pt.x <= crop.x + crop.w
                    }
                    CropEdge::Left => {
                        (pt.x - crop.x).abs() <= edge_band && pt.y >= crop.y && pt.y <= crop.y + crop.h
                    }
                    CropEdge::Right => {
                        (pt.x - (crop.x + crop.w)).abs() <= edge_band
                            && pt.y >= crop.y
                            && pt.y <= crop.y + crop.h
                    }
                };
                if near_mid || on_edge {
                    v.crop_drag = Some(VideoCropDrag::Edge(edge));
                    cx.notify();
                    return;
                }
            }
            if pt.x >= crop.x
                && pt.x <= crop.x + crop.w
                && pt.y >= crop.y
                && pt.y <= crop.y + crop.h
            {
                v.crop_drag = Some(VideoCropDrag::Move {
                    offset_x: pt.x - crop.x,
                    offset_y: pt.y - crop.y,
                });
                cx.notify();
                return;
            }
        }
        let clamped = PointF::new(pt.x.clamp(0.0, vw), pt.y.clamp(0.0, vh));
        v.crop_draft = Some(RectF {
            x: clamped.x,
            y: clamped.y,
            w: 16.0,
            h: 16.0,
        });
        v.crop_drag = Some(VideoCropDrag::New(clamped));
        cx.notify();
    }

    fn video_canvas_mouse_move(
        &mut self,
        event: &MouseMoveEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(v) = self.video_edit.as_mut() else {
            return;
        };
        let Some(drag) = v.crop_drag else {
            return;
        };
        if !event.dragging() {
            v.crop_drag = None;
            return;
        }
        let Some(vbox) = v.video_box() else {
            return;
        };
        let (vw, vh) = v.rotated_dims();
        let (vw, vh) = (vw as f32, vh as f32);
        let pt = vbox.screen_to_video(event.position);
        let clamped = PointF::new(pt.x.clamp(0.0, vw), pt.y.clamp(0.0, vh));

        match drag {
            VideoCropDrag::New(origin) => {
                let raw = RectF::from_points(origin, clamped);
                v.crop_draft = Some(apply_aspect_preset(raw, v.crop_aspect, vw, vh));
            }
            VideoCropDrag::Move { offset_x, offset_y } => {
                if let Some(mut crop) = v.crop_draft {
                    crop.x = (pt.x - offset_x).clamp(0.0, (vw - crop.w).max(0.0));
                    crop.y = (pt.y - offset_y).clamp(0.0, (vh - crop.h).max(0.0));
                    v.crop_draft = Some(crop);
                }
            }
            VideoCropDrag::Edge(edge) => {
                if let Some(crop) = v.crop_draft {
                    v.crop_draft = Some(resize_crop_edge(crop, edge, clamped, v.crop_aspect, vw, vh));
                }
            }
            VideoCropDrag::Corner(corner) => {
                if let Some(crop) = v.crop_draft {
                    let (mut x0, mut y0, mut x1, mut y1) =
                        (crop.x, crop.y, crop.x + crop.w, crop.y + crop.h);
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
                    v.crop_draft = Some(apply_aspect_preset(raw, v.crop_aspect, vw, vh));
                }
            }
        }
        cx.notify();
    }

    pub(super) fn video_edit_view(
        &mut self,
        host: VideoHost,
        path: PathBuf,
        p: crate::appearance::Palette,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        self.ensure_video_view(host, path, window, cx);
        let Some(v) = &self.video_edit else {
            return div().into_any_element();
        };

        let danger_bg = rgba(0xE11D4826);
        let danger_border = rgba(0xE11D4866);
        let danger_text = rgb(0xFDA4AF);

        let pill = |id: &'static str, label: &'static str, active: bool| {
            div()
                .id(id)
                .role(Role::Button)
                .aria_label(label)
                .h(px(28.))
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
                .child(super::interface::button_text(label))
        };

        let playing = v.playback.is_some();
        let editing = v.editing;
        let cropping = v.cropping;
        let exporting = v.exporting.is_some();
        let export_pct = v
            .exporting
            .as_ref()
            .map(|j| j.progress.load(Ordering::Relaxed))
            .unwrap_or(0);
        let dirty = v.is_dirty();
        let info = v.info.clone();
        let duration = info.as_ref().map(|i| i.duration).unwrap_or(0.0);
        let has_audio = info.as_ref().is_some_and(|i| i.has_audio);
        let muted = v.edit.muted;
        let has_crop = v.edit.crop.is_some();
        let file_name = v
            .path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "video".into());

        // Top edit bar (shown when editing is toggled on)
        let mut sub_bars: Vec<AnyElement> = Vec::new();
        if editing {
            let owner = cx.entity().downgrade();
            let save_menu_btn = Button::new("vid-save-options")
                .ghost()
                .h(px(28.))
                .w(px(26.))
                .px_0()
                .rounded_r(crate::tokens::RADIUS_CONTROL)
                .accessibility_label("Save video options")
                .tooltip("Save options")
                .disabled(exporting || !dirty)
                .icon(Glyph::ChevronDown)
                .dropdown_menu(move |menu, window, _| {
                    let o1 = owner.clone();
                    let o2 = owner.clone();
                    super::daily::menu_style(menu, window)
                        .item(PopupMenuItem::new("Save a Copy").on_click(
                            move |_, window, cx| {
                                let _ = o1.update(cx, |this, cx| {
                                    this.video_start_export(SaveMode::SaveCopy, window, cx);
                                });
                            },
                        ))
                        .item(PopupMenuItem::new("Replace Original…").on_click(
                            move |_, _, cx| {
                                let _ = o2.update(cx, |this, cx| {
                                    if let Some(v) = this.video_edit.as_mut() {
                                        v.confirm_replace = true;
                                        v.confirm_discard = false;
                                    }
                                    cx.notify();
                                });
                            },
                        ))
                });

            let trim_badge = format!(
                "{} – {} · {:.1}s",
                format_precise_time(v.edit.trim_start),
                format_precise_time(v.edit.trim_end),
                v.edit.trimmed_duration()
            );

            sub_bars.push(
                div()
                    .flex_shrink_0()
                    .px_3()
                    .py_2()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .justify_between()
                    .gap_2()
                    .bg(p.chrome)
                    .border_b_1()
                    .border_color(p.line)
                    .child(
                        div()
                            .flex()
                            .flex_wrap()
                            .items_center()
                            .gap_1()
                            .child(
                                div()
                                    .px(px(8.))
                                    .h(px(26.))
                                    .rounded(crate::tokens::RADIUS_CONTROL)
                                    .bg(p.pane)
                                    .border_1()
                                    .border_color(p.line)
                                    .flex()
                                    .items_center()
                                    .font_family(crate::tokens::FONT_MONO)
                                    .text_size(px(11.))
                                    .text_color(p.text)
                                    .child(trim_badge),
                            )
                            .child(
                                pill(
                                    "vid-crop",
                                    if has_crop { "Crop ✓" } else { "Crop" },
                                    cropping,
                                )
                                .when(!exporting, |d| {
                                    d.on_click(cx.listener(|this, _, _, cx| {
                                        if let Some(v) = this.video_edit.as_mut() {
                                            v.stop_playback();
                                            if v.cropping {
                                                v.cropping = false;
                                                v.crop_draft = None;
                                            } else {
                                                v.start_crop();
                                            }
                                        }
                                        cx.notify();
                                    }))
                                }),
                            )
                            .child(
                                pill("vid-rotate", "Rotate 90°", false).when(!exporting, |d| {
                                    d.on_click(cx.listener(|this, _, window, cx| {
                                        if let Some(v) = this.video_edit.as_mut() {
                                            v.stop_playback();
                                            v.edit.quarter_turns = (v.edit.quarter_turns + 1) % 4;
                                            v.edit.crop = None;
                                            v.crop_draft = None;
                                            v.cropping = false;
                                            let t = v.current_secs;
                                            this.video_seek_to(t, window, cx);
                                        }
                                    }))
                                }),
                            )
                            .child(
                                pill(
                                    "vid-mute",
                                    if muted { "Muted" } else { "Mute audio" },
                                    muted,
                                )
                                .when(!has_audio || exporting, |d| {
                                    d.opacity(0.45).cursor_default()
                                })
                                .when(has_audio && !exporting, |d| {
                                    d.on_click(cx.listener(|this, _, _, cx| {
                                        if let Some(v) = this.video_edit.as_mut() {
                                            v.edit.muted = !v.edit.muted;
                                        }
                                        cx.notify();
                                    }))
                                }),
                            )
                            .child(
                                pill("vid-save-frame", "Save frame", false).when(!exporting, |d| {
                                    d.on_click(cx.listener(|this, _, window, cx| {
                                        this.video_save_frame(window, cx);
                                    }))
                                }),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .h(px(28.))
                                    .rounded(crate::tokens::RADIUS_CONTROL)
                                    .bg(p.accent.opacity(0.16))
                                    .border_1()
                                    .border_color(p.accent.opacity(0.45))
                                    .when(!dirty || exporting, |d| d.opacity(0.5))
                                    .child(
                                        div()
                                            .id("vid-save-copy")
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
                                            .child(super::interface::button_text("Save a Copy"))
                                            .when(dirty && !exporting, |d| {
                                                d.on_click(cx.listener(|this, _, window, cx| {
                                                    this.video_start_export(
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
                            .child(pill("vid-done-edit", "Done", false).on_click(cx.listener(
                                |this, _, _, cx| {
                                    if let Some(v) = this.video_edit.as_mut() {
                                        if v.is_dirty() {
                                            v.confirm_discard = true;
                                        } else {
                                            v.editing = false;
                                            v.cropping = false;
                                            v.crop_draft = None;
                                        }
                                    }
                                    cx.notify();
                                },
                            ))),
                    )
                    .into_any_element(),
            );
        }

        // Crop controls bar
        if cropping && let Some(crop) = v.crop_draft {
            let cw = (crop.w.round().max(2.0) as u32) & !1;
            let ch = (crop.h.round().max(2.0) as u32) & !1;
            let current_aspect = v.crop_aspect;
            sub_bars.push(
                div()
                    .flex_shrink_0()
                    .px_4()
                    .py_2()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap_2()
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
                            .children(AspectPreset::ALL.into_iter().map(|preset| {
                                let active = current_aspect == preset;
                                let id: &'static str = match preset {
                                    AspectPreset::Free => "vid-crop-free",
                                    AspectPreset::Square => "vid-crop-1-1",
                                    AspectPreset::FourThree => "vid-crop-4-3",
                                    AspectPreset::SixteenNine => "vid-crop-16-9",
                                };
                                pill(id, preset.label(), active).on_click(cx.listener(
                                    move |this, _, _, cx| {
                                        if let Some(v) = this.video_edit.as_mut() {
                                            v.crop_aspect = preset;
                                            let (rw, rh) = v.rotated_dims();
                                            if let Some(r) = v.crop_draft {
                                                v.crop_draft = Some(apply_aspect_preset(
                                                    r,
                                                    preset,
                                                    rw as f32,
                                                    rh as f32,
                                                ));
                                            }
                                        }
                                        cx.notify();
                                    },
                                ))
                            })),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .when(has_crop, |d| {
                                d.child(pill("vid-crop-clear", "Reset crop", false).on_click(
                                    cx.listener(|this, _, _, cx| {
                                        if let Some(v) = this.video_edit.as_mut() {
                                            v.edit.crop = None;
                                            v.crop_draft = None;
                                            v.cropping = false;
                                        }
                                        cx.notify();
                                    }),
                                ))
                            })
                            .child(pill("vid-crop-cancel", "Cancel", false).on_click(cx.listener(
                                |this, _, _, cx| {
                                    if let Some(v) = this.video_edit.as_mut() {
                                        v.cropping = false;
                                        v.crop_draft = None;
                                    }
                                    cx.notify();
                                },
                            )))
                            .child(
                                div()
                                    .id("vid-crop-apply")
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
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        if let Some(v) = this.video_edit.as_mut() {
                                            v.edit.crop = v.crop_draft.take();
                                            v.cropping = false;
                                        }
                                        cx.notify();
                                    })),
                            ),
                    )
                    .into_any_element(),
            );
        }

        if v.confirm_discard {
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
                            .child("Discard unsaved video edits?"),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(pill("vid-keep-editing", "Keep editing", false).on_click(
                                cx.listener(|this, _, _, cx| {
                                    if let Some(v) = this.video_edit.as_mut() {
                                        v.confirm_discard = false;
                                    }
                                    cx.notify();
                                }),
                            ))
                            .child(
                                div()
                                    .id("vid-confirm-discard")
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
                                        if let Some(v) = this.video_edit.as_mut() {
                                            let dur = v.info.as_ref().map(|i| i.duration).unwrap_or(0.0);
                                            v.edit = VideoEdit::identity(dur);
                                            v.editing = false;
                                            v.cropping = false;
                                            v.crop_draft = None;
                                            v.confirm_discard = false;
                                            let t = v.current_secs;
                                            this.video_seek_to(t, window, cx);
                                        }
                                        cx.notify();
                                    })),
                            ),
                    )
                    .into_any_element(),
            );
        }

        if v.confirm_replace {
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
                                "Replace {file_name}? The original video will be overwritten."
                            )),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(pill("vid-cancel-replace", "Cancel", false).on_click(
                                cx.listener(|this, _, _, cx| {
                                    if let Some(v) = this.video_edit.as_mut() {
                                        v.confirm_replace = false;
                                    }
                                    cx.notify();
                                }),
                            ))
                            .child(
                                div()
                                    .id("vid-confirm-replace")
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
                                        this.video_start_export(
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

        if let Some(job) = &v.exporting {
            let cancel = job.cancel.clone();
            let label = match job.mode {
                SaveMode::SaveCopy => format!("Exporting edited copy… {export_pct}%"),
                SaveMode::ReplaceOriginal => format!("Exporting video… {export_pct}%"),
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
                    .child(pill("vid-cancel-export", "Cancel", false).on_click(cx.listener(
                        move |_, _, _, cx| {
                            cancel.store(true, Ordering::Relaxed);
                            cx.notify();
                        },
                    )))
                    .into_any_element(),
            );
        }

        if let Some(err) = &v.error {
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
                    .child(pill("vid-dismiss-err", "Dismiss", false).on_click(cx.listener(
                        |this, _, _, cx| {
                            if let Some(v) = this.video_edit.as_mut() {
                                v.error = None;
                            }
                            cx.notify();
                        },
                    )))
                    .into_any_element(),
            );
        }

        // Video stage canvas
        let bounds_cell = v.canvas_bounds.clone();
        let vbox = v.video_box();
        let frame_img = v.frame.clone();
        let active_crop = if cropping { v.crop_draft } else { v.edit.crop };

        let mut stage = div()
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

        if let Some(frame) = frame_img {
            if let Some(vbox) = vbox {
                let img_x = vbox.x - vbox.canvas_x;
                let img_y = vbox.y - vbox.canvas_y;
                stage = stage.child(
                    div()
                        .absolute()
                        .left(px(img_x))
                        .top(px(img_y))
                        .w(px(vbox.w))
                        .h(px(vbox.h))
                        .rounded(px(4.))
                        .overflow_hidden()
                        .child(img(frame).size_full().object_fit(ObjectFit::Fill)),
                );
                if let Some(crop) = active_crop {
                    let (cx0, cy0) = vbox.video_to_local(PointF::new(crop.x, crop.y));
                    let cw = (crop.w * vbox.scale).max(4.0);
                    let ch = (crop.h * vbox.scale).max(4.0);
                    let shade = rgba(0x0000008C);
                    stage = stage
                        .child(
                            div()
                                .absolute()
                                .left(px(img_x))
                                .top(px(img_y))
                                .w(px(vbox.w))
                                .h(px((cy0 - img_y).max(0.0)))
                                .bg(shade),
                        )
                        .child(
                            div()
                                .absolute()
                                .left(px(img_x))
                                .top(px(cy0 + ch))
                                .w(px(vbox.w))
                                .h(px(((img_y + vbox.h) - (cy0 + ch)).max(0.0)))
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
                                .w(px(((img_x + vbox.w) - (cx0 + cw)).max(0.0)))
                                .h(px(ch))
                                .bg(shade),
                        )
                        .child(
                            div()
                                .absolute()
                                .left(px(cx0))
                                .top(px(cy0))
                                .w(px(cw))
                                .h(px(ch))
                                .border_2()
                                .border_color(if cropping {
                                    rgb(0xFFFFFF)
                                } else {
                                    p.accent.into()
                                }),
                        );
                    if cropping {
                        for (idx, corner) in crop.corners().into_iter().enumerate() {
                            let (hx, hy) = vbox.video_to_local(corner);
                            stage = stage.child(
                                div()
                                    .id(("vid-crop-corner", idx))
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
                        for (idx, (edge, mid)) in crop.edge_midpoints().into_iter().enumerate() {
                            let (mx, my) = vbox.video_to_local(mid);
                            let horizontal = matches!(edge, CropEdge::Top | CropEdge::Bottom);
                            let (bw, bh) = if horizontal { (22.0, 6.0) } else { (6.0, 22.0) };
                            stage = stage.child(
                                div()
                                    .id(("vid-crop-edge", idx))
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
                }
            } else {
                stage = stage.child(
                    div()
                        .size_full()
                        .p_4()
                        .child(img(frame).size_full().object_fit(ObjectFit::Contain)),
                );
            }
        } else {
            stage = stage.child(
                div()
                    .size_full()
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_size(px(13.))
                    .text_color(p.muted)
                    .child(if v.loading {
                        "Loading video preview…"
                    } else {
                        "Video preview unavailable"
                    }),
            );
        }

        // Filmstrip timeline & playback dock
        let tl_bounds_cell = v.timeline_bounds.clone();
        let tl_bounds = v.timeline_bounds.get();
        let dur_safe = duration.max(0.001);
        let play_frac = (v.current_secs / dur_safe).clamp(0.0, 1.0) as f32;
        let trim_start_frac = (v.edit.trim_start / dur_safe).clamp(0.0, 1.0) as f32;
        let trim_end_frac = (v.edit.trim_end / dur_safe).clamp(trim_start_frac as f64, 1.0) as f32;

        let mut filmstrip_row = div()
            .absolute()
            .inset_0()
            .flex()
            .items_stretch()
            .overflow_hidden()
            .rounded(px(5.));
        for thumb in &v.filmstrip {
            filmstrip_row = filmstrip_row.child(
                div()
                    .flex_1()
                    .h_full()
                    .overflow_hidden()
                    .child(img(thumb.clone()).size_full().object_fit(ObjectFit::Cover)),
            );
        }

        let mut timeline = div()
            .id("vid-timeline")
            .relative()
            .flex_1()
            .h(px(38.))
            .rounded(px(6.))
            .bg(p.pane)
            .border_1()
            .border_color(p.line)
            .overflow_hidden()
            .cursor_pointer()
            .on_mouse_down(MouseButton::Left, cx.listener(Self::video_timeline_mouse_down))
            .on_mouse_move(cx.listener(Self::video_timeline_mouse_move))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _, _, _| {
                    if let Some(v) = this.video_edit.as_mut() {
                        v.timeline_drag = None;
                    }
                }),
            )
            .child(filmstrip_row)
            .child(
                canvas(
                    move |bounds, _, _| {
                        tl_bounds_cell.set(Some(bounds));
                    },
                    |_, _, _, _| {},
                )
                .absolute()
                .inset_0(),
            );

        if let Some(bounds) = tl_bounds {
            let tw = f32::from(bounds.size.width).max(1.0);
            if editing {
                let s_px = trim_start_frac * tw;
                let e_px = trim_end_frac * tw;
                timeline = timeline
                    .child(
                        div()
                            .absolute()
                            .left(px(0.))
                            .top(px(0.))
                            .w(px(s_px))
                            .h_full()
                            .bg(rgba(0x00000099)),
                    )
                    .child(
                        div()
                            .absolute()
                            .left(px(e_px))
                            .top(px(0.))
                            .w(px((tw - e_px).max(0.0)))
                            .h_full()
                            .bg(rgba(0x00000099)),
                    )
                    .child(
                        div()
                            .absolute()
                            .left(px(s_px))
                            .top(px(0.))
                            .w(px((e_px - s_px).max(4.0)))
                            .h_full()
                            .border_2()
                            .border_color(p.accent),
                    )
                    // Left & right trim handles
                    .child(
                        div()
                            .absolute()
                            .left(px((s_px - 4.0).max(0.0)))
                            .top(px(0.))
                            .w(px(8.0))
                            .h_full()
                            .rounded_l(px(3.))
                            .bg(p.accent),
                    )
                    .child(
                        div()
                            .absolute()
                            .left(px((e_px - 4.0).clamp(0.0, (tw - 8.0).max(0.0))))
                            .top(px(0.))
                            .w(px(8.0))
                            .h_full()
                            .rounded_r(px(3.))
                            .bg(p.accent),
                    );
            }
            // Playhead needle
            let play_x = (play_frac * tw).clamp(1.0, (tw - 2.0).max(1.0));
            timeline = timeline.child(
                div()
                    .absolute()
                    .left(px(play_x - 1.0))
                    .top(px(0.))
                    .w(px(2.0))
                    .h_full()
                    .bg(rgb(0xFFFFFF)),
            );
        }

        let meta_str = info
            .as_ref()
            .map(|i| {
                let (rw, rh) = i.rotated_dimensions(v.edit.quarter_turns);
                format!(
                    "{rw}×{rh} · {:.0} fps · {}{}",
                    i.fps,
                    i.codec,
                    if !i.has_audio {
                        " · No audio"
                    } else if muted {
                        " · Muted"
                    } else {
                        ""
                    }
                )
            })
            .unwrap_or_default();

        let dock = div()
            .flex_shrink_0()
            .px_4()
            .py_2()
            .flex()
            .flex_col()
            .gap_2()
            .border_t_1()
            .border_color(p.line)
            .bg(p.chrome)
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_3()
                    .child(
                        div()
                            .id("vid-play-pause")
                            .role(Role::Button)
                            .aria_label(if playing { "Pause" } else { "Play" })
                            .h(px(30.))
                            .px(px(12.))
                            .rounded(crate::tokens::RADIUS_CONTROL)
                            .flex()
                            .items_center()
                            .justify_center()
                            .cursor_pointer()
                            .bg(p.accent.opacity(0.18))
                            .border_1()
                            .border_color(p.accent.opacity(0.45))
                            .text_size(px(12.))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(p.text)
                            .hover(|s| s.bg(p.accent.opacity(0.28)))
                            .child(super::interface::button_text(if playing {
                                "Pause"
                            } else {
                                "Play"
                            }))
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.video_toggle_play(window, cx);
                            })),
                    )
                    .child(
                        div()
                            .font_family(crate::tokens::FONT_MONO)
                            .text_size(px(12.))
                            .text_color(p.text)
                            .child(format!(
                                "{} / {}",
                                format_timecode(v.current_secs),
                                format_timecode(duration)
                            )),
                    )
                    .child(timeline)
                    .when(!editing, |d| {
                        d.child(pill("vid-quick-frame", "Save frame", false).on_click(
                            cx.listener(|this, _, window, cx| {
                                this.video_save_frame(window, cx);
                            }),
                        ))
                        .child(
                            pill("vid-enter-edit", "Edit", false).on_click(cx.listener(
                                |this, _, _, cx| {
                                    if let Some(v) = this.video_edit.as_mut() {
                                        v.editing = true;
                                    }
                                    cx.notify();
                                },
                            )),
                        )
                    }),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .text_size(px(11.))
                    .text_color(p.muted)
                    .child(meta_str)
                    .child(if editing {
                        "Drag blue timeline handles to trim · Space plays · ←/→ steps 1s"
                    } else {
                        "Space plays/pauses · Click timeline to scrub · Edit to trim, crop, rotate or mute"
                    }),
            );

        div()
            .id("video-editor-root")
            .track_focus(&v.focus)
            .size_full()
            .flex()
            .flex_col()
            .overflow_hidden()
            .on_key_down(cx.listener(|this, event, window, cx| {
                let _ = this.video_handle_key(event, window, cx);
            }))
            .children(sub_bars)
            .child(
                div()
                    .id("video-editor-canvas")
                    .relative()
                    .flex_1()
                    .min_h_0()
                    .w_full()
                    .overflow_hidden()
                    .bg(p.pane)
                    .cursor(if cropping {
                        CursorStyle::Crosshair
                    } else {
                        CursorStyle::PointingHand
                    })
                    .on_mouse_down(MouseButton::Left, cx.listener(Self::video_canvas_mouse_down))
                    .on_mouse_move(cx.listener(Self::video_canvas_mouse_move))
                    .on_mouse_up(
                        MouseButton::Left,
                        cx.listener(|this, _, _, _| {
                            if let Some(v) = this.video_edit.as_mut() {
                                v.crop_drag = None;
                            }
                        }),
                    )
                    .child(stage),
            )
            .child(dock)
            .into_any_element()
    }
}
