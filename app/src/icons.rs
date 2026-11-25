use gpui::RenderImage;
use std::{path::Path, sync::Arc};
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(usize)]
pub enum Kind {
    Folder,
    Word,
    Excel,
    Powerpoint,
    Pdf,
    Text,
    Code,
    Archive,
    Audio,
    Video,
    Executable,
    Image,
    Generic,
}
pub fn kind(path: &Path, folder: bool) -> Kind {
    if folder {
        return Kind::Folder;
    }
    match path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase()
        .as_str()
    {
        "doc" | "docx" | "docm" | "dot" | "dotx" | "odt" => Kind::Word,
        "xls" | "xlsx" | "xlsm" | "csv" | "ods" => Kind::Excel,
        "ppt" | "pptx" | "pptm" | "pps" | "ppsx" | "ppsm" | "pot" | "potx" | "potm" | "ppam"
        | "sldx" | "sldm" | "odp" => Kind::Powerpoint,
        "pdf" => Kind::Pdf,
        "txt" | "md" | "markdown" | "rtf" | "log" => Kind::Text,
        "rs" | "py" | "js" | "jsx" | "ts" | "tsx" | "c" | "cc" | "cpp" | "h" | "hpp" | "cs"
        | "go" | "java" | "swift" | "html" | "css" | "json" | "xml" | "yaml" | "yml" | "toml"
        | "sh" | "ps1" | "bat" | "sql" | "vue" | "svelte" | "dart" | "kt" | "rb" | "php" => {
            Kind::Code
        }
        "zip" | "rar" | "7z" | "tar" | "gz" | "bz2" | "xz" | "zst" | "iso" => Kind::Archive,
        "mp3" | "wav" | "flac" | "ogg" | "m4a" | "aac" | "opus" | "mid" => Kind::Audio,
        "mp4" | "mov" | "m4v" | "mkv" | "avi" | "webm" | "wmv" | "mpeg" | "mpg" | "3gp" => {
            Kind::Video
        }
        "exe" | "msi" | "com" | "dll" => Kind::Executable,
        "jpg" | "jpeg" | "png" | "gif" | "webp" | "bmp" | "heic" | "heif" | "svg" | "tif"
        | "tiff" | "avif" | "ico" => Kind::Image,
        _ => Kind::Generic,
    }
}
/// Windows' icon for each file type, by lowercase extension, fetched once
/// per type as soon as a folder's rows arrive. Rows show it until (and for
/// most types instead of) a per-file shell icon, so a folder's icons appear
/// with its rows rather than one by one as each file is asked about.
static TYPE_ICONS: std::sync::LazyLock<
    std::sync::Mutex<std::collections::HashMap<String, Option<Arc<RenderImage>>>>,
> = std::sync::LazyLock::new(Default::default);

/// Types whose icon belongs to each file, not to the type.
fn per_file(extension: &str) -> bool {
    matches!(
        extension,
        "" | "exe" | "lnk" | "ico" | "url" | "cur" | "ani" | "scr" | "msc" | "appref-ms" | "website"
    )
}

fn extension(path: &Path) -> String {
    path.extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase()
}

/// The type's icon, once fetched.
pub fn type_icon(path: &Path, folder: bool) -> Option<Arc<RenderImage>> {
    if folder {
        return None;
    }
    let extension = extension(path);
    if per_file(&extension) {
        return None;
    }
    TYPE_ICONS.lock().ok()?.get(&extension).cloned().flatten()
}

/// Types among these files not fetched or being fetched yet; they are
/// marked as being fetched.
pub fn unfetched_types<'a>(files: impl Iterator<Item = (&'a Path, bool)>) -> Vec<String> {
    let Ok(mut known) = TYPE_ICONS.lock() else {
        return vec![];
    };
    let mut wanted = vec![];
    for (path, folder) in files {
        if folder {
            continue;
        }
        let extension = extension(path);
        if per_file(&extension) || known.contains_key(&extension) {
            continue;
        }
        known.insert(extension.clone(), None);
        wanted.push(extension);
    }
    wanted
}

/// Fetches type icons on the calling (background) thread.
pub fn fetch_types(extensions: Vec<String>) {
    use windows::Win32::System::Com::*;
    let initialized = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) }.is_ok();
    for extension in extensions {
        let icon = crate::media::association_icon(&extension).ok();
        if let Ok(mut known) = TYPE_ICONS.lock() {
            known.insert(extension, icon);
        }
    }
    if initialized {
        unsafe { CoUninitialize() };
    }
}

#[derive(Clone)]
pub struct Atlas {
    cells: Arc<Vec<Arc<RenderImage>>>,
}
impl Atlas {
    /// One embedded SVG sprite is rasterized once on the startup worker. The
    /// resulting cells enter GPUI's GPU image atlas and are shared by all views.
    #[cfg(test)]
    pub fn new() -> Self {
        Self::bundled().with_shell()
    }
    pub fn bundled() -> Self {
        let tree = resvg::usvg::Tree::from_data(
            include_bytes!("../assets/file-icons.svg"),
            &resvg::usvg::Options::default(),
        )
        .expect("bundled icon sprite");
        let mut pixels = resvg::tiny_skia::Pixmap::new(13 * 256, 256).unwrap();
        resvg::render(
            &tree,
            resvg::tiny_skia::Transform::identity(),
            &mut pixels.as_mut(),
        );
        let images: Vec<_> = (0..13)
            .map(|i| {
                let mut cell = image::RgbaImage::new(256, 256);
                for y in 0..256 {
                    for x in 0..256 {
                        let at = ((y * 13 * 256 + i * 256 + x) * 4) as usize;
                        let b = &pixels.data()[at..at + 4];
                        cell.put_pixel(x, y, image::Rgba([b[2], b[1], b[0], b[3]]));
                    }
                }
                Arc::new(RenderImage::new(vec![image::Frame::new(cell)]))
            })
            .collect();
        Self {
            cells: Arc::new(images),
        }
    }
    pub fn with_shell(self) -> Self {
        let mut images = self.cells.as_ref().clone();
        // Windows owns folder and file-association artwork; the sidebar's
        // places use Seek's own tinted icons.
        // Bundled cells remain fallbacks when an association/provider is absent.
        use windows::Win32::System::Com::*;
        let initialized = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) }.is_ok();
        if initialized {
            for (kind, ext) in [
                (Kind::Folder, ""),
                (Kind::Word, "docx"),
                (Kind::Excel, "xlsx"),
                (Kind::Powerpoint, "pptx"),
                (Kind::Pdf, "pdf"),
                (Kind::Text, "txt"),
                (Kind::Code, "json"),
                (Kind::Archive, "zip"),
                (Kind::Audio, "mp3"),
                (Kind::Video, "mp4"),
                (Kind::Executable, "exe"),
                (Kind::Image, "png"),
                (Kind::Generic, "xyz"),
            ] {
                if let Ok(image) = crate::media::association_icon(ext) {
                    images[kind as usize] = image;
                }
            }
            unsafe {
                CoUninitialize();
            }
        }
        Self {
            cells: Arc::new(images),
        }
    }
    pub fn get(&self, kind: Kind) -> Arc<RenderImage> {
        self.cells[kind as usize].clone()
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn extensions_are_case_insensitive_and_markdown_is_text() {
        assert_eq!(kind(Path::new("a.DOCX"), false), Kind::Word);
        assert_eq!(kind(Path::new("a.md"), false), Kind::Text);
        assert_eq!(kind(Path::new("a.mp4"), false), Kind::Video);
        assert_eq!(kind(Path::new("folder.exe"), true), Kind::Folder);
        assert_eq!(kind(Path::new("a.unknown"), false), Kind::Generic);
    }
    #[test]
    fn sprite_has_all_cells() {
        let a = Atlas::new();
        for k in [Kind::Folder, Kind::Word, Kind::Image, Kind::Generic] {
            assert!(!a.get(k).as_bytes(0).unwrap().is_empty());
        }
    }
}
