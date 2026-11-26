//! Similar images (Phase 9.5): pictures that look nearly the same (resized,
//! recompressed or lightly edited), found with a perceptual difference hash
//! (dHash: a 9×8 greyscale sample, one bit per left-brighter-than-right
//! pair) compared by Hamming distance, then verified on aspect ratio. Rules
//! only; no model, no face detection. These are similar, not identical:
//! nothing is chosen for the person, and one picture of every group stays.
use rayon::prelude::*;
use std::{
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
};

/// Pictures smaller than this are icons and stickers, not photos.
const SMALLEST: u64 = 8 * 1024;

/// Tests use tiny generated pictures.
fn big_enough(length: u64) -> bool {
    cfg!(test) || length >= SMALLEST
}
/// The most pictures one search hashes.
const MOST: usize = 10_000;
/// Pictures decoded at once: half the cores, two to six. A decoded photo is
/// large (a 24-megapixel JPEG is about 100 MB), so this bounds memory to a
/// few hundred MB however many cores the machine has.
fn decoders() -> usize {
    std::thread::available_parallelism().map_or(2, |n| n.get() / 2).clamp(2, 6)
}
/// The longest edge of a review thumbnail.
const THUMB: u32 = 96;
/// The largest distance any level allows; thumbnails cover this.
pub const LOOSEST: u32 = 12;
/// Distances for the review's levels: nearly identical, very similar, similar.
pub const LEVELS: [(u32, &str); 3] = [(3, "Nearly identical"), (7, "Very similar"), (LOOSEST, "Similar")];

#[derive(Clone)]
pub struct Picture {
    pub path: PathBuf,
    pub size: u64,
    pub width: u32,
    pub height: u32,
    /// Size and modified time when hashed; a changed picture is skipped.
    pub stamp: (u64, u64),
    /// Horizontal 64-bit difference hash (9x8).
    hash: u64,
    /// Vertical 64-bit difference hash (8x9).
    vhash: u64,
    /// Average colour. The hash sees only light and dark; two pictures of
    /// one layout in different colours, or two blank pictures, are told
    /// apart by this.
    tone: [u8; 3],
    /// The picture at 24 x 24 in RGB (1728 bytes), used for both global mean
    /// colour difference and worst 6x6 regional block difference so two UI
    /// screenshots with the same chrome or spatial colour swaps never group.
    fine: Arc<[u8]>,
    pub thumbnail: Option<Arc<gpui::RenderImage>>,
}

/// How far average colours may differ (per channel, out of 255) for two
/// pictures to be similar: a brightness or colour tweak, not a new colour.
const TONE: i32 = 40;

impl Picture {
    /// A flat picture (blank, one colour, a dark screenshot): its hashes say
    /// almost nothing, so it only matches a picture that is virtually identical.
    fn flat(&self) -> bool {
        let h = self.hash.count_ones();
        let v = self.vhash.count_ones();
        (h <= 3 || h >= 61) && (v <= 3 || v >= 61)
    }

    fn close_to(&self, other: &Picture, distance: u32) -> bool {
        let tone = self
            .tone
            .iter()
            .zip(other.tone)
            .map(|(a, b)| (i32::from(*a) - i32::from(b)).abs())
            .max()
            .unwrap_or(0);
        let hbits = (self.hash ^ other.hash).count_ones();
        let vbits = (self.vhash ^ other.vhash).count_ones();
        if !same_shape(self, other) {
            return false;
        }
        // The hashes propose, the 24 x 24 RGB comparison (both global mean and
        // worst 6x6 region) confirms: prevents dark UI screenshots with
        // different content or spatial colour swaps from false-positiving.
        let (global_diff, max_block_diff) = difference_rgb(self, other);
        if self.flat() || other.flat() {
            return hbits == 0
                && vbits == 0
                && tone <= 8
                && global_diff <= 60
                && max_block_diff <= 150;
        }
        let (tone_limit, global_limit, block_limit) = match distance {
            d if d <= LEVELS[0].0 => (25, 160, 480),
            d if d <= LEVELS[1].0 => (35, 360, 950),
            _ => (TONE, 600, 1450),
        };
        hbits <= distance
            && vbits <= distance
            && tone <= tone_limit
            && global_diff <= global_limit
            && max_block_diff <= block_limit
    }
}

#[derive(Default)]
pub struct Scan {
    pub pictures: Vec<Picture>,
    /// Pictures looked at.
    pub seen: usize,
    pub stopped: Option<String>,
}

fn is_picture(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| {
            matches!(
                e.to_ascii_lowercase().as_str(),
                "jpg" | "jpeg" | "jfif" | "png" | "webp" | "bmp" | "gif"
            )
        })
}

fn stamp(meta: &std::fs::Metadata) -> (u64, u64) {
    use std::os::windows::fs::MetadataExt;
    (meta.file_size(), meta.last_write_time())
}

/// Picture files under `folder`, skipping hidden and system items and links.
/// Picture files with their stamps, and why the listing stopped early.
type Listed = (Vec<(PathBuf, (u64, u64))>, Option<String>);

fn pictures_in(folder: &Path, skip_system: bool, cancel: &AtomicBool) -> Listed {
    use std::os::windows::fs::MetadataExt;
    let mut found = vec![];
    let mut stack = vec![folder.to_path_buf()];
    while let Some(dir) = stack.pop() {
        if cancel.load(Ordering::Relaxed) {
            return (found, Some("Stopped".into()));
        }
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        for entry in entries.flatten() {
            let Ok(meta) = entry.metadata() else { continue };
            // Hidden, system, reparse points (links, junctions, cloud
            // placeholders are read only if downloaded).
            if meta.file_attributes() & (0x2 | 0x4 | 0x400) != 0 {
                continue;
            }
            let path = entry.path();
            if skip_system {
                let rel = path.strip_prefix(folder).unwrap_or(&path);
                let skip = if meta.is_dir() {
                    crate::duplicates::system_path(&rel.join("_"))
                } else {
                    crate::duplicates::system_path(rel)
                };
                if skip {
                    continue;
                }
            }
            if meta.is_dir() {
                if skip_system && std::fs::symlink_metadata(path.join(".git")).is_ok() {
                    continue;
                }
                stack.push(path);
            } else if big_enough(meta.len())
                && is_picture(&path)
                && meta.file_attributes() & (0x0040_0000 | 0x0004_0000 | 0x1000) == 0
            {
                found.push((path, stamp(&meta)));
                if found.len() >= MOST {
                    return (found, Some(format!("Looked at the first {MOST} pictures")));
                }
            }
        }
    }
    (found, None)
}

/// The 64-bit horizontal difference hash of a picture (9x8).
fn dhash(image: &image::DynamicImage) -> u64 {
    let small = image.thumbnail_exact(9, 8).to_luma8();
    let mut hash = 0u64;
    for y in 0..8 {
        for x in 0..8 {
            hash <<= 1;
            if small.get_pixel(x, y)[0] < small.get_pixel(x + 1, y)[0] {
                hash |= 1;
            }
        }
    }
    hash
}

/// The 64-bit vertical difference hash of a picture (8x9).
fn vhash(image: &image::DynamicImage) -> u64 {
    let small = image.thumbnail_exact(8, 9).to_luma8();
    let mut hash = 0u64;
    for y in 0..8 {
        for x in 0..8 {
            hash <<= 1;
            if small.get_pixel(x, y)[0] < small.get_pixel(x, y + 1)[0] {
                hash |= 1;
            }
        }
    }
    hash
}

fn thumbnail(image: &image::DynamicImage) -> Arc<gpui::RenderImage> {
    let small = image.thumbnail(THUMB, THUMB).to_rgba8();
    // GPUI expects BGRA.
    let mut bgra = small;
    for pixel in bgra.pixels_mut() {
        pixel.0.swap(0, 2);
    }
    Arc::new(gpui::RenderImage::new(vec![image::Frame::new(bgra)]))
}

/// Decoded upright: a phone photo stores its rotation as a tag, and a
/// rotated export of it has the pixels turned instead; both compare alike.
fn open(path: &Path) -> Option<image::DynamicImage> {
    use image::ImageDecoder;
    let mut reader = image::ImageReader::open(path).ok()?.with_guessed_format().ok()?;
    let mut limits = image::Limits::default();
    limits.max_alloc = Some(512 * 1024 * 1024);
    reader.limits(limits);
    let mut decoder = reader.into_decoder().ok()?;
    let orientation = decoder.orientation().unwrap_or(image::metadata::Orientation::NoTransforms);
    let mut image = image::DynamicImage::from_decoder(decoder).ok()?;
    image.apply_orientation(orientation);
    Some(image)
}

/// The picture at 24 x 24 in RGB (1728 bytes).
const FINE_SIDE: usize = 24;
const FINE_LEN: usize = FINE_SIDE * FINE_SIDE * 3;

fn fine(image: &image::DynamicImage) -> Arc<[u8]> {
    image
        .thumbnail_exact(FINE_SIDE as u32, FINE_SIDE as u32)
        .to_rgb8()
        .into_raw()
        .into()
}

/// How different two pictures are at 24 x 24 RGB: returns `(global, max_block)`
/// in hundredths of a percent of full scale, where `max_block` is the largest
/// mean channel difference across the 16 (4x4 grid of 6x6 pixel) sub-regions.
fn difference_rgb(a: &Picture, b: &Picture) -> (u32, u32) {
    if a.fine.len() != FINE_LEN || b.fine.len() != FINE_LEN {
        return (u32::MAX, u32::MAX);
    }
    let mut total = 0u64;
    let mut blocks = [0u64; 16];
    for y in 0..FINE_SIDE {
        let by = y / 6;
        for x in 0..FINE_SIDE {
            let bx = x / 6;
            let i = (y * FINE_SIDE + x) * 3;
            let d = u64::from(a.fine[i].abs_diff(b.fine[i]))
                + u64::from(a.fine[i + 1].abs_diff(b.fine[i + 1]))
                + u64::from(a.fine[i + 2].abs_diff(b.fine[i + 2]));
            total += d;
            blocks[by * 4 + bx] += d;
        }
    }
    let global = (total * 10_000 / (FINE_LEN as u64 * 255)) as u32;
    let block_channels = (6 * 6 * 3) as u64 * 255;
    let max_block = blocks
        .into_iter()
        .map(|s| (s * 10_000 / block_channels) as u32)
        .max()
        .unwrap_or(0);
    (global, max_block)
}

/// The average colour of a picture.
fn tone(image: &image::DynamicImage) -> [u8; 3] {
    let small = image.thumbnail_exact(8, 8).to_rgb8();
    let mut sum = [0u32; 3];
    for pixel in small.pixels() {
        for (total, value) in sum.iter_mut().zip(pixel.0) {
            *total += u32::from(value);
        }
    }
    let n = (small.width() * small.height()).max(1);
    [(sum[0] / n) as u8, (sum[1] / n) as u8, (sum[2] / n) as u8]
}

/// Hashes every picture under `folders` in parallel; then (for the review,
/// not for agents) makes thumbnails for the pictures that have a near twin
/// at the loosest level.
pub fn scan(
    folders: &[std::path::PathBuf],
    cancel: &AtomicBool,
    thumbnails: bool,
    progress: impl Fn(usize, usize, &'static str) + Sync,
) -> Scan {
    scan_in(folders, false, cancel, thumbnails, progress)
}

/// Like `scan`, optionally skipping system, app-data, dot-folders and Git
/// repositories (used by "Search everywhere").
pub fn scan_in(
    folders: &[std::path::PathBuf],
    skip_system: bool,
    cancel: &AtomicBool,
    thumbnails: bool,
    progress: impl Fn(usize, usize, &'static str) + Sync,
) -> Scan {
    progress(0, 0, "Listing pictures");
    let mut files = vec![];
    let mut stopped = None;
    for folder in folders {
        let (found, why) = pictures_in(folder, skip_system, cancel);
        files.extend(found);
        if why.is_some() || files.len() >= MOST {
            files.truncate(MOST);
            stopped = why.or_else(|| Some(format!("Looked at the first {MOST} pictures")));
            break;
        }
    }
    files.sort_by(|a, b| a.0.cmp(&b.0));
    files.dedup_by(|a, b| a.0.as_os_str().eq_ignore_ascii_case(b.0.as_os_str()));
    // Hard links to one picture are the same file, never similar duplicates.
    let mut seen_ids = std::collections::HashSet::new();
    files.retain(|(path, _)| crate::duplicates::file_id(path).is_none_or(|id| seen_ids.insert(id)));
    let total = files.len();
    let done = AtomicUsize::new(0);
    let pool = rayon::ThreadPoolBuilder::new().num_threads(decoders()).build();
    let bounded = |work: &mut (dyn FnMut() + Send)| match &pool {
        Ok(pool) => pool.install(work),
        Err(_) => work(),
    };
    let mut pictures: Vec<Picture> = vec![];
    bounded(&mut || pictures = files
        .par_iter()
        .filter_map(|(path, stamp)| {
            if cancel.load(Ordering::Relaxed) {
                return None;
            }
            let image = open(path);
            let n = done.fetch_add(1, Ordering::Relaxed) + 1;
            if n.is_multiple_of(25) || n == total {
                progress(n, total, "Comparing pictures");
            }
            let image = image?;
            Some(Picture {
                path: path.clone(),
                size: stamp.0,
                width: image.width(),
                height: image.height(),
                stamp: *stamp,
                hash: dhash(&image),
                vhash: vhash(&image),
                tone: tone(&image),
                fine: fine(&image),
                thumbnail: None,
            })
        })
        .collect());
    if cancel.load(Ordering::Relaxed) {
        stopped = Some("Stopped".into());
    }
    pictures.sort_by(|a, b| a.path.cmp(&b.path));
    if !thumbnails {
        return Scan {
            seen: total,
            pictures,
            stopped,
        };
    }
    // Thumbnails only for pictures that can appear in a group.
    let grouped: std::collections::HashSet<usize> = groups(&pictures, LOOSEST).into_iter().flatten().collect();
    progress(0, grouped.len(), "Making previews");
    let mut thumbs: Vec<(usize, Option<Arc<gpui::RenderImage>>)> = vec![];
    bounded(&mut || thumbs = grouped
        .par_iter()
        .map(|&i| (i, open(&pictures[i].path).map(|image| thumbnail(&image))))
        .collect());
    for (i, thumb) in thumbs {
        pictures[i].thumbnail = thumb;
    }
    Scan {
        seen: total,
        pictures,
        stopped,
    }
}

/// Whether two pictures have the same shape: within 3% aspect ratio, so a
/// resized copy matches but a crop to another shape does not.
fn same_shape(a: &Picture, b: &Picture) -> bool {
    let ratio = |p: &Picture| p.width as f64 / p.height.max(1) as f64;
    let (x, y) = (ratio(a), ratio(b));
    (x - y).abs() <= 0.03 * x.max(y)
}

/// Groups of pictures that look like one picture: the largest picture not
/// yet in a group anchors one, and the pictures within `distance` of it
/// (same shape, similar colour) join it. Every picture is compared with the
/// anchor, not with whichever member it happens to resemble, so a chain of
/// small differences never joins two pictures that look nothing alike.
/// Largest total size first; each group lists picture indexes.
pub fn groups(pictures: &[Picture], distance: u32) -> Vec<Vec<usize>> {
    let mut order: Vec<usize> = (0..pictures.len()).collect();
    // Largest first, then by path, so the grouping never depends on order.
    order.sort_by(|&a, &b| pictures[b].size.cmp(&pictures[a].size).then(pictures[a].path.cmp(&pictures[b].path)));
    let mut taken = vec![false; pictures.len()];
    let mut out: Vec<Vec<usize>> = vec![];
    for (k, &anchor) in order.iter().enumerate() {
        if taken[anchor] {
            continue;
        }
        let mut set = vec![anchor];
        for &other in &order[k + 1..] {
            if !taken[other] && pictures[anchor].close_to(&pictures[other], distance) {
                set.push(other);
            }
        }
        if set.len() > 1 {
            for &i in &set {
                taken[i] = true;
            }
            set.sort_unstable();
            out.push(set);
        }
    }
    out.sort_by_key(|set| std::cmp::Reverse(set.iter().map(|&i| pictures[i].size).sum::<u64>()));
    out
}

/// Whether a picture still looks as it did when it was hashed.
pub fn unchanged(picture: &Picture) -> bool {
    std::fs::symlink_metadata(&picture.path).is_ok_and(|meta| meta.is_file() && stamp(&meta) == picture.stamp)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn picture(width: u32, height: u32, seed: u32) -> image::RgbImage {
        image::RgbImage::from_fn(width, height, |x, y| {
            let v = ((x * 255 / width) ^ (y * 97 / height) ^ seed) as u8;
            image::Rgb([v, v.wrapping_mul(3), 255 - v])
        })
    }

    #[test]
    fn resized_and_recompressed_copies_group_and_different_pictures_do_not() {
        let dir = tempfile::tempdir().unwrap();
        let original = picture(640, 480, 0);
        original.save(dir.path().join("beach.png")).unwrap();
        // The same picture, smaller and as a JPEG.
        image::DynamicImage::ImageRgb8(original.clone())
            .resize_exact(320, 240, image::imageops::FilterType::Triangle)
            .to_rgb8()
            .save(dir.path().join("beach small.jpg"))
            .unwrap();
        // A different picture, and the same picture cropped to a square.
        picture(640, 480, 0x5a).save(dir.path().join("other.png")).unwrap();
        image::imageops::crop_imm(&original, 0, 0, 480, 480)
            .to_image()
            .save(dir.path().join("square.png"))
            .unwrap();
        let scan = scan(&[dir.path().to_path_buf()], &AtomicBool::new(false), true, |_, _, _| {});
        assert_eq!(scan.pictures.len(), 4);
        let found = groups(&scan.pictures, LEVELS[1].0);
        assert_eq!(found.len(), 1, "{found:?}");
        let names: Vec<String> = found[0]
            .iter()
            .map(|&i| scan.pictures[i].path.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, ["beach small.jpg", "beach.png"]);
        assert!(found[0].iter().all(|&i| scan.pictures[i].thumbnail.is_some()));
        assert!(scan.pictures.iter().all(unchanged));
    }

    fn made(name: &str, hash: u64, tone: [u8; 3], size: u64) -> Picture {
        let mut fine = Vec::with_capacity(FINE_LEN);
        for _ in 0..(FINE_SIDE * FINE_SIDE) {
            fine.extend_from_slice(&tone);
        }
        Picture {
            path: PathBuf::from(name),
            size,
            width: 100,
            height: 100,
            stamp: (0, 0),
            hash,
            vhash: hash,
            tone,
            fine: fine.into(),
            thumbnail: None,
        }
    }

    #[test]
    fn blank_pictures_and_other_colours_are_not_similar() {
        // Two different blank pictures hash alike; only an exact match of
        // a flat picture groups.
        let black = made("black.png", 0, [0, 0, 0], 10);
        let white = made("white.png", 0, [255, 255, 255], 10);
        let black_copy = made("black copy.png", 0, [1, 1, 1], 9);
        let found = groups(&[black.clone(), white, black_copy], LEVELS[2].0);
        assert_eq!(found, vec![vec![0, 2]]);
        // One layout in another colour is a different picture.
        let red = made("red.png", 0x0f0f_f0f0_3c3c_c3c3, [200, 30, 30], 10);
        let blue = made("blue.png", 0x0f0f_f0f0_3c3c_c3c3, [30, 30, 200], 10);
        assert!(groups(&[red, blue], LEVELS[2].0).is_empty());
    }

    #[test]
    fn dark_app_screenshots_with_different_content_and_color_swaps_do_not_false_positive() {
        let dir = tempfile::tempdir().unwrap();
        // Two 640x360 dark-mode UI screenshots sharing identical sidebar & top
        // bar chrome, but showing completely different content cards in the middle.
        let mut shot_a = image::RgbImage::from_pixel(640, 360, image::Rgb([22, 22, 25]));
        let mut shot_b = shot_a.clone();
        for x in 0..140 {
            for y in 0..360 {
                shot_a.put_pixel(x, y, image::Rgb([30, 30, 34]));
                shot_b.put_pixel(x, y, image::Rgb([30, 30, 34]));
            }
        }
        // Bright card in middle-left of shot A vs bright card in lower-right of shot B.
        for x in 200..360 {
            for y in 80..180 {
                shot_a.put_pixel(x, y, image::Rgb([220, 220, 230]));
            }
        }
        for x in 420..580 {
            for y in 200..300 {
                shot_b.put_pixel(x, y, image::Rgb([220, 220, 230]));
            }
        }
        shot_a.save(dir.path().join("shot_a.png")).unwrap();
        shot_b.save(dir.path().join("shot_b.png")).unwrap();
        // Two images with identical global average tone (half red, half blue)
        // but flipped spatial layout.
        let swap_a = image::RgbImage::from_fn(200, 200, |x, _| {
            if x < 100 { image::Rgb([210, 40, 40]) } else { image::Rgb([40, 40, 210]) }
        });
        let swap_b = image::RgbImage::from_fn(200, 200, |x, _| {
            if x < 100 { image::Rgb([40, 40, 210]) } else { image::Rgb([210, 40, 40]) }
        });
        swap_a.save(dir.path().join("swap_a.png")).unwrap();
        swap_b.save(dir.path().join("swap_b.png")).unwrap();
        // Hard link to shot_a must not show up as a second picture at all.
        std::fs::hard_link(dir.path().join("shot_a.png"), dir.path().join("shot_a_link.png")).unwrap();

        let scan = scan(&[dir.path().to_path_buf()], &AtomicBool::new(false), false, |_, _, _| {});
        assert_eq!(scan.pictures.len(), 4, "hard link must be deduplicated");
        assert!(
            groups(&scan.pictures, LEVELS[2].0).is_empty(),
            "distinct screenshots and color swaps must not group even at loosest level"
        );
    }

    /// Read-only: prints the groups found under $SEEK_SIMILAR_SURVEY.
    #[test]
    #[ignore]
    fn survey() {
        let Some(folder) = std::env::var_os("SEEK_SIMILAR_SURVEY") else { return };
        let scan = scan(&[PathBuf::from(folder)], &AtomicBool::new(false), false, |_, _, _| {});
        for (level, name) in LEVELS {
            let found = groups(&scan.pictures, level);
            println!("== {name}: {} groups among {}", found.len(), scan.pictures.len());
            for g in found.iter().take(12) {
                let names: Vec<String> = g
                    .iter()
                    .map(|&i| {
                        let p = &scan.pictures[i];
                        let (gd, bd) = difference_rgb(p, &scan.pictures[g[0]]);
                        format!(
                            "{} (bits {}, diff {:.2}% / block {:.2}%)",
                            p.path.file_name().unwrap().to_string_lossy(),
                            (p.hash ^ scan.pictures[g[0]].hash).count_ones(),
                            gd as f64 / 100.,
                            bd as f64 / 100.,
                        )
                    })
                    .collect();
                println!("   {}", names.join("  |  "));
            }
        }
    }

    #[test]
    fn small_differences_do_not_chain_into_one_group() {
        // B is 7 bits from A, C is 7 bits from B but 14 from A.
        let base = 0x0f0f_f0f0_3c3c_c3c3u64;
        let a = made("a.png", base, [90, 90, 90], 30);
        let b = made("b.png", base ^ 0x7f, [90, 90, 90], 20);
        let c = made("c.png", base ^ 0x3fff, [90, 90, 90], 10);
        let found = groups(&[a, b, c], LEVELS[1].0);
        assert_eq!(found, vec![vec![0, 1]], "c is not like a");
    }
}
