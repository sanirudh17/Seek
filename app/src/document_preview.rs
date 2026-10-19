//! Selected-document previews only. A finite, killable host owns native/parser
//! resources; the app keeps at most one decoded page/table, never a document.
use crate::{backend::Row, media};
use gpui::RenderImage;
use serde::{Deserialize, Serialize};
use std::{
    io::{Read, Write},
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
const MAX_OUTPUT: u64 = 8 * 1024 * 1024;
const TIMEOUT: Duration = Duration::from_secs(4);
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Key {
    pub file: media::Key,
    pub page: u32,
}
impl Key {
    pub fn new(row: &Row, page: u32) -> Option<Self> {
        (row.file.kind & 3 == seek_engine::walk::KIND_FILE
            && matches!(extension(&row.file.path).as_str(), "pdf" | "docx" | "xlsx"))
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
    pub source: String,
    pub width: u32,
    pub height: u32,
    pub page: u32,
    pub pages: u32,
    pub text: Option<String>,
    pub sheet: Option<Sheet>,
}
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
    tx: async_channel::Sender<()>,
}
impl Worker {
    pub fn start() -> (Self, async_channel::Receiver<Reply>) {
        let desired = Arc::new(Mutex::new(None::<Key>));
        let epoch = Arc::new(AtomicU64::new(0));
        let work = desired.clone();
        let serial = epoch.clone();
        let (tx, rx) = async_channel::bounded(1);
        let (reply, updates) = async_channel::bounded(1);
        std::thread::Builder::new()
            .name("seek-document-preview".into())
            .spawn(move || {
                while rx.recv_blocking().is_ok() {
                    let stamp = serial.load(Ordering::Acquire);
                    let Some(key) = work.lock().unwrap().clone() else {
                        continue;
                    };
                    let started = Instant::now();
                    let result = load(&key, &|| serial.load(Ordering::Acquire) != stamp);
                    if serial.load(Ordering::Acquire) == stamp {
                        let _ = reply.force_send(Reply {
                            key,
                            epoch: stamp,
                            result,
                            elapsed_ms: started.elapsed().as_secs_f64() * 1000.,
                        });
                    }
                }
            })
            .expect("document preview worker");
        (Self { desired, epoch, tx }, updates)
    }
    pub fn request(&self, key: Option<Key>) {
        let mut wanted = self.desired.lock().unwrap();
        if *wanted == key {
            return;
        }
        self.epoch.fetch_add(1, Ordering::AcqRel);
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
    }
}
pub fn load(key: &Key, cancelled: &impl Fn() -> bool) -> Result<Preview, String> {
    if key.file.size > MAX_FILE {
        return Err("File exceeds the 32 MiB preview limit".into());
    }
    let bytes = host_output(key, cancelled)?;
    let split = bytes
        .iter()
        .position(|b| *b == b'\n')
        .ok_or("Invalid preview output")?;
    let info: Wire = serde_json::from_slice(&bytes[..split]).map_err(|e| e.to_string())?;
    let pixels = &bytes[split + 1..];
    let image = if info.width > 0 && info.height > 0 && info.width <= 1024 && info.height <= 1024 {
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
fn host_output(key: &Key, cancelled: &impl Fn() -> bool) -> Result<Vec<u8>, String> {
    let job = crate::child_job::Job::new().map_err(|e| e.to_string())?;
    let mut command = Command::new(std::env::current_exe().map_err(|e| e.to_string())?);
    command
        .args(["--document-preview-host", "--document-job"])
        .arg(&job.name)
        .arg("--document-file")
        .arg(&key.file.path)
        .arg("--document-page")
        .arg(key.page.to_string())
        .creation_flags(0x08000000)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    #[cfg(feature = "benchmarks")]
    if let Some(ms) = crate::argument("--document-host-test-stall-ms") {
        command.arg("--document-host-test-stall-ms").arg(ms);
    }
    let mut child = command.spawn().map_err(|e| e.to_string())?;
    let output = child.stdout.take().unwrap();
    let reader = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        output
            .take(MAX_OUTPUT + 1)
            .read_to_end(&mut bytes)
            .map(|_| bytes)
    });
    let deadline = Instant::now() + TIMEOUT;
    let result = loop {
        if cancelled() || Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            break Err(if cancelled() {
                "Cancelled"
            } else {
                "Preview timed out"
            }
            .to_owned());
        }
        match child.try_wait() {
            Ok(Some(status)) => {
                break if status.success() {
                    Ok(())
                } else {
                    Err("Unsupported, broken, encrypted or over-limit document".into())
                };
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(10)),
            Err(e) => {
                let _ = child.kill();
                let _ = child.wait();
                break Err(e.to_string());
            }
        }
    };
    let bytes = reader
        .join()
        .map_err(|_| "Preview reader failed")?
        .map_err(|e| e.to_string())?;
    result?;
    if bytes.len() as u64 > MAX_OUTPUT {
        return Err("Preview output exceeds limit".into());
    }
    Ok(bytes)
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
        let (info, pixels) = match extension(&path).as_str() {
            "pdf" => render_pdf(&bytes, page)?,
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
fn check_archive(bytes: &[u8]) -> anyhow::Result<()> {
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
fn render_pdf(bytes: &[u8], index: u32) -> anyhow::Result<(Wire, Vec<u8>)> {
    use windows::{Data::Pdf::*, Storage::Streams::*};
    let input = InMemoryRandomAccessStream::new()?;
    let writer = DataWriter::CreateDataWriter(&input.GetOutputStreamAt(0)?)?;
    writer.WriteBytes(bytes)?;
    writer.StoreAsync()?.join()?;
    writer.DetachStream()?;
    let doc = PdfDocument::LoadFromStreamAsync(&input)?.join()?;
    let pages = doc.PageCount()?;
    anyhow::ensure!(index < pages, "Page out of range");
    let page = doc.GetPage(index)?;
    let size = page.Size()?;
    let scale = media::PREVIEW_EDGE as f32 / size.Width.max(size.Height).max(1.);
    let options = PdfPageRenderOptions::new()?;
    options.SetDestinationWidth((size.Width * scale).round().max(1.) as u32)?;
    options.SetDestinationHeight((size.Height * scale).round().max(1.) as u32)?;
    let output = InMemoryRandomAccessStream::new()?;
    page.RenderWithOptionsToStreamAsync(&output, &options)?
        .join()?;
    let length = output.Size()?;
    anyhow::ensure!(length <= MAX_OUTPUT, "Rendered page exceeds limit");
    let reader = DataReader::CreateDataReader(&output.GetInputStreamAt(0)?)?;
    reader.LoadAsync(length as u32)?.join()?;
    let mut png = vec![0; length as usize];
    reader.ReadBytes(&mut png)?;
    // Windows may scale the requested raster by monitor DPI. Normalize the
    // display payload so a high-DPI monitor cannot exceed the parent's bounds.
    let image = image::load_from_memory(&png)?
        .thumbnail(media::PREVIEW_EDGE as u32, media::PREVIEW_EDGE as u32)
        .into_rgba8();
    let (width, height) = image.dimensions();
    anyhow::ensure!(width <= 1024 && height <= 1024, "Invalid render dimensions");
    let mut pixels = image.into_raw();
    for p in pixels.chunks_exact_mut(4) {
        p.swap(0, 2);
    }
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
