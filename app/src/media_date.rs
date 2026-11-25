//! When a photo or video was taken, read from the file itself: EXIF
//! DateTimeOriginal (then DateTimeDigitized, then DateTime) for pictures, the
//! MP4/QuickTime movie header's creation time for videos, else the modified
//! date. Used by Suggest Names and Organize by date taken. Reads at most the
//! headers; never decodes an image.
use chrono::{Datelike, NaiveDateTime};
use std::{
    io::{Read, Seek, SeekFrom},
    path::Path,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Source {
    /// EXIF date taken.
    Taken,
    /// A video's recorded creation time.
    Recorded,
    /// The file's modified date: nothing better was in the file.
    Modified,
}

impl Source {
    pub fn label(self) -> &'static str {
        match self {
            Source::Taken => "date taken",
            Source::Recorded => "date recorded",
            Source::Modified => "date modified",
        }
    }
}

const PICTURES: [&str; 12] = [
    "jpg", "jpeg", "jfif", "heic", "heif", "tif", "tiff", "dng", "cr2", "nef", "arw", "webp",
];
const MOVIES: [&str; 5] = ["mp4", "mov", "m4v", "3gp", "3g2"];

fn extension(path: &Path) -> String {
    path.extension()
        .and_then(|e| e.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase()
}

pub fn is_photo(path: &Path) -> bool {
    PICTURES.contains(&extension(path).as_str()) || extension(path) == "png"
}

pub fn is_video(path: &Path) -> bool {
    MOVIES.contains(&extension(path).as_str())
        || matches!(extension(path).as_str(), "mkv" | "avi" | "webm" | "wmv")
}

/// The capture date from inside the file, if it has one.
pub fn embedded(path: &Path) -> Option<(NaiveDateTime, Source)> {
    let extension = extension(path);
    if PICTURES.contains(&extension.as_str()) {
        exif_date(path).map(|date| (date, Source::Taken))
    } else if MOVIES.contains(&extension.as_str()) {
        movie_date(path).map(|date| (date, Source::Recorded))
    } else {
        None
    }
}

/// The capture date, falling back to the modified date (local time).
pub fn captured(path: &Path) -> Option<(NaiveDateTime, Source)> {
    embedded(path).or_else(|| {
        let modified = std::fs::metadata(path).ok()?.modified().ok()?;
        let local: chrono::DateTime<chrono::Local> = modified.into();
        Some((local.naive_local(), Source::Modified))
    })
}

fn plausible(date: NaiveDateTime) -> bool {
    // Cameras with an unset clock write 1970 or 2000-01-01; reject those
    // and anything in the future.
    let now = chrono::Local::now().naive_local();
    date.year() >= 1990 && date <= now + chrono::Duration::days(2)
}

fn exif_date(path: &Path) -> Option<NaiveDateTime> {
    let file = std::fs::File::open(path).ok()?;
    let mut reader = std::io::BufReader::new(file);
    let exif = exif::Reader::new().read_from_container(&mut reader).ok()?;
    for tag in [exif::Tag::DateTimeOriginal, exif::Tag::DateTimeDigitized, exif::Tag::DateTime] {
        let Some(field) = exif.get_field(tag, exif::In::PRIMARY) else {
            continue;
        };
        if let exif::Value::Ascii(ref parts) = field.value
            && let Some(bytes) = parts.first()
            && let Ok(date) = exif::DateTime::from_ascii(bytes)
            && let Some(day) = chrono::NaiveDate::from_ymd_opt(
                date.year.into(),
                date.month.into(),
                date.day.into(),
            )
            && let Some(time) = chrono::NaiveTime::from_hms_opt(
                date.hour.into(),
                date.minute.into(),
                date.second.into(),
            )
        {
            let taken = NaiveDateTime::new(day, time);
            if plausible(taken) {
                return Some(taken);
            }
        }
    }
    None
}

/// The `mvhd` creation time of an MP4/QuickTime file: seconds since
/// 1904-01-01 UTC, shown in local time. Walks top-level boxes to `moov`
/// (which may sit at the end) without reading media data.
fn movie_date(path: &Path) -> Option<NaiveDateTime> {
    let mut file = std::fs::File::open(path).ok()?;
    let length = file.metadata().ok()?.len();
    let moov = find_box(&mut file, 0, length, b"moov")?;
    let mvhd = find_box(&mut file, moov.0, moov.1, b"mvhd")?;
    file.seek(SeekFrom::Start(mvhd.0)).ok()?;
    let mut head = [0u8; 12];
    file.read_exact(&mut head).ok()?;
    let seconds = if head[0] == 1 {
        u64::from_be_bytes(head[4..12].try_into().ok()?)
    } else {
        u64::from(u32::from_be_bytes(head[4..8].try_into().ok()?))
    };
    const MAC_TO_UNIX: u64 = 2_082_844_800;
    let unix = i64::try_from(seconds.checked_sub(MAC_TO_UNIX)?).ok()?;
    let utc = chrono::DateTime::from_timestamp(unix, 0)?;
    let local = utc.with_timezone(&chrono::Local).naive_local();
    plausible(local).then_some(local)
}

/// The body (start, end) of the first box named `name` among the boxes
/// between `start` and `end`.
fn find_box(file: &mut std::fs::File, start: u64, end: u64, name: &[u8; 4]) -> Option<(u64, u64)> {
    let mut at = start;
    for _ in 0..4096 {
        if at + 8 > end {
            return None;
        }
        file.seek(SeekFrom::Start(at)).ok()?;
        let mut header = [0u8; 8];
        file.read_exact(&mut header).ok()?;
        let mut size = u64::from(u32::from_be_bytes(header[..4].try_into().ok()?));
        let mut body = at + 8;
        if size == 1 {
            let mut large = [0u8; 8];
            file.read_exact(&mut large).ok()?;
            size = u64::from_be_bytes(large);
            body += 8;
        } else if size == 0 {
            size = end - at;
        }
        if size < body - at || at + size > end {
            return None;
        }
        if &header[4..] == name {
            return Some((body, at + size));
        }
        at += size;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mp4(creation: u32) -> Vec<u8> {
        let mut mvhd = vec![0u8; 4];
        mvhd.extend(creation.to_be_bytes());
        mvhd.extend([0u8; 92]);
        let mut moov = ((8 + 8 + mvhd.len()) as u32).to_be_bytes().to_vec();
        moov.extend(b"moov");
        moov.extend(((8 + mvhd.len()) as u32).to_be_bytes());
        moov.extend(b"mvhd");
        moov.extend(mvhd);
        let mut file = 16u32.to_be_bytes().to_vec();
        file.extend(b"ftypisom");
        file.extend([0u8; 4]);
        file.extend(24u32.to_be_bytes());
        file.extend(b"mdat");
        file.extend([7u8; 16]);
        file.extend(moov);
        file
    }

    #[test]
    fn a_movie_header_gives_its_recorded_date() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("clip.mp4");
        // 2024-05-06 07:08:09 UTC as seconds since 1904.
        let unix = chrono::NaiveDate::from_ymd_opt(2024, 5, 6)
            .unwrap()
            .and_hms_opt(7, 8, 9)
            .unwrap()
            .and_utc()
            .timestamp() as u64;
        std::fs::write(&path, mp4((unix + 2_082_844_800) as u32)).unwrap();
        let (date, source) = embedded(&path).unwrap();
        assert_eq!(source, Source::Recorded);
        let expected = chrono::DateTime::from_timestamp(unix as i64, 0)
            .unwrap()
            .with_timezone(&chrono::Local)
            .naive_local();
        assert_eq!(date, expected);
        // An unset clock (zero) is not a date.
        std::fs::write(&path, mp4(0)).unwrap();
        assert!(embedded(&path).is_none());
        assert_eq!(captured(&path).unwrap().1, Source::Modified);
    }

    #[test]
    fn pictures_without_exif_fall_back_to_modified() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("IMG_0001.jpg");
        std::fs::write(&path, b"\xff\xd8\xff\xd9").unwrap();
        assert!(embedded(&path).is_none());
        assert_eq!(captured(&path).unwrap().1, Source::Modified);
    }
}
