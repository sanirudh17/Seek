//! Selected-document previews only. A finite, killable host owns native/parser
//! resources; the UI displays one page/table and a bounded volatile frame cache
//! accelerates navigation. No native document is retained between host runs.
use crate::{backend::Row, media};
use gpui::RenderImage;
use serde::{Deserialize, Serialize};
use std::{
    io::{Read, Write},
    os::windows::io::AsRawHandle,
    os::windows::process::CommandExt,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};
pub const MAX_FILE: u64 = 32 * 1024 * 1024;
const MAX_OUTPUT: u64 = 16 * 1024 * 1024;
pub const ENLARGED_EDGE: u16 = 1536;
const TIMEOUT: Duration = Duration::from_secs(4);
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Key {
    pub file: media::Key,
    pub page: u32,
}
impl Key {
    pub fn new(row: &Row, page: u32) -> Option<Self> {
        (row.file.kind & 3 == seek_engine::walk::KIND_FILE
            && matches!(
                extension(&row.file.path).as_str(),
                "pdf" | "docx" | "xlsx" | "pptx" | "pptm" | "ppsx" | "ppt"
            ))
        .then(|| Self {
            file: media::Key::new(row, media::PREVIEW_EDGE, false),
            page,
        })
    }
}
fn extension(path: &Path) -> String {
    path.extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_ascii_lowercase()
}
#[derive(Clone, Default, Serialize, Deserialize)]
pub struct Sheet {
    pub name: String,
    pub rows: Vec<Vec<String>>,
    pub start_row: u32,
    pub start_col: u32,
    pub total_rows: usize,
    pub total_cols: usize,
}
#[derive(Clone, Default, Serialize, Deserialize)]
pub struct Wire {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    pub source: String,
    pub width: u32,
    pub height: u32,
    pub page: u32,
    pub pages: u32,
    pub text: Option<String>,
    pub sheet: Option<Sheet>,
}
#[derive(Clone)]
pub struct Preview {
    pub info: Wire,
    pub image: Option<Arc<RenderImage>>,
}
pub struct Reply {
    pub key: Key,
    pub epoch: u64,
    pub result: Result<Preview, String>,
    #[cfg_attr(not(feature = "benchmarks"), allow(dead_code))]
    pub elapsed_ms: f64,
}
pub struct Worker {
    desired: Arc<Mutex<Option<Key>>>,
    epoch: Arc<AtomicU64>,
    reset: Arc<AtomicU64>,
    tx: async_channel::Sender<()>,
}
impl Worker {
    pub fn start() -> (Self, async_channel::Receiver<Reply>) {
        let loader = Arc::new(Mutex::new(crate::pdf_session::Loader::default()));
        let clear = loader.clone();
        Self::start_impl(
            move |key, cancelled| loader.lock().unwrap().load(key, cancelled),
            move || clear.lock().unwrap().clear(),
        )
    }
    #[cfg(test)]
    fn start_with(
        loader: impl FnMut(&Key, &dyn Fn() -> bool) -> Result<Preview, String> + Send + 'static,
    ) -> (Self, async_channel::Receiver<Reply>) {
        Self::start_impl(loader, || {})
    }
    fn start_impl(
        mut loader: impl FnMut(&Key, &dyn Fn() -> bool) -> Result<Preview, String> + Send + 'static,
        mut clear_loader: impl FnMut() + Send + 'static,
    ) -> (Self, async_channel::Receiver<Reply>) {
        let desired = Arc::new(Mutex::new(None::<Key>));
        let epoch = Arc::new(AtomicU64::new(0));
        let reset = Arc::new(AtomicU64::new(0));
        let reset_work = reset.clone();
        let work = desired.clone();
        let serial = epoch.clone();
        let (tx, rx) = async_channel::bounded(1);
        let (reply, updates) = async_channel::bounded(1);
        std::thread::Builder::new()
            .name("seek-document-preview".into())
            .spawn(move || {
                let mut cache = crate::preview_cache::Cache::default();
                let mut published = None::<(u64, Key)>;
                let mut cache_generation = 0;
                while rx.recv_blocking().is_ok() {
                    let clear = reset_work.load(Ordering::Acquire);
                    if clear != cache_generation {
                        cache.clear();
                        clear_loader();
                        cache_generation = clear;
                    }
                    // Read the key and its generation as one snapshot under the
                    // same lock request() uses. Never process a new key with an
                    // old ticket during a rapid selection change.
                    let (stamp, key) = {
                        let wanted = work.lock().unwrap();
                        (serial.load(Ordering::Acquire), wanted.clone())
                    };
                    let Some(key) = key else {
                        clear_loader();
                        cache.clear();
                        published = None;
                        continue;
                    };
                    let already_published = published.as_ref() == Some(&(stamp, key.clone()));
                    let started = Instant::now();
                    let file_stamp = crate::preview_cache::Stamp::read(&key);
                    let result = match cache.get(&key) {
                        Some(value) => Ok(value),
                        None => loader(&key, &|| serial.load(Ordering::Acquire) != stamp),
                    };
                    if let Ok(value) = &result {
                        cache.put(key.clone(), file_stamp, value);
                    }
                    let pages = result.as_ref().ok().map_or(0, |p| p.info.pages);
                    if serial.load(Ordering::Acquire) == stamp && !already_published {
                        let _ = reply.force_send(Reply {
                            key: key.clone(),
                            epoch: stamp,
                            result,
                            elapsed_ms: started.elapsed().as_secs_f64() * 1000.,
                        });
                        published = Some((stamp, key.clone()));
                    }
                    // At most one adjacent page of the selected PDF. It is a
                    // finite, cancellable host, never a retained idle process.
                    if matches!(
                        extension(&key.file.path).as_str(),
                        "pdf" | "pptx" | "pptm" | "ppsx"
                    ) && key.page.saturating_add(1) < pages
                        && serial.load(Ordering::Acquire) == stamp
                    {
                        let mut next = key.clone();
                        next.page += 1;
                        if cache.get(&next).is_some() {
                            continue;
                        }
                        let file_stamp = crate::preview_cache::Stamp::read(&next);
                        let started = Instant::now();
                        let result = loader(&next, &|| {
                            if reset_work.load(Ordering::Acquire) != cache_generation {
                                return true;
                            }
                            let desired = work.lock().unwrap();
                            desired.as_ref() != Some(&key) && desired.as_ref() != Some(&next)
                        });
                        let (desired, current) = {
                            let desired = work.lock().unwrap();
                            (desired.clone(), serial.load(Ordering::Acquire))
                        };
                        if desired.as_ref() == Some(&key) || desired.as_ref() == Some(&next) {
                            if let Ok(value) = &result {
                                cache.put(next.clone(), file_stamp, value);
                            }
                            if desired.as_ref() == Some(&next) {
                                let _ = reply.force_send(Reply {
                                    key: next.clone(),
                                    epoch: current,
                                    result,
                                    elapsed_ms: started.elapsed().as_secs_f64() * 1000.,
                                });
                                published = Some((current, next));
                            }
                        }
                    }
                }
            })
            .expect("document preview worker");
        (
            Self {
                desired,
                epoch,
                reset,
                tx,
            },
            updates,
        )
    }
    pub fn request(&self, key: Option<Key>) {
        let mut wanted = self.desired.lock().unwrap();
        if *wanted == key {
            return;
        }
        self.epoch.fetch_add(1, Ordering::AcqRel);
        if key.is_none() {
            self.reset.fetch_add(1, Ordering::AcqRel);
        }
        *wanted = key;
        let _ = self.tx.force_send(());
    }
    pub fn accepts(&self, reply: &Reply) -> bool {
        self.epoch.load(Ordering::Acquire) == reply.epoch
            && self.desired.lock().unwrap().as_ref() == Some(&reply.key)
    }
}
impl Drop for Worker {
    fn drop(&mut self) {
        self.epoch.fetch_add(1, Ordering::AcqRel);
        *self.desired.lock().unwrap() = None;
    }
}
pub fn load(key: &Key, cancelled: &(impl Fn() -> bool + ?Sized)) -> Result<Preview, String> {
    if key.file.size > MAX_FILE {
        return Err("File exceeds the 32 MiB preview limit".into());
    }
    let bytes = host_output(key, cancelled)?;
    decode_wire(bytes)
}
pub(crate) fn decode_wire(bytes: Vec<u8>) -> Result<Preview, String> {
    let split = bytes
        .iter()
        .position(|b| *b == b'\n')
        .ok_or("Invalid preview output")?;
    let info: Wire = serde_json::from_slice(&bytes[..split]).map_err(|e| e.to_string())?;
    if let Some(error) = &info.error {
        return Err(error.clone());
    }
    let pixels = &bytes[split + 1..];
    let image = if info.width > 0 && info.height > 0 && info.width <= 1536 && info.height <= 1536 {
        if pixels.len() != info.width as usize * info.height as usize * 4 {
            return Err("Invalid preview pixels".into());
        }
        Some(Arc::new(RenderImage::new(vec![image::Frame::new(
            image::RgbaImage::from_raw(info.width, info.height, pixels.to_vec())
                .ok_or("Invalid preview size")?,
        )])))
    } else if info.text.is_some() || info.sheet.is_some() {
        None
    } else {
        return Err("Preview unavailable".into());
    };
    Ok(Preview { info, image })
}
fn host_output(key: &Key, cancelled: &(impl Fn() -> bool + ?Sized)) -> Result<Vec<u8>, String> {
    let job = crate::child_job::Job::new().map_err(|e| e.to_string())?;
    let executable = std::env::current_exe().map_err(|e| e.to_string())?;
    #[cfg(test)]
    let executable = std::env::var_os("SEEK_TEST_PREVIEW_HOST")
        .map(PathBuf::from)
        .unwrap_or(executable);
    let mut command = Command::new(executable);
    command
        .args(["--document-preview-host", "--document-job"])
        .arg(&job.name)
        .arg("--document-file")
        .arg(&key.file.path)
        .arg("--document-page")
        .arg(key.page.to_string())
        .arg("--document-edge")
        .arg(key.file.edge.to_string())
        .creation_flags(0x08000000)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    #[cfg(feature = "benchmarks")]
    if let Some(ms) = crate::argument("--document-host-test-stall-ms") {
        command.arg("--document-host-test-stall-ms").arg(ms);
    }
    let mut child = command.spawn().map_err(|e| e.to_string())?;
    let result = collect_output(&mut child, cancelled, TIMEOUT);
    if result.is_err() {
        let _ = child.kill();
        let _ = child.wait();
    }
    result
}
#[link(name = "kernel32")]
unsafe extern "system" {
    fn PeekNamedPipe(
        pipe: *mut std::ffi::c_void,
        buffer: *mut std::ffi::c_void,
        size: u32,
        read: *mut u32,
        available: *mut u32,
        left: *mut u32,
    ) -> i32;
}
fn collect_output(
    child: &mut std::process::Child,
    cancelled: &(impl Fn() -> bool + ?Sized),
    timeout: Duration,
) -> Result<Vec<u8>, String> {
    let mut output = child.stdout.take().ok_or("Missing preview pipe")?;
    let mut bytes = Vec::new();
    let mut buffer = [0u8; 16384];
    let deadline = Instant::now() + timeout;
    loop {
        if cancelled() || Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Err(if cancelled() {
                "Cancelled"
            } else {
                "Preview timed out"
            }
            .to_owned());
        }
        // A COM provider or another inherited handle can keep stdout open after
        // the host exits. read_to_end + join then ignored the four-second
        // timeout. Read only available bytes on this worker; never wait for EOF.
        let mut available = 0;
        let peek = unsafe {
            PeekNamedPipe(
                output.as_raw_handle(),
                std::ptr::null_mut(),
                0,
                std::ptr::null_mut(),
                &mut available,
                std::ptr::null_mut(),
            )
        };
        if peek != 0 && available > 0 {
            let count = (available as usize).min(buffer.len());
            let read = output
                .read(&mut buffer[..count])
                .map_err(|e| e.to_string())?;
            if bytes.len() + read > MAX_OUTPUT as usize {
                return Err("Preview output exceeds limit".into());
            }
            bytes.extend_from_slice(&buffer[..read]);
            continue;
        }
        match child.try_wait() {
            Ok(Some(status)) => {
                // Recheck after observing exit: the final write may have raced
                // the earlier peek. Once exited, a host cannot add more bytes.
                let mut tail = 0;
                if unsafe {
                    PeekNamedPipe(
                        output.as_raw_handle(),
                        std::ptr::null_mut(),
                        0,
                        std::ptr::null_mut(),
                        &mut tail,
                        std::ptr::null_mut(),
                    )
                } != 0
                    && tail > 0
                {
                    continue;
                }
                return if status.success() {
                    Ok(bytes)
                } else {
                    Err("Unsupported, broken, encrypted or over-limit document".into())
                };
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(10)),
            Err(e) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(e.to_string());
            }
        }
    }
}
/// Runs before GPUI/profile initialization. No Office COM object, preview
/// handler, persistent thumbnail file or document-specific watcher is created.
pub fn host() -> i32 {
    let result = (|| -> anyhow::Result<()> {
        crate::child_job::attach_current(
            &crate::argument("--document-job").ok_or_else(|| anyhow::anyhow!("Missing job"))?,
        )?;
        #[cfg(feature = "benchmarks")]
        if let Some(ms) = crate::argument("--document-host-test-stall-ms") {
            std::thread::sleep(Duration::from_millis(ms.parse::<u64>()?.min(10000)));
        }
        unsafe {
            windows::Win32::System::WinRT::RoInitialize(
                windows::Win32::System::WinRT::RO_INIT_MULTITHREADED,
            )?;
        }
        let path = PathBuf::from(
            crate::argument("--document-file").ok_or_else(|| anyhow::anyhow!("Missing file"))?,
        );
        let page = crate::argument("--document-page")
            .unwrap_or_default()
            .parse::<u32>()?;
        let edge = crate::argument("--document-edge")
            .and_then(|s| s.parse::<u16>().ok())
            .unwrap_or(media::PREVIEW_EDGE)
            .clamp(256, ENLARGED_EDGE);
        let meta = std::fs::symlink_metadata(&path)?;
        anyhow::ensure!(
            meta.is_file() && meta.len() <= MAX_FILE && !crate::content::unavailable(&path, &meta),
            "Unavailable or over-limit file"
        );
        let mut bytes = Vec::new();
        std::fs::File::open(&path)?
            .take(MAX_FILE + 1)
            .read_to_end(&mut bytes)?;
        anyhow::ensure!(bytes.len() as u64 <= MAX_FILE, "Input grew beyond cap");
        if extension(&path) == "pdf" && std::env::args().any(|a| a == "--document-session") {
            return crate::pdf_session::serve(&bytes);
        }
        let (info, pixels) = match extension(&path).as_str() {
            "pdf" => render_pdf(&bytes, page, edge)?,
            "pptx" | "pptm" | "ppsx" | "ppt" => {
                let legacy = extension(&path) == "ppt";
                let (pages, text) = if legacy {
                    (1, None)
                } else {
                    check_archive(&bytes)?;
                    let (pages, text) = crate::presentation::slide(&bytes, page)?;
                    (pages, Some(text))
                };
                let key = media::Key {
                    path: path.clone(),
                    mtime: 0,
                    size: meta.len(),
                    edge: edge.min(1024),
                    details: false,
                    folder: false,
                };
                // Windows exposes a first-slide thumbnail, not a page renderer.
                // Never label that first slide as a later slide's artwork.
                if page == 0
                    && !legacy
                    && let Some((width, height, pixels)) =
                        crate::presentation::thumbnail(&bytes, edge)
                {
                    (
                        Wire {
                            source: "PowerPoint saved first-slide preview".into(),
                            width,
                            height,
                            page,
                            pages,
                            text,
                            ..Default::default()
                        },
                        pixels,
                    )
                } else if page == 0
                    && let Some(image) = media::document_thumbnail(&key)
                        .ok()
                        .filter(|i| thumbnail_has_content(i))
                {
                    (
                        Wire {
                            source: "PowerPoint thumbnail".into(),
                            width: image.size(0).width.0 as u32,
                            height: image.size(0).height.0 as u32,
                            page,
                            pages,
                            text,
                            ..Default::default()
                        },
                        image
                            .as_bytes(0)
                            .ok_or_else(|| anyhow::anyhow!("No pixels"))?
                            .to_vec(),
                    )
                } else {
                    anyhow::ensure!(
                        !legacy,
                        "No PowerPoint thumbnail available for this legacy presentation"
                    );
                    (
                        Wire {
                            source: "PowerPoint slide text".into(),
                            page,
                            pages,
                            text,
                            ..Default::default()
                        },
                        vec![],
                    )
                }
            }
            "xlsx" => {
                check_archive(&bytes)?;
                (
                    Wire {
                        source: "sheet".into(),
                        sheet: Some(read_sheet(bytes)?),
                        ..Default::default()
                    },
                    vec![],
                )
            }
            "docx" => {
                check_archive(&bytes)?;
                // Validate the package before asking a provider for artwork.
                zip::ZipArchive::new(std::io::Cursor::new(&bytes))?.by_name("word/document.xml")?;
                let key = media::Key {
                    path,
                    mtime: 0,
                    size: meta.len(),
                    edge: media::PREVIEW_EDGE,
                    details: false,
                    folder: false,
                };
                // Some packages contain an empty/stale white template preview.
                // A blank cached bitmap is not a useful document thumbnail.
                if let Some(image) = media::document_thumbnail(&key)
                    .ok()
                    .filter(|i| thumbnail_has_content(i))
                {
                    (
                        Wire {
                            source: "Word thumbnail".into(),
                            width: image.size(0).width.0 as u32,
                            height: image.size(0).height.0 as u32,
                            ..Default::default()
                        },
                        image
                            .as_bytes(0)
                            .ok_or_else(|| anyhow::anyhow!("No pixels"))?
                            .to_vec(),
                    )
                } else {
                    let text =
                        crate::content::docx(&bytes, &|| false).map_err(anyhow::Error::msg)?;
                    (
                        Wire {
                            source: "Word text".into(),
                            text: Some(
                                text.lines()
                                    .take(100)
                                    .collect::<Vec<_>>()
                                    .join("\n")
                                    .chars()
                                    .take(12000)
                                    .collect(),
                            ),
                            ..Default::default()
                        },
                        vec![],
                    )
                }
            }
            _ => anyhow::bail!("Unsupported document"),
        };
        let mut out = std::io::stdout().lock();
        serde_json::to_writer(&mut out, &info)?;
        out.write_all(b"\n")?;
        out.write_all(&pixels)?;
        Ok(())
    })();
    if result.is_ok() { 0 } else { 1 }
}
pub(crate) fn check_archive(bytes: &[u8]) -> anyhow::Result<()> {
    let archive = zip::ZipArchive::new(std::io::Cursor::new(bytes))?;
    anyhow::ensure!(archive.len() <= 10000, "Too many package parts");
    let expanded = archive
        .decompressed_size()
        .ok_or_else(|| anyhow::anyhow!("Unknown archive size"))?;
    anyhow::ensure!(
        expanded <= 64 * 1024 * 1024,
        "Expanded package exceeds limit"
    );
    Ok(())
}
fn thumbnail_has_content(image: &RenderImage) -> bool {
    image.as_bytes(0).is_some_and(|bytes| {
        bytes
            .chunks_exact(4)
            .filter(|p| p[3] > 16 && p[..3].iter().any(|c| *c < 224))
            .take(24)
            .count()
            >= 24
    })
}
fn read_sheet(bytes: Vec<u8>) -> anyhow::Result<Sheet> {
    use calamine::Reader;
    let mut workbook = calamine::Xlsx::new(std::io::Cursor::new(bytes))?;
    let name = workbook
        .sheet_names()
        .first()
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("No sheet"))?;
    let range = workbook.worksheet_range(&name)?;
    let (total_rows, total_cols) = range.get_size();
    let (start_row, start_col) = range.start().unwrap_or((0, 0));
    let rows = range
        .rows()
        .take(100)
        .map(|r| {
            r.iter()
                .take(12)
                .map(|cell| cell.to_string().chars().take(256).collect())
                .collect()
        })
        .collect();
    Ok(Sheet {
        name,
        rows,
        start_row,
        start_col,
        total_rows,
        total_cols,
    })
}
fn render_pdf(bytes: &[u8], index: u32, edge: u16) -> anyhow::Result<(Wire, Vec<u8>)> {
    use windows::{Data::Pdf::*, Storage::Streams::*};
    let input = InMemoryRandomAccessStream::new()?;
    let writer = DataWriter::CreateDataWriter(&input.GetOutputStreamAt(0)?)?;
    writer.WriteBytes(bytes)?;
    writer.StoreAsync()?.join()?;
    writer.DetachStream()?;
    let doc = PdfDocument::LoadFromStreamAsync(&input)?.join()?;
    render_loaded_pdf(&doc, index, edge)
}
pub(crate) fn render_loaded_pdf(
    doc: &windows::Data::Pdf::PdfDocument,
    index: u32,
    edge: u16,
) -> anyhow::Result<(Wire, Vec<u8>)> {
    use windows::{Data::Pdf::*, Storage::Streams::*};
    let pages = doc.PageCount()?;
    anyhow::ensure!(index < pages, "Page out of range");
    let page = doc.GetPage(index)?;
    let size = page.Size()?;
    let scale = edge as f32 / size.Width.max(size.Height).max(1.);
    let options = PdfPageRenderOptions::new()?;
    options.SetDestinationWidth((size.Width * scale).round().max(1.) as u32)?;
    options.SetDestinationHeight((size.Height * scale).round().max(1.) as u32)?;
    let output = InMemoryRandomAccessStream::new()?;
    page.RenderWithOptionsToStreamAsync(&output, &options)?
        .join()?;
    anyhow::ensure!(output.Size()? <= MAX_OUTPUT, "Rendered page exceeds limit");
    // Keep decoding/scaling in Windows' optimized native imaging path. The
    // default PNG codec plus Rust debug decode/resampling dominated large-page
    // turns; the final buffer is already GPUI's required premultiplied BGRA.
    use windows::Graphics::Imaging::*;
    let decoder = BitmapDecoder::CreateAsync(&output)?.join()?;
    let native_width = decoder.PixelWidth()?;
    let native_height = decoder.PixelHeight()?;
    anyhow::ensure!(
        native_width > 0 && native_height > 0 && native_width <= 4096 && native_height <= 4096,
        "Invalid render dimensions"
    );
    let ratio = (edge as f64 / native_width.max(native_height) as f64).min(1.);
    let width = (native_width as f64 * ratio).round().max(1.) as u32;
    let height = (native_height as f64 * ratio).round().max(1.) as u32;
    let transform = BitmapTransform::new()?;
    transform.SetScaledWidth(width)?;
    transform.SetScaledHeight(height)?;
    let pixels = decoder
        .GetPixelDataTransformedAsync(
            BitmapPixelFormat::Bgra8,
            BitmapAlphaMode::Premultiplied,
            &transform,
            ExifOrientationMode::IgnoreExifOrientation,
            ColorManagementMode::DoNotColorManage,
        )?
        .join()?
        .DetachPixelData()?
        .to_vec();
    anyhow::ensure!(
        width <= 1536 && height <= 1536 && pixels.len() == width as usize * height as usize * 4,
        "Invalid preview pixels"
    );
    page.Close()?;
    Ok((
        Wire {
            source: "PDF".into(),
            width,
            height,
            page: index,
            pages,
            ..Default::default()
        },
        pixels,
    ))
}
#[cfg(test)]
mod tests {
    use super::*;
    fn wait_reply(worker: &Worker, rx: &async_channel::Receiver<Reply>) -> Reply {
        let deadline = Instant::now() + Duration::from_secs(8);
        loop {
            if let Ok(reply) = rx.try_recv()
                && worker.accepts(&reply)
            {
                return reply;
            }
            assert!(Instant::now() < deadline, "Preview reply missing");
            std::thread::sleep(Duration::from_millis(1));
        }
    }
    #[test]
    fn adjacent_pages_reuse_work_and_closing_clears_cached_previews() {
        let temp = tempfile::tempdir().unwrap();
        let file = temp.path().join("owned.pdf");
        std::fs::write(&file, "owned").unwrap();
        let calls = Arc::new(Mutex::new(Vec::new()));
        let observed = calls.clone();
        let (done, finished) = std::sync::mpsc::channel();
        let (worker, rx) = Worker::start_with(move |key, cancelled| {
            observed.lock().unwrap().push(key.page);
            for _ in 0..10 {
                if cancelled() {
                    return Err("Cancelled".into());
                }
                std::thread::sleep(Duration::from_millis(1));
            }
            let _ = done.send(key.page);
            Ok(Preview {
                info: Wire {
                    source: "PDF".into(),
                    page: key.page,
                    pages: 3,
                    text: Some(format!("page {}", key.page)),
                    ..Default::default()
                },
                image: None,
            })
        });
        let key = |page| Key {
            file: media::Key {
                path: file.clone(),
                mtime: 0,
                size: 5,
                edge: 768,
                details: false,
                folder: false,
            },
            page,
        };
        worker.request(Some(key(0)));
        assert!(wait_reply(&worker, &rx).result.is_ok());
        while finished.recv_timeout(Duration::from_secs(2)).unwrap() != 1 {}
        worker.request(Some(key(1)));
        assert_eq!(wait_reply(&worker, &rx).result.unwrap().info.page, 1);
        assert_eq!(
            calls.lock().unwrap().iter().filter(|&&p| p == 1).count(),
            1,
            "Next page must reuse prefetch instead of opening another host"
        );
        worker.request(None);
        worker.request(Some(key(1)));
        assert!(wait_reply(&worker, &rx).result.is_ok());
        assert_eq!(
            calls.lock().unwrap().iter().filter(|&&p| p == 1).count(),
            2,
            "Even a quick panel close/reopen must discard old cached previews"
        );
        worker.request(None);
    }
    #[test]
    #[ignore = "Requires explicit read-only PDF/Word paths and an existing preview-host executable"]
    fn selected_documents_cache_latency() {
        let pdf = PathBuf::from(std::env::var_os("SEEK_TEST_PREVIEW_PDF").unwrap());
        let word = PathBuf::from(std::env::var_os("SEEK_TEST_PREVIEW_WORD").unwrap());
        let make_key = |path: &Path, page| Key {
            file: media::Key {
                path: path.into(),
                mtime: 0,
                size: std::fs::metadata(path).unwrap().len(),
                edge: std::env::var("SEEK_TEST_PREVIEW_EDGE")
                    .ok()
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(768),
                details: false,
                folder: false,
            },
            page,
        };
        let (done, finished) = std::sync::mpsc::channel();
        let native = Arc::new(Mutex::new(crate::pdf_session::Loader::default()));
        let clear = native.clone();
        let (worker, rx) = Worker::start_impl(
            move |key, cancelled| {
                let result = native.lock().unwrap().load(key, cancelled);
                let _ = done.send((key.file.path.clone(), key.page, result.is_ok()));
                result
            },
            move || clear.lock().unwrap().clear(),
        );
        let started = Instant::now();
        worker.request(Some(make_key(&pdf, 0)));
        let first = wait_reply(&worker, &rx);
        assert!(first.result.is_ok());
        let first_ms = started.elapsed().as_secs_f64() * 1000.;
        let started = Instant::now();
        worker.request(Some(make_key(&pdf, 1)));
        assert!(wait_reply(&worker, &rx).result.is_ok());
        let immediate_next_ms = started.elapsed().as_secs_f64() * 1000.;
        while {
            let (path, page, ok) = finished.recv_timeout(Duration::from_secs(6)).unwrap();
            assert!(ok);
            path != pdf || page != 2
        } {}
        let started = Instant::now();
        worker.request(Some(make_key(&pdf, 2)));
        assert!(wait_reply(&worker, &rx).result.is_ok());
        let next_ms = started.elapsed().as_secs_f64() * 1000.;
        let started = Instant::now();
        worker.request(Some(make_key(&word, 0)));
        assert!(wait_reply(&worker, &rx).result.is_ok());
        let word_ms = started.elapsed().as_secs_f64() * 1000.;
        worker.request(Some(make_key(&pdf, 0)));
        assert!(wait_reply(&worker, &rx).result.is_ok());
        let started = Instant::now();
        worker.request(Some(make_key(&word, 0)));
        assert!(wait_reply(&worker, &rx).result.is_ok());
        let word_hit_ms = started.elapsed().as_secs_f64() * 1000.;
        eprintln!(
            "Selected-document worker measurements (debug host): PDF initial {first_ms:.3} ms, immediate next {immediate_next_ms:.3} ms, next after prefetch {next_ms:.3} ms; Word initial {word_ms:.3} ms, revisit {word_hit_ms:.3} ms. Excludes UI paint."
        );
        worker.request(None);
    }
    fn fixture_pdf() -> Vec<u8> {
        let stream = b"BT /F1 18 Tf 40 100 Td (Owned preview fixture) Tj ET";
        let content = format!(
            "<< /Length {} >>\nstream\n{}\nendstream",
            stream.len(),
            String::from_utf8_lossy(stream)
        );
        let objects=[
            "<< /Type /Catalog /Pages 2 0 R >>".to_owned(),
            "<< /Type /Pages /Kids [3 0 R 4 0 R] /Count 2 >>".to_owned(),
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 200] /Resources << /Font << /F1 5 0 R >> >> /Contents 6 0 R >>".to_owned(),
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 200] /Resources << /Font << /F1 5 0 R >> >> /Contents 6 0 R >>".to_owned(),
            "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_owned(),content
        ];
        let mut bytes = b"%PDF-1.4\n".to_vec();
        let mut offsets = Vec::new();
        for (i, obj) in objects.iter().enumerate() {
            offsets.push(bytes.len());
            bytes.extend_from_slice(format!("{} 0 obj\n{obj}\nendobj\n", i + 1).as_bytes());
        }
        let xref = bytes.len();
        bytes.extend_from_slice(b"xref\n0 7\n0000000000 65535 f \n");
        for offset in offsets {
            bytes.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
        }
        bytes.extend_from_slice(
            format!("trailer\n<< /Size 7 /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n").as_bytes(),
        );
        bytes
    }
    #[test]
    fn inherited_stdout_cannot_keep_a_completed_preview_loading() {
        let role = std::env::var("SEEK_PREVIEW_PIPE_TEST").unwrap_or_default();
        if role == "stall" {
            crate::child_job::attach_current(&std::env::var("SEEK_PREVIEW_PIPE_JOB").unwrap())
                .unwrap();
            std::thread::sleep(Duration::from_secs(30));
            return;
        }
        if role == "descendant" {
            std::thread::sleep(Duration::from_secs(30));
            return;
        }
        if role == "host" {
            crate::child_job::attach_current(&std::env::var("SEEK_PREVIEW_PIPE_JOB").unwrap())
                .unwrap();
            let _child=Command::new(std::env::current_exe().unwrap()).args(["--exact","document_preview::tests::inherited_stdout_cannot_keep_a_completed_preview_loading","--nocapture"]).env("SEEK_PREVIEW_PIPE_TEST","descendant").stdout(Stdio::inherit()).creation_flags(0x08000000).spawn().unwrap();
            println!("owned-preview-complete");
            std::process::exit(0);
        }
        let job = crate::child_job::Job::new().unwrap();
        let mut child = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "document_preview::tests::inherited_stdout_cannot_keep_a_completed_preview_loading",
                "--nocapture",
            ])
            .env("SEEK_PREVIEW_PIPE_TEST", "host")
            .env("SEEK_PREVIEW_PIPE_JOB", &job.name)
            .stdout(Stdio::piped())
            .creation_flags(0x08000000)
            .spawn()
            .unwrap();
        let started = Instant::now();
        let bytes = collect_output(&mut child, &|| false, Duration::from_secs(3)).unwrap();
        assert!(String::from_utf8_lossy(&bytes).contains("owned-preview-complete"));
        assert!(started.elapsed() < Duration::from_secs(3));
        eprintln!(
            "Inherited stdout regression: host result returned in {:.3} ms while its descendant kept stdout open",
            started.elapsed().as_secs_f64() * 1000.
        );
        let mut stalled = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "document_preview::tests::inherited_stdout_cannot_keep_a_completed_preview_loading",
                "--nocapture",
            ])
            .env("SEEK_PREVIEW_PIPE_TEST", "stall")
            .env("SEEK_PREVIEW_PIPE_JOB", &job.name)
            .stdout(Stdio::piped())
            .creation_flags(0x08000000)
            .spawn()
            .unwrap();
        let start = Instant::now();
        assert_eq!(
            collect_output(&mut stalled, &|| false, Duration::from_millis(150)).unwrap_err(),
            "Preview timed out"
        );
        assert!(start.elapsed() < Duration::from_secs(2));
        drop(job); // terminate only the owned descendants
    }
    #[test]
    fn rapid_selection_cancels_old_ticket_and_keeps_latest_reply() {
        let (worker, rx) = Worker::start_with(|key, cancelled| {
            for _ in 0..10 {
                if cancelled() {
                    return Err("Cancelled".into());
                }
                std::thread::sleep(Duration::from_millis(1));
            }
            Err(key.file.path.to_string_lossy().into_owned())
        });
        let key = |i| Key {
            file: media::Key {
                path: format!("owned-{i}.pdf").into(),
                mtime: 0,
                size: 1,
                edge: 768,
                details: false,
                folder: false,
            },
            page: 0,
        };
        for i in 0..100 {
            worker.request(Some(key(i)));
        }
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            if let Ok(reply) = rx.try_recv()
                && worker.accepts(&reply)
            {
                assert_eq!(reply.key, key(99));
                assert_eq!(reply.result.err().as_deref(), Some("owned-99.pdf"));
                break;
            }
            assert!(Instant::now() < deadline, "Latest preview never arrived");
            std::thread::sleep(Duration::from_millis(2));
        }
        worker.request(None);
    }
    #[test]
    fn native_pdf_pages_render_at_panel_and_enlarged_resolution() {
        use windows::Win32::System::WinRT::*;
        unsafe {
            RoInitialize(RO_INIT_MULTITHREADED).unwrap();
        }
        struct Apartment;
        impl Drop for Apartment {
            fn drop(&mut self) {
                unsafe { RoUninitialize() };
            }
        }
        let _apartment = Apartment;
        let bytes = std::env::var_os("SEEK_TEST_PREVIEW_PDF")
            .map(|path| std::fs::read(path).expect("Read selected PDF"))
            .unwrap_or_else(fixture_pdf);
        let started = Instant::now();
        for edge in [media::PREVIEW_EDGE, ENLARGED_EDGE] {
            let (first, pixels) = render_pdf(&bytes, 0, edge).unwrap();
            assert!(first.pages > 0);
            assert_eq!(first.width.max(first.height), u32::from(edge));
            assert_eq!(pixels.len(), (first.width * first.height * 4) as usize);
            if first.pages > 1 {
                assert_eq!(render_pdf(&bytes, 1, edge).unwrap().0.page, 1);
            }
            assert!(render_pdf(&bytes, first.pages, edge).is_err());
        }
        assert!(render_pdf(b"broken", 0, 768).is_err());
        eprintln!(
            "Native PDF panel/enlarged + page/broken checks: {:.3} ms (test harness)",
            started.elapsed().as_secs_f64() * 1000.
        );
    }
}
