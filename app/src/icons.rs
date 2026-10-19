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
        "ppt" | "pptx" | "pptm" | "odp" => Kind::Powerpoint,
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
#[derive(Clone)]
pub struct Atlas {
    cells: Arc<Vec<Arc<RenderImage>>>,
    locations: Arc<std::collections::HashMap<std::path::PathBuf, Arc<RenderImage>>>,
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
            locations: Arc::new(Default::default()),
        }
    }
    pub fn with_shell(self) -> Self {
        let mut images = self.cells.as_ref().clone();
        // Windows owns folders, file associations and Favorites artwork.
        // Bundled cells remain fallbacks when an association/provider is absent.
        use windows::Win32::System::Com::*;
        let initialized = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) }.is_ok();
        let mut locations = std::collections::HashMap::new();
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
            for (_, path) in crate::backend::favorites() {
                let key = crate::media::Key {
                    path: path.clone(),
                    mtime: 0,
                    size: 0,
                    edge: 48,
                    details: false,
                    folder: true,
                };
                if let Ok(image) = crate::media::thumbnail(&key) {
                    locations.insert(path, image);
                }
            }
            unsafe {
                CoUninitialize();
            }
        }
        Self {
            cells: Arc::new(images),
            locations: Arc::new(locations),
        }
    }
    pub fn get(&self, kind: Kind) -> Arc<RenderImage> {
        self.cells[kind as usize].clone()
    }
    pub fn location(&self, path: &Path) -> Option<Arc<RenderImage>> {
        self.locations.get(path).cloned()
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
