//! Bounded, event-driven shell work. No file reads or COM calls on the UI thread.
use crate::{
    backend::Row,
    icons::{self, Kind},
};
use gpui::RenderImage;
use std::{
    collections::{HashMap, HashSet},
    io::Read,
    path::PathBuf,
    sync::{Arc, Mutex},
};
// Covers the largest preview at 200% DPI without enlarging a small list icon.
pub const PREVIEW_EDGE: u16 = 768;
#[derive(Clone, Debug, Hash, PartialEq, Eq)]
pub struct Key {
    pub path: PathBuf,
    pub mtime: u32,
    pub size: u64,
    pub edge: u16,
    pub details: bool,
    pub folder: bool,
}
impl Key {
    pub fn new(row: &Row, edge: u16, details: bool) -> Self {
        Self {
            path: row.file.path.clone(),
            mtime: row.file.mtime,
            size: row.file.size,
            edge,
            details,
            folder: row.file.kind & 3 == seek_engine::walk::KIND_DIR,
        }
    }
}
#[derive(Clone, Default)]
pub struct Details {
    pub created: String,
    pub modified: String,
    pub opened: String,
    pub text: Option<String>,
    pub error: Option<String>,
}
#[derive(Clone)]
pub enum Value {
    Image(Option<Arc<RenderImage>>),
    Details(Details),
}
impl Value {
    fn bytes(&self) -> usize {
        match self {
            Self::Image(Some(i)) => i.as_bytes(0).map_or(0, |b| b.len()),
            Self::Details(d) => d.text.as_ref().map_or(0, |t| t.len()) + 1024,
            _ => 64,
        }
    }
}
pub struct Reply {
    pub key: Key,
    pub value: Value,
    pub epoch: u64,
}
#[derive(Clone, Debug, PartialEq, Eq)]
struct Fingerprint {
    file: Option<(std::time::SystemTime, u64)>,
    opened: Option<std::time::SystemTime>,
}
impl Fingerprint {
    fn read(key: &Key, data: &std::path::Path) -> Self {
        let file = std::fs::metadata(&key.path)
            .ok()
            .and_then(|m| Some((m.modified().ok()?, m.len())));
        let opened = key
            .details
            .then(|| {
                std::fs::metadata(data.join("opened-dates.json"))
                    .ok()?
                    .modified()
                    .ok()
            })
            .flatten();
        Self { file, opened }
    }
}
#[derive(Clone)]
pub struct Worker {
    desired: Arc<Mutex<HashMap<Key, u64>>>,
    tx: async_channel::Sender<()>,
    epoch: Arc<std::sync::atomic::AtomicU64>,
    revalidate: Arc<std::sync::atomic::AtomicBool>,
}
impl Worker {
    pub fn start(data: PathBuf) -> (Self, async_channel::Receiver<Reply>) {
        let desired = Arc::new(Mutex::new(HashMap::<Key, u64>::new()));
        let work = desired.clone();
        let epoch = Arc::new(std::sync::atomic::AtomicU64::new(0));
        let revalidate = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let validation = revalidate.clone();
        let (tx, rx) = async_channel::bounded(1);
        let (result, updates) = async_channel::bounded(16);
        std::thread::Builder::new()
            .name("seek-previews".into())
            .spawn(move || {
                use windows::Win32::System::Com::*;
                let initialized = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) }.is_ok();
                unsafe {
                    windows_sys::Win32::System::Threading::SetThreadPriority(
                        windows_sys::Win32::System::Threading::GetCurrentThread(),
                        windows_sys::Win32::System::Threading::THREAD_PRIORITY_BELOW_NORMAL,
                    );
                }
                let mut completed = HashMap::<Key, (u64, Fingerprint)>::new();
                while rx.recv_blocking().is_ok() {
                    let check_changes = validation.swap(false, std::sync::atomic::Ordering::AcqRel);
                    let current = work.lock().unwrap().clone();
                    completed.retain(|k, (stamp, _)| current.get(k) == Some(stamp));
                    let mut pending: Vec<_> = current.into_iter().collect();
                    pending.sort_by_key(|(k, _)| {
                        (std::cmp::Reverse(k.details), std::cmp::Reverse(k.edge))
                    });
                    for (key, stamp) in pending {
                        if work.lock().unwrap().get(&key) != Some(&stamp) {
                            continue;
                        }
                        if completed.contains_key(&key) && !check_changes {
                            continue;
                        }
                        let fingerprint = Fingerprint::read(&key, &data);
                        if completed
                            .get(&key)
                            .is_some_and(|(_, old)| *old == fingerprint)
                        {
                            continue;
                        }
                        let value = if key.details {
                            Value::Details(details(&key, &data))
                        } else {
                            Value::Image(if initialized {
                                svg_preview(&key).or_else(|| thumbnail(&key).ok())
                            } else {
                                None
                            })
                        };
                        // Shared keys keep their stamp while other rows load or appear.
                        // Removed/re-added keys get a new stamp and reject stale replies.
                        if work.lock().unwrap().get(&key) == Some(&stamp) {
                            completed.insert(key.clone(), (stamp, fingerprint));
                            if result
                                .send_blocking(Reply {
                                    key,
                                    value,
                                    epoch: stamp,
                                })
                                .is_err()
                            {
                                break;
                            }
                        }
                    }
                }
                if initialized {
                    unsafe {
                        CoUninitialize();
                    }
                }
            })
            .expect("preview worker");
        (
            Self {
                desired,
                tx,
                epoch,
                revalidate,
            },
            updates,
        )
    }
    pub fn request(&self, keys: HashSet<Key>) {
        let mut current = self.desired.lock().unwrap();
        if current.len() == keys.len() && keys.iter().all(|key| current.contains_key(key)) {
            return;
        }
        let stamp = self.epoch.fetch_add(1, std::sync::atomic::Ordering::AcqRel) + 1;
        current.retain(|key, _| keys.contains(key));
        for key in keys {
            current.entry(key).or_insert(stamp);
        }
        let _ = self.tx.force_send(());
    }
    pub fn force(&self) {
        self.revalidate
            .store(true, std::sync::atomic::Ordering::Release);
        let _ = self.tx.force_send(());
    }
    pub fn accepts(&self, key: &Key, epoch: u64) -> bool {
        self.desired.lock().unwrap().get(key) == Some(&epoch)
    }
}

pub struct Cache {
    items: HashMap<Key, (Value, u64)>,
    clock: u64,
    bytes: usize,
    capacity: usize,
    budget: usize,
}
impl Default for Cache {
    fn default() -> Self {
        Self::new(96, 32 * 1024 * 1024)
    }
}
impl Cache {
    #[cfg(feature = "benchmarks")]
    pub fn stats(&self) -> (usize, usize, usize, usize) {
        (
            self.items.len(),
            self.bytes,
            self.items.keys().filter(|k| k.edge >= PREVIEW_EDGE).count(),
            self.items.keys().filter(|k| k.details).count(),
        )
    }
    /// Large previews and extracted details belong only to the active selection.
    /// Small list/tile images retain the existing bounded LRU behavior.
    pub fn retain_previews(&mut self, wanted: &HashSet<Key>) -> Vec<Arc<RenderImage>> {
        let retired: Vec<_> = self
            .items
            .keys()
            .filter(|k| (k.details || k.edge >= PREVIEW_EDGE) && !wanted.contains(*k))
            .cloned()
            .collect();
        let mut images = Vec::new();
        for key in retired {
            if let Some((value, _)) = self.items.remove(&key) {
                self.bytes -= value.bytes();
                if let Value::Image(Some(image)) = value {
                    images.push(image);
                }
            }
        }
        images
    }
    #[cfg(test)]
    pub fn clear(&mut self) -> Vec<Arc<RenderImage>> {
        self.bytes = 0;
        self.items
            .drain()
            .filter_map(|(_, (value, _))| match value {
                Value::Image(Some(i)) => Some(i),
                _ => None,
            })
            .collect()
    }
    pub fn new(capacity: usize, budget: usize) -> Self {
        Self {
            items: HashMap::new(),
            clock: 0,
            bytes: 0,
            capacity,
            budget,
        }
    }
    pub fn get(&mut self, key: &Key) -> Option<Value> {
        let (v, t) = self.items.get_mut(key)?;
        self.clock += 1;
        *t = self.clock;
        Some(v.clone())
    }
    #[cfg(test)]
    pub fn contains(&self, key: &Key) -> bool {
        self.items.contains_key(key)
    }
    pub fn insert(&mut self, key: Key, value: Value) -> Vec<Arc<RenderImage>> {
        self.clock += 1;
        self.bytes += value.bytes();
        let mut removed = vec![];
        if let Some((old, _)) = self.items.insert(key, (value, self.clock)) {
            self.bytes -= old.bytes();
            if let Value::Image(Some(i)) = old {
                removed.push(i)
            }
        }
        while self.items.len() > self.capacity || self.bytes > self.budget {
            let key = self
                .items
                .iter()
                .min_by_key(|(_, (_, t))| t)
                .unwrap()
                .0
                .clone();
            let (old, _) = self.items.remove(&key).unwrap();
            self.bytes -= old.bytes();
            if let Value::Image(Some(i)) = old {
                removed.push(i)
            }
        }
        removed
    }
}
fn date(value: std::io::Result<std::time::SystemTime>) -> String {
    value
        .ok()
        .map(chrono::DateTime::<chrono::Local>::from)
        .map(|d| d.format("%d %b %Y, %H:%M").to_string())
        .unwrap_or_else(|| "Unavailable".into())
}
fn details(key: &Key, data: &std::path::Path) -> Details {
    let meta = match std::fs::metadata(&key.path) {
        Ok(m) => m,
        Err(e) => {
            return Details {
                error: Some(format!("Cannot read this item: {e}")),
                ..Default::default()
            };
        }
    };
    let opened = std::fs::read(data.join("opened-dates.json"))
        .ok()
        .and_then(|bytes| serde_json::from_slice::<HashMap<PathBuf, u64>>(&bytes).ok())
        .and_then(|map| map.get(&key.path).copied())
        .and_then(|seconds| chrono::DateTime::from_timestamp(seconds as i64, 0))
        .map(|d| {
            d.with_timezone(&chrono::Local)
                .format("%d %b %Y, %H:%M")
                .to_string()
        })
        .unwrap_or_else(|| "Not opened in Seek".into());
    let text = if !key.folder && matches!(icons::kind(&key.path, false), Kind::Text | Kind::Code) {
        read_text(&key.path).ok()
    } else {
        None
    };
    Details {
        created: date(meta.created()),
        modified: date(meta.modified()),
        opened,
        text,
        error: None,
    }
}
pub fn record_open(data: &std::path::Path, path: &std::path::Path) {
    let mut dates = std::fs::read(data.join("opened-dates.json"))
        .ok()
        .and_then(|bytes| serde_json::from_slice::<HashMap<PathBuf, u64>>(&bytes).ok())
        .unwrap_or_default();
    dates.insert(
        path.to_path_buf(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs(),
    );
    while dates.len() > 4096 {
        let key = dates
            .iter()
            .min_by_key(|(_, date)| *date)
            .unwrap()
            .0
            .clone();
        dates.remove(&key);
    }
    if let Ok(value) = serde_json::to_value(dates) {
        let _ = crate::settings::atomic_json(data, "opened-dates.json", &value);
    }
}
pub fn read_text(path: &std::path::Path) -> std::io::Result<String> {
    let mut bytes = vec![];
    std::fs::File::open(path)?
        .take(8192)
        .read_to_end(&mut bytes)?;
    let text = if bytes.starts_with(&[0xff, 0xfe]) || bytes.starts_with(&[0xfe, 0xff]) {
        let le = bytes[0] == 0xff;
        String::from_utf16_lossy(
            &bytes[2..]
                .chunks_exact(2)
                .map(|b| {
                    if le {
                        u16::from_le_bytes([b[0], b[1]])
                    } else {
                        u16::from_be_bytes([b[0], b[1]])
                    }
                })
                .collect::<Vec<_>>(),
        )
    } else {
        String::from_utf8_lossy(bytes.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(&bytes))
            .into_owned()
    };
    Ok(text.lines().take(80).collect::<Vec<_>>().join("\n"))
}
pub(crate) fn thumbnail(key: &Key) -> windows::core::Result<Arc<RenderImage>> {
    use std::os::windows::ffi::OsStrExt;
    use windows::{
        Win32::{Foundation::SIZE, UI::Shell::*},
        core::PCWSTR,
    };
    let path = key
        .path
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect::<Vec<_>>();
    // Ask the actual provider for undecorated pixels. ImageFactory may return
    // an old, small cached image with Windows' photo border/drop shadow.
    // Providers also handle installed codecs and photo orientation for us.
    if key.edge >= 128
        && matches!(
            icons::kind(&key.path, key.folder),
            Kind::Image | Kind::Video
        )
    {
        let item: IShellItem = unsafe { SHCreateItemFromParsingName(PCWSTR(path.as_ptr()), None) }?;
        if let Ok(provider) =
            unsafe { item.BindToHandler::<_, IThumbnailProvider>(None, &BHID_ThumbnailHandler) }
        {
            let mut bitmap = Default::default();
            let mut alpha = WTSAT_UNKNOWN;
            if unsafe { provider.GetThumbnail(key.edge as u32, &mut bitmap, &mut alpha) }.is_ok() {
                return bitmap_image(
                    bitmap,
                    match alpha {
                        WTSAT_RGB => Some(true),
                        WTSAT_ARGB => Some(false),
                        _ => None,
                    },
                );
            }
        }
    }
    let factory: IShellItemImageFactory =
        unsafe { SHCreateItemFromParsingName(PCWSTR(path.as_ptr()), None) }?;
    let kind = icons::kind(&key.path, key.folder);
    let flags = if matches!(kind, Kind::Image | Kind::Video) {
        SIIGBF_THUMBNAILONLY
    } else {
        SIIGBF_ICONONLY
    };
    let bitmap = unsafe {
        factory.GetImage(
            SIZE {
                cx: key.edge as i32,
                cy: key.edge as i32,
            },
            flags | SIIGBF_BIGGERSIZEOK,
        )
    }?;
    bitmap_image(bitmap, None)
}

/// A real large document thumbnail, never an association icon fallback.
pub(crate) fn document_thumbnail(key: &Key) -> windows::core::Result<Arc<RenderImage>> {
    use std::os::windows::ffi::OsStrExt;
    use windows::{
        Win32::{Foundation::SIZE, UI::Shell::*},
        core::PCWSTR,
    };
    let path: Vec<_> = key.path.as_os_str().encode_wide().chain(Some(0)).collect();
    let factory: IShellItemImageFactory =
        unsafe { SHCreateItemFromParsingName(PCWSTR(path.as_ptr()), None) }?;
    let bitmap = unsafe {
        factory.GetImage(
            SIZE {
                cx: key.edge as i32,
                cy: key.edge as i32,
            },
            SIIGBF_THUMBNAILONLY | SIIGBF_BIGGERSIZEOK,
        )
    }?;
    bitmap_image(bitmap, None)
}

/// An association icon without opening a file. Called only on the startup worker.
pub(crate) fn association_icon(extension: &str) -> windows::core::Result<Arc<RenderImage>> {
    use windows::{
        Win32::{
            Graphics::Gdi::*,
            UI::{Shell::*, WindowsAndMessaging::*},
        },
        core::PCWSTR,
    };
    let directory = extension.is_empty();
    let path: Vec<u16> = format!("seek.{extension}\0").encode_utf16().collect();
    let mut info = SHFILEINFOW::default();
    let ok = unsafe {
        SHGetFileInfoW(
            PCWSTR(path.as_ptr()),
            if directory {
                windows::Win32::Storage::FileSystem::FILE_ATTRIBUTE_DIRECTORY
            } else {
                windows::Win32::Storage::FileSystem::FILE_ATTRIBUTE_NORMAL
            },
            Some(&mut info),
            std::mem::size_of::<SHFILEINFOW>() as u32,
            SHGFI_ICON | SHGFI_LARGEICON | SHGFI_USEFILEATTRIBUTES,
        )
    };
    if ok == 0 {
        return Err(windows::core::Error::from_thread());
    }
    let dc = unsafe { CreateCompatibleDC(None) };
    if dc.is_invalid() {
        unsafe {
            let _ = DestroyIcon(info.hIcon);
        }
        return Err(windows::core::Error::from_thread());
    }
    let bitmap_info = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: 32,
            biHeight: -32,
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut bits = std::ptr::null_mut();
    let bitmap =
        unsafe { CreateDIBSection(Some(dc), &bitmap_info, DIB_RGB_COLORS, &mut bits, None, 0) };
    let result = match bitmap {
        Ok(bitmap) => {
            // DIB pixels start transparent, then Windows paints its premultiplied icon.
            unsafe {
                std::ptr::write_bytes(bits, 0, 32 * 32 * 4);
            }
            let old = unsafe { SelectObject(dc, bitmap.into()) };
            let drawn = unsafe { DrawIconEx(dc, 0, 0, info.hIcon, 32, 32, 0, None, DI_NORMAL) };
            unsafe {
                SelectObject(dc, old);
            }
            if drawn.is_ok() {
                bitmap_image(bitmap, None)
            } else {
                unsafe {
                    let _ = DeleteObject(bitmap.into());
                }
                Err(windows::core::Error::from_thread())
            }
        }
        Err(e) => Err(e),
    };
    unsafe {
        let _ = DeleteDC(dc);
        let _ = DestroyIcon(info.hIcon);
    }
    result
}

fn bitmap_image(
    bitmap: windows::Win32::Graphics::Gdi::HBITMAP,
    opaque: Option<bool>,
) -> windows::core::Result<Arc<RenderImage>> {
    use windows::Win32::Graphics::Gdi::*;
    // RAII covers every failure after the provider hands ownership to us.
    struct OwnedBitmap(HBITMAP);
    impl Drop for OwnedBitmap {
        fn drop(&mut self) {
            unsafe {
                let _ = DeleteObject(self.0.into());
            }
        }
    }
    let bitmap = OwnedBitmap(bitmap);
    let mut object = BITMAP::default();
    if unsafe {
        GetObjectW(
            bitmap.0.into(),
            std::mem::size_of::<BITMAP>() as i32,
            Some((&mut object as *mut BITMAP).cast()),
        )
    } == 0
    {
        return Err(windows::core::Error::from_thread());
    }
    let width = object.bmWidth.unsigned_abs();
    let height = object.bmHeight.unsigned_abs();
    if width == 0 || height == 0 || width > 1024 || height > 1024 {
        return Err(windows::core::Error::from_hresult(windows::core::HRESULT(
            0x80070057u32 as i32,
        )));
    }
    let mut info = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: width as i32,
            biHeight: -(height as i32),
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut bytes = vec![0u8; (width * height * 4) as usize];
    let dc = unsafe { CreateCompatibleDC(None) };
    let copied = unsafe {
        GetDIBits(
            dc,
            bitmap.0,
            0,
            height,
            Some(bytes.as_mut_ptr().cast()),
            &mut info,
            DIB_RGB_COLORS,
        )
    };
    unsafe {
        let _ = DeleteDC(dc);
    }
    if copied != height as i32 {
        return Err(windows::core::Error::from_thread());
    }
    // Some JPEG/video providers return RGB with an all-zero alpha channel.
    let make_opaque = opaque.unwrap_or_else(|| bytes.chunks_exact(4).all(|p| p[3] == 0));
    // RenderImage::new accepts BGRA, exactly the layout returned by GetDIBits.
    // Converting this to RGBA swaps red/blue again when GPUI uploads it.
    for p in bytes.chunks_exact_mut(4) {
        if make_opaque {
            p[3] = 255;
        }
    }
    let pixels = image::RgbaImage::from_raw(width, height, bytes).unwrap();
    Ok(Arc::new(RenderImage::new(vec![image::Frame::new(pixels)])))
}
// Windows does not always install an SVG thumbnail provider. Use the existing
// rasterizer on this worker, with bounded input/output and no external resources.
fn svg_preview(key: &Key) -> Option<Arc<RenderImage>> {
    if !key.path.extension()?.eq_ignore_ascii_case("svg") {
        return None;
    }
    let mut bytes = Vec::new();
    std::fs::File::open(&key.path)
        .ok()?
        .take(4 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .ok()?;
    if bytes.len() > 4 * 1024 * 1024 {
        return None;
    }
    let tree = resvg::usvg::Tree::from_data(&bytes, &resvg::usvg::Options::default()).ok()?;
    let edge = u32::from(key.edge.clamp(1, 480));
    let scale = edge as f32 / tree.size().width().max(tree.size().height());
    let mut pixels = resvg::tiny_skia::Pixmap::new(edge, edge)?;
    resvg::render(
        &tree,
        resvg::tiny_skia::Transform::from_scale(scale, scale),
        &mut pixels.as_mut(),
    );
    let mut bytes = pixels.take();
    for pixel in bytes.chunks_exact_mut(4) {
        pixel.swap(0, 2);
    }
    let pixels = image::RgbaImage::from_raw(edge, edge, bytes)?;
    Some(Arc::new(RenderImage::new(vec![image::Frame::new(pixels)])))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn windows_bitmap_preserves_red_green_and_blue_in_gpui_bgra_order() {
        use windows::Win32::Graphics::Gdi::*;
        let info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: 3,
                biHeight: -1,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let expected = [0u8, 0, 255, 255, 0, 255, 0, 255, 255, 0, 0, 255];
        let mut bits = std::ptr::null_mut();
        let bitmap =
            unsafe { CreateDIBSection(None, &info, DIB_RGB_COLORS, &mut bits, None, 0) }.unwrap();
        unsafe {
            std::ptr::copy_nonoverlapping(expected.as_ptr(), bits.cast(), expected.len());
        }
        let rendered = bitmap_image(bitmap, None).unwrap();
        assert_eq!(rendered.as_bytes(0).unwrap(), expected);
    }

    #[test]
    fn large_previews_keep_source_resolution_and_have_no_shell_photo_frame() {
        use windows::Win32::System::Com::*;
        unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) }.unwrap();
        let temp = tempfile::tempdir().unwrap();
        // A real, large BMP: the outer pixels are red, not a black photo frame.
        let (width, height) = (1200u32, 800u32);
        let length = 54 + width * height * 3;
        let mut bmp = vec![0; length as usize];
        bmp[0..2].copy_from_slice(b"BM");
        bmp[2..6].copy_from_slice(&length.to_le_bytes());
        bmp[10..14].copy_from_slice(&54u32.to_le_bytes());
        bmp[14..18].copy_from_slice(&40u32.to_le_bytes());
        bmp[18..22].copy_from_slice(&width.to_le_bytes());
        bmp[22..26].copy_from_slice(&height.to_le_bytes());
        bmp[26..28].copy_from_slice(&1u16.to_le_bytes());
        bmp[28..30].copy_from_slice(&24u16.to_le_bytes());
        for pixel in bmp[54..].chunks_exact_mut(3) {
            pixel[2] = 255;
        }
        let mut k = key(1);
        k.path = temp.path().join("large.bmp");
        k.edge = PREVIEW_EDGE;
        std::fs::write(&k.path, bmp).unwrap();
        let preview = thumbnail(&k).unwrap();
        assert_eq!(
            preview.size(0).width,
            gpui::DevicePixels(PREVIEW_EDGE as i32)
        );
        assert_eq!(preview.size(0).height, gpui::DevicePixels(512));
        for pixel in preview.as_bytes(0).unwrap().chunks_exact(4) {
            assert_eq!(
                pixel,
                &[0, 0, 255, 255],
                "provider added a border or changed color"
            );
        }
        k.path = temp.path().to_owned();
        k.folder = true;
        let folder = thumbnail(&k).unwrap();
        assert!(
            folder.size(0).width >= gpui::DevicePixels(256),
            "folder preview enlarged a small icon"
        );
        unsafe { CoUninitialize() };
    }

    #[test]
    fn unrelated_events_preserve_preview_and_same_size_edits_are_detected() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("picture.png");
        std::fs::write(&path, include_bytes!("../assets/spike.png")).unwrap();
        let mut k = key(1);
        k.path = path;
        k.edge = 32;
        let (worker, replies) = Worker::start(temp.path().to_owned());
        worker.request(HashSet::from([k.clone()]));
        let until = std::time::Instant::now() + std::time::Duration::from_secs(10);
        let reply = loop {
            if let Ok(reply) = replies.try_recv() {
                break reply;
            }
            assert!(std::time::Instant::now() < until, "preview did not finish");
            std::thread::sleep(std::time::Duration::from_millis(10));
        };
        assert!(matches!(reply.value, Value::Image(Some(_))));
        for i in 0..10 {
            std::fs::write(temp.path().join("unrelated.log"), format!("event {i}")).unwrap();
            worker.force();
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
        assert!(
            replies.try_recv().is_err(),
            "unrelated writes regenerated the image"
        );
        assert!(worker.accepts(&k, reply.epoch));
        let before = Fingerprint::read(&k, temp.path());
        std::fs::File::options()
            .write(true)
            .open(&k.path)
            .unwrap()
            .set_modified(std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_900_000_001))
            .unwrap();
        assert_ne!(Fingerprint::read(&k, temp.path()), before);
        worker.force();
        let until = std::time::Instant::now() + std::time::Duration::from_secs(10);
        loop {
            if let Ok(reply) = replies.try_recv() {
                assert!(matches!(reply.value, Value::Image(Some(_))));
                break;
            }
            assert!(
                std::time::Instant::now() < until,
                "changed image was not refreshed"
            );
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    }
    fn key(n: u32) -> Key {
        Key {
            path: format!("{n}").into(),
            mtime: n,
            size: 0,
            edge: 32,
            details: false,
            folder: false,
        }
    }
    #[test]
    fn inactive_large_previews_are_released_but_list_icons_stay_cached() {
        let image = || {
            Arc::new(RenderImage::new(vec![image::Frame::new(
                image::RgbaImage::new(8, 8),
            )]))
        };
        let mut cache = Cache::new(96, 32 * 1024 * 1024);
        let mut a = key(1);
        a.edge = PREVIEW_EDGE;
        let mut b = key(2);
        b.edge = PREVIEW_EDGE;
        let icon = key(3);
        cache.insert(a.clone(), Value::Image(Some(image())));
        cache.insert(b.clone(), Value::Image(Some(image())));
        cache.insert(icon.clone(), Value::Image(Some(image())));
        let before = cache.bytes;
        assert_eq!(cache.retain_previews(&HashSet::from([b.clone()])).len(), 1);
        assert!(!cache.contains(&a));
        assert!(cache.contains(&b));
        assert!(cache.contains(&icon));
        assert_eq!(cache.bytes, before - 8 * 8 * 4);
        assert_eq!(cache.retain_previews(&HashSet::new()).len(), 1);
        assert!(cache.contains(&icon));
        assert_eq!(cache.bytes, 8 * 8 * 4);
    }
    #[test]
    fn lru_caps_storage_and_keeps_recently_used() {
        let mut c = Cache::new(2, 4096);
        c.insert(key(1), Value::Image(None));
        c.insert(key(2), Value::Image(None));
        c.get(&key(1));
        c.insert(key(3), Value::Image(None));
        assert!(c.contains(&key(1)));
        assert!(!c.contains(&key(2)));
        assert!(c.bytes <= 4096);
    }
    #[test]
    fn text_preview_is_bounded_and_reads_utf16() {
        let t = tempfile::tempdir().unwrap();
        let p = t.path().join("readme.md");
        let mut bytes = vec![0xff, 0xfe];
        for w in "Hello 🦀\nsecond".encode_utf16() {
            bytes.extend(w.to_le_bytes());
        }
        std::fs::write(&p, bytes).unwrap();
        assert_eq!(read_text(&p).unwrap(), "Hello 🦀\nsecond");
        std::fs::write(&p, "a\n".repeat(20000)).unwrap();
        assert_eq!(read_text(&p).unwrap().lines().count(), 80);
    }
    #[test]
    fn previews_use_real_pixels_and_budget_eviction_releases_images() {
        use windows::Win32::System::Com::*;
        unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) }.unwrap();
        let temp = tempfile::tempdir().unwrap();
        let png = temp.path().join("picture.png");
        std::fs::write(&png, include_bytes!("../assets/spike.png")).unwrap();
        let mut k = key(1);
        k.path = png;
        k.edge = 32;
        let native = thumbnail(&k).unwrap();
        assert!(!native.as_bytes(0).unwrap().is_empty());
        let svg = temp.path().join("picture.svg");
        std::fs::write(&svg, br##"<svg xmlns="http://www.w3.org/2000/svg" width="40" height="40"><rect width="40" height="40" fill="#ff0000"/></svg>"##).unwrap();
        k.path = svg;
        let image = svg_preview(&k).unwrap();
        assert_eq!(&image.as_bytes(0).unwrap()[0..4], &[0, 0, 255, 255]);
        let mut cache = Cache::new(96, 4096);
        cache.insert(key(1), Value::Image(Some(image.clone())));
        let released = cache.insert(key(2), Value::Image(Some(image)));
        assert_eq!(released.len(), 1);
        assert!(!cache.contains(&key(1)));
        assert_eq!(cache.clear().len(), 1);
        unsafe {
            CoUninitialize();
        }
    }
    #[test]
    fn cancelled_requests_reject_old_replies_and_open_dates_are_persisted() {
        let temp = tempfile::tempdir().unwrap();
        let (worker, _replies) = Worker::start(temp.path().to_owned());
        worker.request(HashSet::from([key(1)]));
        let epoch = worker.epoch.load(std::sync::atomic::Ordering::Acquire);
        assert!(worker.accepts(&key(1), epoch));
        worker.request(HashSet::from([key(2)]));
        assert!(!worker.accepts(&key(1), epoch));
        worker.force();
        assert!(worker.accepts(&key(2), epoch + 1));
        worker.request(HashSet::from([key(2), key(1)]));
        assert!(worker.accepts(&key(2), epoch + 1));
        assert!(!worker.accepts(&key(1), epoch));
        let path = temp.path().join("readme.md");
        std::fs::write(&path, "hello").unwrap();
        record_open(temp.path(), &path);
        let mut k = key(3);
        k.path = path;
        k.details = true;
        let d = details(&k, temp.path());
        assert_ne!(d.opened, "Not opened in Seek");
        assert_eq!(d.text.as_deref(), Some("hello"));
    }
}
