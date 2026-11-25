//! Image tools (Phase 8.3): convert, resize, rotate and flip. Every tool
//! saves a new copy beside the original, never over it: the copy's name says
//! what changed, gets a number if that name is taken, and is created with
//! `create_new`, so even a file that appears meanwhile is never replaced.
use image::{DynamicImage, GenericImageView, ImageFormat};
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    Png,
    Jpeg,
    Webp,
}
impl Format {
    pub fn extension(self) -> &'static str {
        match self {
            Format::Png => "png",
            Format::Jpeg => "jpg",
            Format::Webp => "webp",
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Format::Png => "PNG",
            Format::Jpeg => "JPEG",
            Format::Webp => "WebP",
        }
    }
    fn of(extension: &str) -> Option<Format> {
        match extension.to_ascii_lowercase().as_str() {
            "png" => Some(Format::Png),
            "jpg" | "jpeg" => Some(Format::Jpeg),
            "webp" => Some(Format::Webp),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Size {
    Width(u32),
    Height(u32),
    /// 1/2, 1/3 or 1/4 of each side.
    Fraction(u32),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tool {
    Convert(Format),
    Resize(Size),
    RotateLeft,
    RotateRight,
    Rotate180,
    FlipHorizontal,
    FlipVertical,
}
impl Tool {
    pub fn title(self) -> String {
        match self {
            Tool::Convert(format) => format!("Convert to {}", format.label()),
            Tool::Resize(_) => "Resize".into(),
            Tool::RotateLeft => "Rotate left".into(),
            Tool::RotateRight => "Rotate right".into(),
            Tool::Rotate180 => "Rotate 180°".into(),
            Tool::FlipHorizontal => "Flip horizontally".into(),
            Tool::FlipVertical => "Flip vertically".into(),
        }
    }
    /// What the copy's name says about it.
    fn note(self, to: (u32, u32)) -> Option<String> {
        match self {
            Tool::Convert(_) => None,
            Tool::Resize(_) => Some(format!("resized {}×{}", to.0, to.1)),
            Tool::RotateLeft | Tool::RotateRight | Tool::Rotate180 => Some("rotated".into()),
            Tool::FlipHorizontal | Tool::FlipVertical => Some("flipped".into()),
        }
    }
}

/// Pictures larger than this are refused rather than decoded.
const MAX_SIDE: u32 = 20_000;
const MAX_PIXELS: u64 = 120_000_000;

pub fn readable(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|ext| {
            matches!(
                ext.to_ascii_lowercase().as_str(),
                "png" | "jpg" | "jpeg" | "webp" | "bmp" | "gif" | "heic" | "heif"
            )
        })
}

fn heif(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|ext| matches!(ext.to_ascii_lowercase().as_str(), "heic" | "heif"))
}

fn placeholder(path: &Path) -> bool {
    use std::os::windows::fs::MetadataExt;
    std::fs::symlink_metadata(path)
        .is_ok_and(|m| m.file_attributes() & (0x400000 | 0x40000 | 0x1000) != 0)
}

const HEIF_MISSING: &str =
    "HEIC needs Microsoft's HEIF Image Extensions, which aren't installed on this PC";

/// Decodes HEIC/HEIF with Windows' codec, if one is installed.
fn decode_heif(path: &Path) -> Result<DynamicImage, String> {
    use windows::{Graphics::Imaging::*, Storage::*};
    let open = || -> windows::core::Result<DynamicImage> {
        let full = std::path::absolute(path).unwrap_or(path.into());
        let file = StorageFile::GetFileFromPathAsync(&windows::core::HSTRING::from(
            full.as_os_str(),
        ))?
        .join()?;
        let stream = file.OpenAsync(FileAccessMode::Read)?.join()?;
        let decoder = BitmapDecoder::CreateAsync(&stream)?.join()?;
        let (width, height) = (decoder.OrientedPixelWidth()?, decoder.OrientedPixelHeight()?);
        let pixels = decoder
            .GetPixelDataTransformedAsync(
                BitmapPixelFormat::Rgba8,
                BitmapAlphaMode::Straight,
                &BitmapTransform::new()?,
                ExifOrientationMode::RespectExifOrientation,
                ColorManagementMode::DoNotColorManage,
            )?
            .join()?
            .DetachPixelData()?
            .to_vec();
        image::RgbaImage::from_raw(width, height, pixels)
            .map(DynamicImage::ImageRgba8)
            .ok_or_else(|| windows::core::Error::from_hresult(windows::core::HRESULT(-1)))
    };
    open().map_err(|_| HEIF_MISSING.to_string())
}

fn dimensions(path: &Path) -> Result<(u32, u32), String> {
    if heif(path) {
        return decode_heif(path).map(|image| image.dimensions());
    }
    image::image_dimensions(path).map_err(|_| "This picture can't be read".to_string())
}

fn decode(path: &Path) -> Result<DynamicImage, String> {
    let (width, height) = dimensions(path)?;
    if width > MAX_SIDE || height > MAX_SIDE || width as u64 * height as u64 > MAX_PIXELS {
        return Err("This picture is too large to edit here".into());
    }
    if heif(path) {
        return decode_heif(path);
    }
    let mut reader = image::ImageReader::open(path)
        .map_err(|_| "This picture can't be opened".to_string())?
        .with_guessed_format()
        .map_err(|_| "This picture can't be opened".to_string())?;
    reader.no_limits();
    reader
        .decode()
        .map_err(|_| "This picture can't be read".to_string())
}

/// The new size for a tool, from the original's.
pub fn size_after(tool: Tool, (w, h): (u32, u32)) -> (u32, u32) {
    let scale = |a: u32, num: u32, den: u32| {
        ((a as u64 * num as u64 + den as u64 / 2) / den.max(1) as u64).max(1) as u32
    };
    match tool {
        Tool::Resize(Size::Width(width)) => (width.max(1), scale(h, width.max(1), w)),
        Tool::Resize(Size::Height(height)) => (scale(w, height.max(1), h), height.max(1)),
        Tool::Resize(Size::Fraction(n)) => (scale(w, 1, n.max(1)), scale(h, 1, n.max(1))),
        Tool::RotateLeft | Tool::RotateRight => (h, w),
        _ => (w, h),
    }
}

/// `photo.jpg` → `photo (resized 960×540).jpg`; a conversion only changes
/// the extension (or says "converted" when it would not).
pub fn copy_name(source: &Path, tool: Tool, to: (u32, u32)) -> String {
    let stem = source
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "image".into());
    let old = source
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    let extension = match tool {
        Tool::Convert(format) => format.extension().to_string(),
        // HEIC and the other read-only formats are written as PNG.
        _ => Format::of(&old)
            .map(|_| old.clone())
            .unwrap_or_else(|| "png".into()),
    };
    let note = tool.note(to).or_else(|| {
        (Format::of(&old) == Format::of(&extension)).then(|| "converted".to_string())
    });
    match note {
        Some(note) => format!("{stem} ({note}).{extension}"),
        None => format!("{stem}.{extension}"),
    }
}

/// `name.ext`, or `name 2.ext`, `name 3.ext`… whichever is free.
pub fn free_target(folder: &Path, name: &str) -> PathBuf {
    let first = folder.join(name);
    if std::fs::symlink_metadata(&first).is_err() {
        return first;
    }
    let (stem, ext) = match name.rfind('.') {
        Some(dot) if dot > 0 => (&name[..dot], &name[dot..]),
        _ => (name, ""),
    };
    (2..10_000)
        .map(|n| folder.join(format!("{stem} {n}{ext}")))
        .find(|path| std::fs::symlink_metadata(path).is_err())
        .unwrap_or(first)
}

#[derive(Clone, Debug)]
pub struct Planned {
    pub source: PathBuf,
    pub target: PathBuf,
    pub from: (u32, u32),
    pub to: (u32, u32),
}

/// What a tool would do to each path, or why a path is skipped. Reads only
/// the picture headers (HEIC is decoded to learn its size).
pub fn plan(paths: &[PathBuf], tool: Tool) -> Vec<Result<Planned, (PathBuf, String)>> {
    let mut claimed: Vec<PathBuf> = vec![];
    paths
        .iter()
        .map(|path| {
            if !readable(path) {
                return Err((path.clone(), "Not a picture".into()));
            }
            if placeholder(path) {
                return Err((path.clone(), "Not downloaded from the cloud".into()));
            }
            let from = dimensions(path).map_err(|e| (path.clone(), e))?;
            let to = size_after(tool, from);
            let folder = path.parent().unwrap_or(Path::new("."));
            let name = copy_name(path, tool, to);
            // Two sources must not plan the same copy name.
            let mut target = free_target(folder, &name);
            let mut n = 2;
            while claimed.iter().any(|c| c.eq(&target)) {
                let (stem, ext) = name.rsplit_once('.').unwrap_or((&name, ""));
                target = free_target(folder, &format!("{stem} {n}.{ext}"));
                n += 1;
            }
            claimed.push(target.clone());
            Ok(Planned {
                source: path.clone(),
                target,
                from,
                to,
            })
        })
        .collect()
}

fn apply(image: DynamicImage, tool: Tool, to: (u32, u32)) -> DynamicImage {
    match tool {
        Tool::Convert(_) => image,
        Tool::Resize(_) => image.resize_exact(to.0, to.1, image::imageops::FilterType::Lanczos3),
        Tool::RotateLeft => image.rotate270(),
        Tool::RotateRight => image.rotate90(),
        Tool::Rotate180 => image.rotate180(),
        Tool::FlipHorizontal => image.fliph(),
        Tool::FlipVertical => image.flipv(),
    }
}

/// Makes one copy. The target is created new (never replaced) and removed
/// again if writing fails, so a failure leaves nothing behind.
pub fn run(planned: &Planned, tool: Tool) -> Result<PathBuf, String> {
    let image = apply(decode(&planned.source)?, tool, planned.to);
    let format = planned
        .target
        .extension()
        .and_then(|e| e.to_str())
        .and_then(Format::of)
        .unwrap_or(Format::Png);
    let file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&planned.target)
        .map_err(|error| match error.kind() {
            std::io::ErrorKind::AlreadyExists => "A file with this name appeared".to_string(),
            _ => format!("Can't create the copy: {error}"),
        })?;
    let mut writer = std::io::BufWriter::new(file);
    let written = match format {
        Format::Png => image.write_to(&mut writer, ImageFormat::Png),
        Format::Jpeg => DynamicImage::ImageRgb8(image.to_rgb8()).write_with_encoder(
            image::codecs::jpeg::JpegEncoder::new_with_quality(&mut writer, 90),
        ),
        Format::Webp => DynamicImage::ImageRgba8(image.to_rgba8())
            .write_with_encoder(image::codecs::webp::WebPEncoder::new_lossless(&mut writer)),
    }
    .map_err(|_| "Can't write the copy".to_string())
    .and_then(|_| {
        use std::io::Write;
        writer
            .flush()
            .map_err(|_| "Can't write the copy".to_string())
    });
    drop(writer);
    if let Err(error) = written {
        let _ = std::fs::remove_file(&planned.target);
        return Err(error);
    }
    Ok(planned.target.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn picture(dir: &Path, name: &str, w: u32, h: u32) -> PathBuf {
        let path = dir.join(name);
        let mut image = image::RgbaImage::new(w, h);
        image.put_pixel(0, 0, image::Rgba([255, 0, 0, 255]));
        image.save(&path).unwrap();
        path
    }

    #[test]
    fn sizes_keep_proportions() {
        assert_eq!(size_after(Tool::Resize(Size::Width(960)), (1920, 1080)), (960, 540));
        assert_eq!(size_after(Tool::Resize(Size::Height(270)), (1920, 1080)), (480, 270));
        assert_eq!(size_after(Tool::Resize(Size::Fraction(3)), (900, 600)), (300, 200));
        assert_eq!(size_after(Tool::Resize(Size::Fraction(4)), (1, 1)), (1, 1));
        assert_eq!(size_after(Tool::RotateLeft, (40, 30)), (30, 40));
        assert_eq!(size_after(Tool::FlipVertical, (40, 30)), (40, 30));
    }

    #[test]
    fn copy_names_say_what_changed() {
        let p = Path::new(r"C:\x\photo.JPG");
        assert_eq!(copy_name(p, Tool::Convert(Format::Png), (1, 1)), "photo.png");
        assert_eq!(
            copy_name(p, Tool::Convert(Format::Jpeg), (1, 1)),
            "photo (converted).jpg"
        );
        assert_eq!(
            copy_name(p, Tool::Resize(Size::Fraction(2)), (960, 540)),
            "photo (resized 960×540).jpg"
        );
        assert_eq!(copy_name(p, Tool::FlipHorizontal, (1, 1)), "photo (flipped).jpg");
        assert_eq!(
            copy_name(Path::new("a.heic"), Tool::RotateRight, (1, 1)),
            "a (rotated).png",
            "read-only formats are written as PNG"
        );
    }

    #[test]
    fn copies_are_new_files_beside_the_original_and_never_overwrite() {
        let dir = tempfile::tempdir().unwrap();
        let source = picture(dir.path(), "shot.png", 40, 20);
        let before = std::fs::read(&source).unwrap();
        // The obvious name is taken: the copy gets a number.
        std::fs::write(dir.path().join("shot (rotated).png"), b"keep").unwrap();
        let plans = plan(std::slice::from_ref(&source), Tool::RotateLeft);
        let planned = plans[0].as_ref().unwrap();
        assert_eq!(planned.target, dir.path().join("shot (rotated) 2.png"));
        let made = run(planned, Tool::RotateLeft).unwrap();
        assert_eq!(image::image_dimensions(&made).unwrap(), (20, 40));
        assert_eq!(std::fs::read(&source).unwrap(), before, "original untouched");
        assert_eq!(
            std::fs::read(dir.path().join("shot (rotated).png")).unwrap(),
            b"keep"
        );
        // A file that appears after planning is never replaced.
        assert!(run(planned, Tool::RotateLeft).is_err());
        assert_eq!(image::image_dimensions(&made).unwrap(), (20, 40));
    }

    #[test]
    fn every_format_writes_and_batches_skip_what_is_not_a_picture() {
        let dir = tempfile::tempdir().unwrap();
        let a = picture(dir.path(), "a.png", 30, 30);
        let b = picture(dir.path(), "b.png", 30, 30);
        let text = dir.path().join("notes.txt");
        std::fs::write(&text, "x").unwrap();
        for format in [Format::Png, Format::Jpeg, Format::Webp] {
            let plans = plan(&[a.clone(), text.clone()], Tool::Convert(format));
            assert!(plans[1].is_err(), "text skipped");
            let made = run(plans[0].as_ref().unwrap(), Tool::Convert(format)).unwrap();
            assert_eq!(image::image_dimensions(&made).unwrap(), (30, 30));
        }
        // Two sources never plan the same copy.
        let plans = plan(&[a.clone(), b.clone()], Tool::Resize(Size::Fraction(2)));
        let targets: Vec<_> = plans.iter().map(|p| p.as_ref().unwrap().target.clone()).collect();
        assert_ne!(targets[0], targets[1]);
    }

    #[test]
    fn a_failed_write_leaves_nothing_behind() {
        let dir = tempfile::tempdir().unwrap();
        let broken = dir.path().join("broken.png");
        std::fs::write(&broken, b"not a png").unwrap();
        let plans = plan(std::slice::from_ref(&broken), Tool::FlipVertical);
        assert!(plans[0].is_err(), "unreadable pictures are skipped at planning");
        let planned = Planned {
            source: broken,
            target: dir.path().join("out.png"),
            from: (1, 1),
            to: (1, 1),
        };
        assert!(run(&planned, Tool::FlipVertical).is_err());
        assert!(!planned.target.exists());
    }
}
