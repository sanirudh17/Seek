//! The optional picture behind the file list. Decoding and the effect are
//! baked into one texture off the UI thread; the view only places it.
//! The effects are adapted from Monodrive's `background.rs` (MIT, see
//! THIRD-PARTY-NOTICES.txt), itself ported from Monocode.
use gpui::RenderImage;
use image::RgbaImage;
use serde::{Deserialize, Serialize};
use std::{
    io::{Cursor, Read},
    path::Path,
    sync::Arc,
};

const MAX_BYTES: u64 = 16 * 1024 * 1024;

/// How the picture is drawn.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Effect {
    None,
    Dither,
    Ascii,
    Halftone,
    Scanlines,
    /// Blurred and faded into the window: the calm default.
    #[default]
    Haze,
}

impl Effect {
    pub const ALL: [Effect; 6] = [
        Effect::None,
        Effect::Dither,
        Effect::Ascii,
        Effect::Halftone,
        Effect::Scanlines,
        Effect::Haze,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Effect::None => "None",
            Effect::Dither => "Dither",
            Effect::Ascii => "ASCII",
            Effect::Halftone => "Halftone",
            Effect::Scanlines => "Scanlines",
            Effect::Haze => "Haze",
        }
    }
}

/// Where the picture shows: only in empty folders, or behind every folder.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Scope {
    Empty,
    #[default]
    All,
}

pub fn load(path: &Path, effect: Effect) -> Result<Arc<RenderImage>, String> {
    let mut bytes = Vec::new();
    let file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    if !file.metadata().map_err(|e| e.to_string())?.is_file() {
        return Err("Choose an image file.".into());
    }
    file.take(MAX_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() as u64 > MAX_BYTES {
        return Err("The image must be 16 MiB or smaller.".into());
    }
    decode(&bytes, effect)
}

fn decode(bytes: &[u8], effect: Effect) -> Result<Arc<RenderImage>, String> {
    let unreadable = || "Choose a PNG, JPEG, WebP, GIF or BMP image.".to_string();
    let reader = image::ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|_| unreadable())?;
    let (w, h) = reader.into_dimensions().map_err(|_| unreadable())?;
    if w == 0 || h == 0 || w as u64 * h as u64 > 16_000_000 {
        return Err("The image must have at most 16 million pixels.".into());
    }
    let mut reader = image::ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|_| unreadable())?;
    let mut limits = image::Limits::default();
    limits.max_alloc = Some(96 * 1024 * 1024);
    reader.limits(limits);
    let image = reader
        .decode()
        .map_err(|_| unreadable())?
        .thumbnail(1920, 1080);
    let image = if effect == Effect::Haze {
        image.blur(14.)
    } else {
        image
    };
    let mut pixels = texture(image.to_rgba8(), effect);
    // GPUI's textures are BGRA.
    for p in pixels.pixels_mut() {
        p.0.swap(0, 2);
    }
    Ok(Arc::new(RenderImage::new(vec![image::Frame::new(pixels)])))
}

/// The picture with its effect, at the picture's resolution. Haze is the
/// blur above plus the view's fade, so it leaves the pixels alone here.
fn texture(source: RgbaImage, effect: Effect) -> RgbaImage {
    if matches!(effect, Effect::None | Effect::Haze) {
        return source;
    }
    const BAYER: [[f32; 4]; 4] = [
        [0., 8., 2., 10.],
        [12., 4., 14., 6.],
        [3., 11., 1., 9.],
        [15., 7., 13., 5.],
    ];
    // 5×7 bitmaps from blank to dense, picked by brightness.
    const GLYPHS: [[u8; 7]; 10] = [
        [0, 0, 0, 0, 0, 0, 0],
        [0, 0, 0, 0, 0, 4, 0],
        [0, 4, 0, 0, 4, 0, 0],
        [0, 0, 0, 14, 0, 0, 0],
        [0, 0, 14, 0, 14, 0, 0],
        [0, 4, 4, 31, 4, 4, 0],
        [0, 21, 14, 31, 14, 21, 0],
        [10, 10, 31, 10, 31, 10, 10],
        [17, 2, 4, 4, 8, 16, 17],
        [14, 17, 23, 21, 23, 16, 14],
    ];
    let (width, height) = source.dimensions();
    let mut output = source.clone();
    let sample = |x: u32, y: u32| source.get_pixel(x.min(width - 1), y.min(height - 1));
    let luma = |p: &image::Rgba<u8>| {
        (p[0] as f32 * 0.2126 + p[1] as f32 * 0.7152 + p[2] as f32 * 0.0722).round()
    };
    for (x, y, pixel) in output.enumerate_pixels_mut() {
        match effect {
            Effect::None | Effect::Haze => {}
            Effect::Dither => {
                let p = sample(x / 2 * 2 + 1, y / 2 * 2 + 1);
                let peak = p[0].max(p[1]).max(p[2]) as f32;
                let gain = if peak / 255.
                    > (BAYER[(y / 2 % 4) as usize][(x / 2 % 4) as usize] + 0.5) / 16.
                {
                    255. / peak.max(1.)
                } else {
                    0.08
                };
                for c in 0..3 {
                    pixel[c] = (p[c] as f32 * gain).round() as u8;
                }
                pixel[3] = p[3];
            }
            Effect::Ascii => {
                let p = sample(x / 6 * 6 + 3, y / 8 * 8 + 4);
                let glyph = &GLYPHS[((luma(p) / 255.).sqrt() * 9.) as usize];
                let ink =
                    x % 6 < 5 && y % 8 < 7 && (glyph[(y % 8) as usize] & (1 << (4 - x % 6))) != 0;
                for c in 0..3 {
                    pixel[c] = (pixel[c] as f32 * 0.6 + if ink { p[c] as f32 * 0.4 } else { 0. })
                        .round() as u8;
                }
            }
            Effect::Halftone => {
                let radius = 2. * (0.3 + 0.7 * (luma(sample(x / 4 * 4, y / 4 * 4)) / 255.).sqrt());
                let p = sample(x / 4 * 4 + 2, y / 4 * 4 + 2);
                let distance = (x as f32 % 4. - 1.5).hypot(y as f32 % 4. - 1.5);
                let coverage = (radius + 0.5 - distance).clamp(0., 1.) * p[3] as f32 / 255.;
                for c in 0..3 {
                    pixel[c] = (pixel[c] as f32 * 0.6 + p[c] as f32 * coverage * 0.4).round() as u8;
                }
            }
            Effect::Scanlines => {
                if y % 3 == 0 {
                    for c in 0..3 {
                        pixel[c] = (pixel[c] as f32 * 0.52).round() as u8;
                    }
                }
            }
        }
    }
    output
}

/// Fine monochrome noise for the glass's optional grain: one tile, drawn
/// repeatedly at 1:1. Alpha carries the grain so it lightens and darkens
/// whatever is under it evenly.
/// The grain tile's side, in pixels.
pub const GRAIN_SIDE: u32 = 512;

pub fn grain_tile() -> Arc<RenderImage> {
    const SIDE: u32 = GRAIN_SIDE;
    let mut state = 0x9e37_79b9_u32;
    let tile = RgbaImage::from_fn(SIDE, SIDE, |_, _| {
        state ^= state << 13;
        state ^= state >> 17;
        state ^= state << 5;
        let v = (state >> 24) as u8;
        // Half the pixels lighten, half darken; at most ~4% either way.
        if v & 1 == 0 {
            image::Rgba([255, 255, 255, v % 11])
        } else {
            image::Rgba([0, 0, 0, v % 11])
        }
    });
    Arc::new(RenderImage::new(vec![image::Frame::new(tile)]))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn invalid_missing_oversize_and_valid_backgrounds() {
        let dir = tempfile::tempdir().unwrap();
        assert!(load(&dir.path().join("missing.png"), Effect::None).is_err());
        assert!(load(dir.path(), Effect::None).is_err());
        assert!(decode(b"not an image", Effect::None).is_err());
        let path = dir.path().join("large.png");
        std::fs::File::create(&path)
            .unwrap()
            .set_len(MAX_BYTES + 1)
            .unwrap();
        assert!(load(&path, Effect::None).unwrap_err().contains("16 MiB"));
        image::RgbaImage::from_pixel(16, 16, image::Rgba([220, 30, 10, 255]))
            .save(&path)
            .unwrap();
        for effect in [Effect::None, Effect::Haze] {
            let rendered = load(&path, effect).unwrap();
            assert_eq!(&rendered.as_bytes(0).unwrap()[..4], &[10, 30, 220, 255]);
        }
        let jpeg = dir.path().join("photo.jpg");
        image::RgbImage::from_pixel(16, 16, image::Rgb([20, 120, 220]))
            .save(&jpeg)
            .unwrap();
        assert!(load(&jpeg, Effect::Dither).is_ok());
    }

    #[test]
    fn baked_effects_change_pixels_and_haze_is_the_blur() {
        let source = RgbaImage::from_fn(24, 24, |x, y| {
            image::Rgba([(x * 10) as u8, (y * 10) as u8, 90, 255])
        });
        for effect in [
            Effect::Dither,
            Effect::Ascii,
            Effect::Halftone,
            Effect::Scanlines,
        ] {
            let result = texture(source.clone(), effect);
            assert_eq!(result.dimensions(), source.dimensions());
            assert_ne!(result, source, "{effect:?}");
        }
        for effect in [Effect::None, Effect::Haze] {
            assert_eq!(texture(source.clone(), effect), source);
        }
    }
}
