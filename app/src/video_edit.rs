//! Inline video preview, playback, and editing engine powered by `ffmpeg` / `ffprobe`.
//!
//! Architecture inspired by Monodrive's `edits/video.rs` (MIT) and Glint's FFmpeg pipeline:
//! - Probe metadata (duration, resolution, FPS, audio presence, rotation) via `ffprobe`
//! - Instant frame extraction & filmstrip thumbnails via fast `-ss` seeks
//! - Real-time piped raw BGRA playback stream (zero PNG decode overhead, directly into GPUI)
//! - Non-destructive edit recipe: Trim, Crop (even-pixel snapped), Rotate 90°, Mute, Save frame
//! - Fast stream-copy export when only trimming/muting; clean H.264/AAC re-encode for crop/rotate
//! - Live progress reporting, instant cancellation, `Save a Copy` default, atomic `Replace Original`
//! - Always spawns `ffmpeg`/`ffprobe` with `CREATE_NO_WINDOW` on Windows (never flashes a console)

use crate::picture_edit::{RectF, SaveMode, to_render_image};
use gpui::RenderImage;
use image::RgbaImage;
use std::{
    fs::OpenOptions,
    io::{BufRead, BufReader, Read},
    path::{Path, PathBuf},
    process::{Child, ChildStdout, Command, Stdio},
    sync::{
        Arc, OnceLock,
        atomic::{AtomicBool, AtomicU8, Ordering},
    },
};

#[cfg(windows)]
use std::os::windows::process::CommandExt;

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Supported video extensions for inline preview & editing.
pub fn is_video(path: &Path) -> bool {
    let Some(ext) = path.extension().and_then(|e| e.to_str()) else {
        return false;
    };
    matches!(
        ext.to_ascii_lowercase().as_str(),
        "mp4" | "mov" | "m4v" | "mkv" | "webm" | "avi" | "wmv"
    )
}

fn hidden_command(program: &Path) -> Command {
    let mut cmd = Command::new(program);
    #[cfg(windows)]
    cmd.creation_flags(CREATE_NO_WINDOW);
    cmd
}

fn locate_binary(name: &str) -> Option<PathBuf> {
    if let Ok(exe) = std::env::current_exe()
        && let Some(dir) = exe.parent()
    {
        let sibling = dir.join(name);
        if sibling.is_file() {
            return Some(sibling);
        }
    }
    if let Ok(path_var) = std::env::var("PATH") {
        for dir in std::env::split_paths(&path_var) {
            let candidate = dir.join(name);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

pub fn ffmpeg_path() -> Option<PathBuf> {
    static PATH: OnceLock<Option<PathBuf>> = OnceLock::new();
    PATH.get_or_init(|| locate_binary("ffmpeg.exe").or_else(|| locate_binary("ffmpeg")))
        .clone()
}

pub fn ffprobe_path() -> Option<PathBuf> {
    static PATH: OnceLock<Option<PathBuf>> = OnceLock::new();
    PATH.get_or_init(|| locate_binary("ffprobe.exe").or_else(|| locate_binary("ffprobe")))
        .clone()
}

#[derive(Clone, Debug, PartialEq)]
pub struct VideoInfo {
    pub duration: f64,
    pub width: u32,
    pub height: u32,
    pub fps: f64,
    pub has_audio: bool,
    pub codec: String,
}

impl VideoInfo {
    /// Display dimensions after applying `quarter_turns` clockwise 90° rotations.
    pub fn rotated_dimensions(&self, quarter_turns: u8) -> (u32, u32) {
        if quarter_turns % 2 == 1 {
            (self.height.max(1), self.width.max(1))
        } else {
            (self.width.max(1), self.height.max(1))
        }
    }
}

/// Non-destructive video editing recipe.
#[derive(Clone, Debug, PartialEq)]
pub struct VideoEdit {
    pub trim_start: f64,
    pub trim_end: f64,
    pub crop: Option<RectF>,
    /// Clockwise 90-degree turns (0..=3).
    pub quarter_turns: u8,
    pub muted: bool,
}

impl VideoEdit {
    pub fn identity(duration: f64) -> Self {
        Self {
            trim_start: 0.0,
            trim_end: duration.max(0.0),
            crop: None,
            quarter_turns: 0,
            muted: false,
        }
    }

    pub fn is_dirty(&self, duration: f64) -> bool {
        self.trim_start > 0.02
            || (duration > 0.05 && (duration - self.trim_end).abs() > 0.02)
            || self.crop.is_some()
            || (self.quarter_turns % 4) != 0
            || self.muted
    }

    pub fn trimmed_duration(&self) -> f64 {
        (self.trim_end - self.trim_start).max(0.0)
    }

    pub fn needs_reencode(&self) -> bool {
        self.crop.is_some() || (self.quarter_turns % 4) != 0
    }
}

pub struct VideoExportRequest {
    pub source: PathBuf,
    pub info: VideoInfo,
    pub edit: VideoEdit,
    pub mode: SaveMode,
}

/// Format seconds as `M:SS` (or `H:MM:SS` if >= 1 hour).
pub fn format_timecode(secs: f64) -> String {
    let total = secs.max(0.0).floor() as u64;
    let h = total / 3600;
    let m = (total % 3600) / 60;
    let s = total % 60;
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    }
}

/// Format seconds with tenths for trim markers (`M:SS.t`).
pub fn format_precise_time(secs: f64) -> String {
    let clamped = secs.max(0.0);
    let tenths = ((clamped * 10.0).round() as u64) % 10;
    let total = ((clamped * 10.0).round() as u64) / 10;
    let m = total / 60;
    let s = total % 60;
    format!("{m}:{s:02}.{tenths}")
}

fn parse_fps(raw: &str) -> f64 {
    if let Some((num, den)) = raw.split_once('/') {
        let n: f64 = num.parse().unwrap_or(0.0);
        let d: f64 = den.parse().unwrap_or(1.0);
        if d > 0.0 && n > 0.0 {
            return (n / d).clamp(1.0, 240.0);
        }
    }
    raw.parse::<f64>().unwrap_or(30.0).clamp(1.0, 240.0)
}

/// Probe video duration, resolution, frame rate, and audio stream via `ffprobe`.
pub fn probe(path: &Path) -> Result<VideoInfo, String> {
    let ffprobe = ffprobe_path()
        .ok_or_else(|| "FFmpeg (ffprobe) was not found on PATH. Install FFmpeg to preview and edit videos.".to_string())?;
    let out = hidden_command(&ffprobe)
        .args([
            "-v",
            "quiet",
            "-print_format",
            "json",
            "-show_format",
            "-show_streams",
        ])
        .arg(path)
        .output()
        .map_err(|e| format!("Could not run ffprobe: {e}"))?;
    if !out.status.success() {
        return Err("Could not read video metadata.".into());
    }
    let json: serde_json::Value = serde_json::from_slice(&out.stdout)
        .map_err(|e| format!("Invalid ffprobe output: {e}"))?;
    let streams = json
        .get("streams")
        .and_then(|s| s.as_array())
        .ok_or_else(|| "No streams found in video.".to_string())?;

    let video_stream = streams
        .iter()
        .find(|s| s.get("codec_type").and_then(|v| v.as_str()) == Some("video"))
        .ok_or_else(|| "No video stream found in file.".to_string())?;
    let has_audio = streams
        .iter()
        .any(|s| s.get("codec_type").and_then(|v| v.as_str()) == Some("audio"));

    let mut width = video_stream
        .get("width")
        .and_then(|v| v.as_u64())
        .unwrap_or(0) as u32;
    let mut height = video_stream
        .get("height")
        .and_then(|v| v.as_u64())
        .unwrap_or(0) as u32;

    // Account for display matrix rotation (e.g. vertical phone clips).
    let side_rotation = video_stream
        .get("side_data_list")
        .and_then(|v| v.as_array())
        .and_then(|arr| {
            arr.iter()
                .find_map(|d| d.get("rotation").and_then(|r| r.as_i64()))
        })
        .or_else(|| {
            video_stream
                .get("tags")
                .and_then(|t| t.get("rotate"))
                .and_then(|r| r.as_str())
                .and_then(|s| s.parse::<i64>().ok())
        })
        .unwrap_or(0);
    if (side_rotation.abs() % 180) == 90 {
        std::mem::swap(&mut width, &mut height);
    }

    if width == 0 || height == 0 {
        return Err("Video has invalid dimensions.".into());
    }

    let duration = json
        .get("format")
        .and_then(|f| f.get("duration"))
        .and_then(|v| v.as_str())
        .and_then(|s| s.parse::<f64>().ok())
        .or_else(|| {
            video_stream
                .get("duration")
                .and_then(|v| v.as_str())
                .and_then(|s| s.parse::<f64>().ok())
        })
        .unwrap_or(0.0)
        .max(0.0);

    let fps = video_stream
        .get("avg_frame_rate")
        .and_then(|v| v.as_str())
        .map(parse_fps)
        .or_else(|| {
            video_stream
                .get("r_frame_rate")
                .and_then(|v| v.as_str())
                .map(parse_fps)
        })
        .unwrap_or(30.0);

    let codec = video_stream
        .get("codec_name")
        .and_then(|v| v.as_str())
        .unwrap_or("video")
        .to_uppercase();

    Ok(VideoInfo {
        duration,
        width,
        height,
        fps,
        has_audio,
        codec,
    })
}

fn rotate_filter(quarter_turns: u8) -> Option<&'static str> {
    match quarter_turns % 4 {
        1 => Some("transpose=1"),
        2 => Some("transpose=1,transpose=1"),
        3 => Some("transpose=2"),
        _ => None,
    }
}

/// Extract a single video frame at `secs` as an `RgbaImage`, optionally rotated
/// and scaled to fit within `max_edge`.
pub fn extract_frame(
    path: &Path,
    secs: f64,
    max_edge: u32,
    quarter_turns: u8,
) -> Result<RgbaImage, String> {
    let ffmpeg = ffmpeg_path()
        .ok_or_else(|| "FFmpeg was not found on PATH.".to_string())?;
    let mut vf_parts: Vec<String> = Vec::new();
    if let Some(rot) = rotate_filter(quarter_turns) {
        vf_parts.push(rot.to_string());
    }
    if max_edge > 0 {
        vf_parts.push(format!(
            "scale=w='min({max_edge},iw)':h='min({max_edge},ih)':force_original_aspect_ratio=decrease"
        ));
    }

    let mut cmd = hidden_command(&ffmpeg);
    cmd.args(["-v", "error", "-ss", &format!("{:.3}", secs.max(0.0)), "-i"])
        .arg(path)
        .args(["-frames:v", "1"]);
    if !vf_parts.is_empty() {
        cmd.args(["-vf", &vf_parts.join(",")]);
    }
    cmd.args(["-f", "image2pipe", "-vcodec", "png", "-"]);

    let out = cmd
        .output()
        .map_err(|e| format!("Could not run ffmpeg: {e}"))?;
    if !out.status.success() || out.stdout.is_empty() {
        return Err("Could not decode video frame.".into());
    }
    let dyn_img = image::load_from_memory(&out.stdout)
        .map_err(|e| format!("Could not parse extracted frame: {e}"))?;
    Ok(dyn_img.to_rgba8())
}

/// Extract `count` evenly spaced small thumbnails for the timeline filmstrip.
pub fn extract_filmstrip(
    path: &Path,
    duration: f64,
    count: usize,
    quarter_turns: u8,
) -> Vec<Arc<RenderImage>> {
    let n = count.clamp(1, 12);
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let frac = (i as f64 + 0.5) / n as f64;
        let t = (duration * frac).clamp(0.0, (duration - 0.05).max(0.0));
        if let Ok(img) = extract_frame(path, t, 180, quarter_turns) {
            out.push(to_render_image(&img));
        }
    }
    out
}

/// Streaming real-time BGRA playback pipe backed by a single `ffmpeg` child.
pub struct PlaybackStream {
    child: Child,
    stdout: ChildStdout,
    pub width: u32,
    pub height: u32,
    pub fps: f64,
    frame_bytes: usize,
}

impl PlaybackStream {
    pub fn spawn(
        path: &Path,
        start_secs: f64,
        end_secs: f64,
        info: &VideoInfo,
        quarter_turns: u8,
        max_edge: u32,
    ) -> Result<Self, String> {
        let ffmpeg = ffmpeg_path().ok_or_else(|| "FFmpeg not found.".to_string())?;
        let (rot_w, rot_h) = info.rotated_dimensions(quarter_turns);
        let scale = (max_edge as f64 / rot_w.max(rot_h).max(1) as f64).min(1.0);
        // Even dimensions >= 2 for clean scaling.
        let out_w = (((rot_w as f64 * scale).round() as u32) & !1).max(2);
        let out_h = (((rot_h as f64 * scale).round() as u32) & !1).max(2);
        let fps = info.fps.clamp(12.0, 24.0);
        let span = (end_secs - start_secs).max(0.1);

        let mut vf_parts: Vec<String> = Vec::new();
        if let Some(rot) = rotate_filter(quarter_turns) {
            vf_parts.push(rot.to_string());
        }
        vf_parts.push(format!("fps={fps:.3}"));
        vf_parts.push(format!("scale={out_w}:{out_h}"));

        let mut cmd = hidden_command(&ffmpeg);
        cmd.args([
            "-v",
            "error",
            "-ss",
            &format!("{:.3}", start_secs.max(0.0)),
            "-t",
            &format!("{:.3}", span),
            "-i",
        ])
        .arg(path)
        .args([
            "-an",
            "-sn",
            "-vf",
            &vf_parts.join(","),
            "-f",
            "rawvideo",
            "-pix_fmt",
            "bgra",
            "-",
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());

        let mut child = cmd
            .spawn()
            .map_err(|e| format!("Could not start video playback: {e}"))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| "Could not capture playback stream.".to_string())?;
        Ok(Self {
            child,
            stdout,
            width: out_w,
            height: out_h,
            fps,
            frame_bytes: (out_w as usize) * (out_h as usize) * 4,
        })
    }

    /// Read the next BGRA frame directly into a GPUI `RenderImage` (zero swizzle!).
    pub fn next_frame(&mut self) -> Option<Arc<RenderImage>> {
        let mut buf = vec![0u8; self.frame_bytes];
        self.stdout.read_exact(&mut buf).ok()?;
        let img = RgbaImage::from_raw(self.width, self.height, buf)?;
        Some(Arc::new(RenderImage::new(vec![image::Frame::new(img)])))
    }
}

impl Drop for PlaybackStream {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Snap a crop rectangle to even pixel boundaries inside `(rot_w, rot_h)` so
/// H.264 `yuv420p` encoders always succeed cleanly.
pub fn snap_crop_even(crop: RectF, rot_w: u32, rot_h: u32) -> (u32, u32, u32, u32) {
    let max_w = (rot_w & !1).max(2);
    let max_h = (rot_h & !1).max(2);
    let x = ((crop.x.max(0.0).round() as u32) & !1).min(max_w.saturating_sub(2));
    let y = ((crop.y.max(0.0).round() as u32) & !1).min(max_h.saturating_sub(2));
    let w = ((crop.w.max(2.0).round() as u32) & !1).clamp(2, (max_w - x).max(2));
    let h = ((crop.h.max(2.0).round() as u32) & !1).clamp(2, (max_h - y).max(2));
    (x, y, w, h)
}

/// Build the FFmpeg command-line argument list for a `VideoEdit` recipe.
/// Pure function so every flag can be exhaustively unit-tested!
pub fn build_export_args(
    source: &Path,
    dest: &Path,
    info: &VideoInfo,
    edit: &VideoEdit,
) -> Vec<String> {
    let mut args = vec!["-y".to_string(), "-v".to_string(), "error".to_string()];

    let trimmed_start = edit.trim_start.max(0.0);
    let trimmed_end = edit.trim_end.min(info.duration.max(0.0)).max(trimmed_start);
    let has_trim_start = trimmed_start > 0.02;
    let has_trim_end = info.duration > 0.05 && (info.duration - trimmed_end).abs() > 0.02;

    if has_trim_start {
        args.push("-ss".into());
        args.push(format!("{trimmed_start:.3}"));
    }
    args.push("-i".into());
    args.push(source.to_string_lossy().into_owned());

    if has_trim_start || has_trim_end {
        let span = (trimmed_end - trimmed_start).max(0.05);
        args.push("-t".into());
        args.push(format!("{span:.3}"));
    }

    if edit.needs_reencode() {
        let mut filters: Vec<String> = Vec::new();
        if let Some(rot) = rotate_filter(edit.quarter_turns) {
            filters.push(rot.into());
        }
        if let Some(crop) = edit.crop {
            let (rw, rh) = info.rotated_dimensions(edit.quarter_turns);
            let (x, y, w, h) = snap_crop_even(crop, rw, rh);
            filters.push(format!("crop={w}:{h}:{x}:{y}"));
        }
        if !filters.is_empty() {
            args.push("-vf".into());
            args.push(filters.join(","));
        }
        args.extend([
            "-c:v".into(),
            "libx264".into(),
            "-preset".into(),
            "fast".into(),
            "-crf".into(),
            "18".into(),
            "-pix_fmt".into(),
            "yuv420p".into(),
        ]);
        if edit.muted || !info.has_audio {
            args.push("-an".into());
        } else {
            args.extend(["-c:a".into(), "aac".into(), "-b:a".into(), "192k".into()]);
        }
    } else {
        // Fast lossless stream copy when only trimming and/or muting!
        args.extend(["-c:v".into(), "copy".into()]);
        if edit.muted || !info.has_audio {
            args.push("-an".into());
        } else {
            args.extend(["-c:a".into(), "copy".into()]);
        }
    }

    // Preserve faststart for MP4/MOV playback
    let ext = dest
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    if matches!(ext.as_str(), "mp4" | "mov" | "m4v") {
        args.extend(["-movflags".into(), "+faststart".into()]);
    }

    args.extend(["-progress".into(), "pipe:1".into(), "-nostats".into()]);
    args.push(dest.to_string_lossy().into_owned());
    args
}

/// Reserve a non-colliding sibling output path (e.g. `clip edited.mp4` or `clip frame.png`).
fn next_sibling_path(source: &Path, suffix: &str, ext: &str) -> Result<PathBuf, String> {
    let parent = source
        .parent()
        .ok_or_else(|| "File has no parent folder.".to_string())?;
    let stem = source
        .file_stem()
        .and_then(|s| s.to_str())
        .filter(|s| !s.is_empty())
        .unwrap_or("video");
    for n in 1..=9999u32 {
        let name = if n == 1 {
            format!("{stem} {suffix}.{ext}")
        } else {
            format!("{stem} {suffix} {n}.{ext}")
        };
        let candidate = parent.join(name);
        if OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&candidate)
            .is_ok()
        {
            return Ok(candidate);
        }
    }
    Err("Could not find a free filename for the copy.".into())
}

/// Save the full-resolution frame at `secs` (with optional rotation/crop) as a
/// sibling PNG file (`<stem> frame.png`), never overwriting existing files.
pub fn save_frame_png(
    source: &Path,
    secs: f64,
    info: &VideoInfo,
    quarter_turns: u8,
    crop: Option<RectF>,
) -> Result<PathBuf, String> {
    let ffmpeg = ffmpeg_path().ok_or_else(|| "FFmpeg not found on PATH.".to_string())?;
    let dest = next_sibling_path(source, "frame", "png")?;
    let mut filters: Vec<String> = Vec::new();
    if let Some(rot) = rotate_filter(quarter_turns) {
        filters.push(rot.into());
    }
    if let Some(crop) = crop {
        let (rw, rh) = info.rotated_dimensions(quarter_turns);
        let (x, y, w, h) = snap_crop_even(crop, rw, rh);
        filters.push(format!("crop={w}:{h}:{x}:{y}"));
    }

    let mut cmd = hidden_command(&ffmpeg);
    cmd.args(["-y", "-v", "error", "-ss", &format!("{:.3}", secs.max(0.0)), "-i"])
        .arg(source)
        .args(["-frames:v", "1"]);
    if !filters.is_empty() {
        cmd.args(["-vf", &filters.join(",")]);
    }
    cmd.arg(&dest);

    let out = cmd
        .output()
        .map_err(|e| format!("Could not save video frame: {e}"))?;
    if !out.status.success() {
        let _ = std::fs::remove_file(&dest);
        return Err("FFmpeg failed to save the video frame.".into());
    }
    Ok(dest)
}

#[cfg(windows)]
fn atomic_replace(target: &Path, staged: &Path) -> Result<(), String> {
    use std::os::windows::ffi::OsStrExt;
    use windows::Win32::Storage::FileSystem::{REPLACEFILE_IGNORE_MERGE_ERRORS, ReplaceFileW};
    use windows::core::PCWSTR;
    let wide = |p: &Path| -> Vec<u16> {
        p.as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect()
    };
    let target_w = wide(target);
    let staged_w = wide(staged);
    let ok = unsafe {
        ReplaceFileW(
            PCWSTR(target_w.as_ptr()),
            PCWSTR(staged_w.as_ptr()),
            PCWSTR::null(),
            REPLACEFILE_IGNORE_MERGE_ERRORS,
            None,
            None,
        )
    };
    ok.map_err(|e| format!("Could not replace original video: {e}"))
}

#[cfg(not(windows))]
fn atomic_replace(target: &Path, staged: &Path) -> Result<(), String> {
    std::fs::rename(staged, target).map_err(|e| format!("Could not replace original video: {e}"))
}

/// Export the edited video with live `0..=100` progress and instant cancellation.
pub fn export_video(
    req: &VideoExportRequest,
    cancel: &AtomicBool,
    progress: &AtomicU8,
) -> Result<PathBuf, String> {
    let ffmpeg = ffmpeg_path().ok_or_else(|| "FFmpeg was not found on PATH.".to_string())?;
    let ext = req
        .source
        .extension()
        .and_then(|e| e.to_str())
        .filter(|s| !s.is_empty())
        .unwrap_or("mp4");
    let parent = req
        .source
        .parent()
        .ok_or_else(|| "Video has no parent folder.".to_string())?;

    let (staged_path, final_path) = match req.mode {
        SaveMode::SaveCopy => {
            let copy = next_sibling_path(&req.source, "edited", ext)?;
            (copy.clone(), copy)
        }
        SaveMode::ReplaceOriginal => {
            let pid = std::process::id();
            let stamp = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0);
            let tmp = parent.join(format!(".seek-video-{pid}-{stamp}.{ext}"));
            (tmp, req.source.clone())
        }
    };

    let args = build_export_args(&req.source, &staged_path, &req.info, &req.edit);
    let mut child = hidden_command(&ffmpeg)
        .args(&args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| {
            let _ = std::fs::remove_file(&staged_path);
            format!("Could not start FFmpeg export: {e}")
        })?;

    let expected_us = (req.edit.trimmed_duration().max(0.1) * 1_000_000.0) as u64;
    if let Some(stdout) = child.stdout.take() {
        let reader = BufReader::new(stdout);
        for line in reader.lines().map_while(Result::ok) {
            if cancel.load(Ordering::Relaxed) {
                let _ = child.kill();
                let _ = child.wait();
                let _ = std::fs::remove_file(&staged_path);
                return Err("Export cancelled.".into());
            }
            if let Some(val) = line.strip_prefix("out_time_us=")
                && let Ok(us) = val.trim().parse::<u64>()
            {
                let pct = ((us.saturating_mul(99)) / expected_us.max(1)).min(99) as u8;
                progress.store(pct, Ordering::Relaxed);
            }
        }
    }

    if cancel.load(Ordering::Relaxed) {
        let _ = child.kill();
        let _ = child.wait();
        let _ = std::fs::remove_file(&staged_path);
        return Err("Export cancelled.".into());
    }

    let status = child.wait().map_err(|e| {
        let _ = std::fs::remove_file(&staged_path);
        format!("Export process error: {e}")
    })?;
    if !status.success() {
        let _ = std::fs::remove_file(&staged_path);
        return Err("FFmpeg export failed. Try Save a Copy or check the video format.".into());
    }

    if req.mode == SaveMode::ReplaceOriginal {
        if let Err(err) = atomic_replace(&final_path, &staged_path) {
            let _ = std::fs::remove_file(&staged_path);
            return Err(err);
        }
    }
    progress.store(100, Ordering::Relaxed);
    Ok(final_path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_export_args_uses_stream_copy_for_trim_and_mute_and_reencodes_for_crop_rotate() {
        let info = VideoInfo {
            duration: 10.0,
            width: 1920,
            height: 1080,
            fps: 30.0,
            has_audio: true,
            codec: "H264".into(),
        };
        let src = Path::new(r"C:\Videos\clip.mp4");
        let dst = Path::new(r"C:\Videos\clip edited.mp4");

        // 1. Trim + Mute -> fast stream copy, -an
        let trim_mute = VideoEdit {
            trim_start: 1.5,
            trim_end: 6.5,
            crop: None,
            quarter_turns: 0,
            muted: true,
        };
        let args = build_export_args(src, dst, &info, &trim_mute);
        assert!(args.windows(2).any(|w| w == ["-c:v", "copy"]));
        assert!(args.iter().any(|a| a == "-an"));
        assert!(args.windows(2).any(|w| w == ["-ss", "1.500"]));
        assert!(args.windows(2).any(|w| w == ["-t", "5.000"]));

        // 2. Rotate 90° + Crop (odd coords snapped to even) -> libx264 + transpose + crop
        let rot_crop = VideoEdit {
            trim_start: 0.0,
            trim_end: 10.0,
            crop: Some(RectF {
                x: 11.0,
                y: 21.0,
                w: 501.0,
                h: 703.0,
            }),
            quarter_turns: 1,
            muted: false,
        };
        let args2 = build_export_args(src, dst, &info, &rot_crop);
        assert!(args2.windows(2).any(|w| w == ["-c:v", "libx264"]));
        assert!(args2.windows(2).any(|w| w == ["-c:a", "aac"]));
        let vf = args2
            .windows(2)
            .find(|w| w[0] == "-vf")
            .map(|w| w[1].as_str())
            .unwrap();
        assert_eq!(vf, "transpose=1,crop=500:702:10:20");
    }

    #[test]
    fn live_ffmpeg_probe_trim_mute_and_save_frame() {
        let Some(ffmpeg) = ffmpeg_path() else {
            return;
        };
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("synth.mp4");
        // Generate a 2-second test clip with audio tone
        let status = hidden_command(&ffmpeg)
            .args([
                "-y",
                "-v",
                "error",
                "-f",
                "lavfi",
                "-i",
                "testsrc=duration=2:size=320x240:rate=24",
                "-f",
                "lavfi",
                "-i",
                "sine=frequency=440:duration=2",
                "-c:v",
                "libx264",
                "-pix_fmt",
                "yuv420p",
                "-c:a",
                "aac",
            ])
            .arg(&src)
            .status()
            .unwrap();
        assert!(status.success(), "ffmpeg synthetic clip creation failed");

        let info = probe(&src).unwrap();
        assert_eq!(info.width, 320);
        assert_eq!(info.height, 240);
        assert!(info.has_audio);
        assert!((info.duration - 2.0).abs() < 0.3);

        // Extract frame
        let frame = extract_frame(&src, 0.5, 320, 0).unwrap();
        assert_eq!(frame.width(), 320);
        assert_eq!(frame.height(), 240);

        // Save frame PNG
        let png = save_frame_png(&src, 0.5, &info, 0, None).unwrap();
        assert!(png.is_file());
        assert!(png.file_name().unwrap().to_string_lossy().contains("frame.png"));

        // Export trimmed & muted copy
        let req = VideoExportRequest {
            source: src.clone(),
            info: info.clone(),
            edit: VideoEdit {
                trim_start: 0.4,
                trim_end: 1.4,
                crop: None,
                quarter_turns: 0,
                muted: true,
            },
            mode: SaveMode::SaveCopy,
        };
        let cancel = AtomicBool::new(false);
        let prog = AtomicU8::new(0);
        let exported = export_video(&req, &cancel, &prog).unwrap();
        assert!(exported.is_file());
        let out_info = probe(&exported).unwrap();
        assert!(!out_info.has_audio);
        assert!(out_info.duration > 0.5 && out_info.duration < 1.5);
    }
}
