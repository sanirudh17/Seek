//! Organize Folder: sort one folder's files into subfolders inside it, by
//! fixed rules (no model): by type (extension and location), by date taken
//! (Year\Month from EXIF or a video's recorded date, else modified), or by
//! name pattern (numbered camera sequences and versioned names). `propose` reads the
//! folder and changes nothing; `Operation::Organize` rechecks every item before
//! it moves, never overwrites, and records one journal job that Ctrl+Z reverses.
//! Folders, links, hidden and system files stay where they are.
//! Adapted from Monodrive's rules-based organize (MIT, see THIRD-PARTY-NOTICES).

use std::path::{Path, PathBuf};

/// The groups a folder can be sorted into, in review order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Rule {
    Screenshots,
    Images,
    Videos,
    Audio,
    Documents,
    Spreadsheets,
    Presentations,
    Archives,
    Installers,
}

impl Rule {
    /// The subfolder's name when the folder has none spelled differently.
    pub fn folder(self) -> &'static str {
        match self {
            Rule::Screenshots => "Screenshots",
            Rule::Images => "Images",
            Rule::Videos => "Videos",
            Rule::Audio => "Audio",
            Rule::Documents => "Documents",
            Rule::Spreadsheets => "Spreadsheets",
            Rule::Presentations => "Presentations",
            Rule::Archives => "Archives",
            Rule::Installers => "Installers",
        }
    }
    /// The rule in one line.
    pub fn describe(self) -> &'static str {
        match self {
            Rule::Screenshots => "Images named Screenshot, or taken from the Screenshots folder",
            Rule::Images => ".jpg, .png, .gif, .webp, .heic and other pictures",
            Rule::Videos => ".mp4, .mov, .mkv, .webm and other videos",
            Rule::Audio => ".mp3, .m4a, .wav, .flac and other audio",
            Rule::Documents => ".pdf, .docx, .txt, .md, .rtf and other documents",
            Rule::Spreadsheets => ".xlsx, .xls, .csv and .ods",
            Rule::Presentations => ".pptx, .ppt, .key and .odp",
            Rule::Archives => ".zip, .7z, .rar, .tar, .gz and other archives",
            Rule::Installers => ".exe, .msi, .msix and .appx",
        }
    }
    fn of_extension(extension: &str) -> Option<Rule> {
        Some(match extension {
            "jpg" | "jpeg" | "png" | "gif" | "webp" | "heic" | "heif" | "tif" | "tiff" | "bmp"
            | "svg" | "avif" | "ico" | "raw" | "cr2" | "nef" | "arw" | "dng" | "jfif" => {
                Rule::Images
            }
            "mp4" | "mov" | "m4v" | "mkv" | "webm" | "avi" | "wmv" | "mpg" | "mpeg" | "3gp" => {
                Rule::Videos
            }
            "mp3" | "m4a" | "aac" | "wav" | "flac" | "aiff" | "ogg" | "opus" | "wma" => Rule::Audio,
            "pdf" | "doc" | "docx" | "rtf" | "txt" | "md" | "odt" | "epub" | "pages" | "xps" => {
                Rule::Documents
            }
            "xls" | "xlsx" | "xlsm" | "csv" | "tsv" | "ods" | "numbers" => Rule::Spreadsheets,
            "ppt" | "pptx" | "key" | "odp" => Rule::Presentations,
            "zip" | "7z" | "rar" | "tar" | "gz" | "tgz" | "bz2" | "xz" | "zst" | "cab" | "iso" => {
                Rule::Archives
            }
            "exe" | "msi" | "msix" | "msixbundle" | "appx" | "appxbundle" => Rule::Installers,
            _ => return None,
        })
    }
}

/// How a folder is sorted.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Mode {
    /// Documents, Images, Videos… from extensions.
    #[default]
    Kind,
    /// Year\Month folders from when photos and videos were taken.
    Date,
    /// Camera sequences (IMG_, DSC_, Screenshot) and versions of one name.
    Names,
}

/// Items going to one subfolder of the organized folder.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Group {
    /// The type rule, when sorting by type.
    pub rule: Option<Rule>,
    /// The destination, relative to the folder: "Documents", or a year and
    /// month "2024\\05 May". Spelled as an existing folder spells it.
    pub name: String,
    /// What the group holds and how it was decided, for the review.
    pub detail: String,
    pub items: Vec<Item>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Item {
    pub path: PathBuf,
    /// Size and modified time when the plan was made; a changed file is skipped.
    pub stamp: (u64, u64),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Proposal {
    pub folder: PathBuf,
    pub groups: Vec<Group>,
    /// Files no rule placed; they stay where they are.
    pub unplaced: usize,
}

impl Proposal {
    #[cfg(test)]
    pub fn files(&self) -> usize {
        self.groups.iter().map(|group| group.items.len()).sum()
    }
}

const HIDDEN: u32 = 0x2;
const SYSTEM: u32 = 0x4;
const REPARSE: u32 = 0x400;

pub fn stamp(meta: &std::fs::Metadata) -> (u64, u64) {
    use std::os::windows::fs::MetadataExt;
    (meta.file_size(), meta.last_write_time())
}

/// Whether `name` in `folder` is a screenshot: Windows and most tools name them
/// "Screenshot ...", and the Pictures\Screenshots folder holds nothing else.
pub fn is_screenshot(folder: &Path, name: &str) -> bool {
    let lower = name.to_lowercase();
    lower.starts_with("screenshot") || lower.starts_with("screen shot") || in_screenshots(folder)
}

fn in_screenshots(folder: &Path) -> bool {
    let mut parts = folder
        .components()
        .rev()
        .filter_map(|part| part.as_os_str().to_str().map(str::to_lowercase));
    parts.next().as_deref() == Some("screenshots") && parts.next().as_deref() == Some("pictures")
}

/// What organizing `folder` by type would do.
#[cfg(test)]
pub fn propose(folder: &Path) -> Result<Proposal, String> {
    propose_by(folder, Mode::Kind)
}

/// What organizing `folder` would do. Reads one directory listing; by date,
/// also each photo's or video's header. Changes nothing.
pub fn propose_by(folder: &Path, mode: Mode) -> Result<Proposal, String> {
    let listing = list(folder)?;
    match mode {
        Mode::Kind => Ok(by_kind(folder, listing)),
        Mode::Date => Ok(by_date(folder, listing)),
        Mode::Names => Ok(by_names(folder, listing)),
    }
}

struct Listing {
    /// Subfolders, by lowercase name, as they are spelled.
    existing: std::collections::HashMap<String, String>,
    /// Names no group folder can take: files and links already using them.
    blocked: std::collections::HashSet<String>,
    files: Vec<(String, PathBuf, (u64, u64))>,
}

impl Listing {
    /// `wanted` spelled as an existing folder spells it, or None when a file
    /// or link already uses the name.
    fn destination(&self, wanted: &str) -> Option<String> {
        let lower = wanted.to_lowercase();
        if self.blocked.contains(&lower) || !crate::file_ops::valid_name(wanted) {
            return None;
        }
        Some(self.existing.get(&lower).cloned().unwrap_or_else(|| wanted.to_string()))
    }
}

fn list(folder: &Path) -> Result<Listing, String> {
    use std::os::windows::fs::MetadataExt;
    let listed = std::fs::read_dir(folder).map_err(|e| format!("Cannot read this folder: {e}"))?;
    let mut existing = std::collections::HashMap::<String, String>::new();
    let mut blocked = std::collections::HashSet::<String>::new();
    let mut files = vec![];
    for entry in listed.flatten() {
        let Ok(meta) = entry.metadata() else {
            continue;
        };
        let name = entry.file_name().to_string_lossy().into_owned();
        let attributes = meta.file_attributes();
        if meta.is_dir() && attributes & REPARSE == 0 {
            existing.insert(name.to_lowercase(), name);
        } else if !meta.is_file() || attributes & REPARSE != 0 {
            // A link or junction named like a group cannot receive files.
            blocked.insert(name.to_lowercase());
        } else {
            // Any file already using a group's name keeps that group out.
            blocked.insert(name.to_lowercase());
            if attributes & (HIDDEN | SYSTEM) == 0 {
                files.push((name, entry.path(), stamp(&meta)));
            }
        }
    }
    files.sort_by_key(|file| file.0.to_lowercase());
    Ok(Listing {
        existing,
        blocked,
        files,
    })
}

fn by_kind(folder: &Path, listing: Listing) -> Proposal {
    let screenshots_folder = in_screenshots(folder);
    let mut groups: Vec<Group> = vec![];
    let mut unplaced = 0;
    for (name, path, stamp) in &listing.files {
        let (name, path, stamp) = (name.clone(), path.clone(), *stamp);
        let extension = Path::new(&name)
            .extension()
            .and_then(|e| e.to_str())
            .map(str::to_lowercase)
            .unwrap_or_default();
        let kind = Rule::of_extension(&extension);
        let rule = if kind == Some(Rule::Images) && is_screenshot(folder, &name) {
            // The Screenshots folder is already where screenshots belong.
            (!screenshots_folder).then_some(Rule::Screenshots)
        } else {
            kind
        };
        let Some(rule) = rule else {
            unplaced += 1;
            continue;
        };
        let Some(destination) = listing.destination(rule.folder()) else {
            unplaced += 1;
            continue;
        };
        let item = Item { path, stamp };
        match groups.iter_mut().find(|group| group.rule == Some(rule)) {
            Some(group) => group.items.push(item),
            None => groups.push(Group {
                rule: Some(rule),
                name: destination,
                detail: rule.describe().into(),
                items: vec![item],
            }),
        }
    }
    groups.sort_by_key(|group| group.rule);
    Proposal {
        folder: folder.to_path_buf(),
        groups,
        unplaced,
    }
}

const MONTHS: [&str; 12] = [
    "January", "February", "March", "April", "May", "June", "July", "August", "September",
    "October", "November", "December",
];

/// Photos and videos into Year\Month ("2024\05 May") from the date inside
/// the file, else the modified date; each group says which it used.
fn by_date(folder: &Path, listing: Listing) -> Proposal {
    use chrono::Datelike;
    use crate::media_date::Source;
    let mut buckets: std::collections::BTreeMap<(i32, u32), Vec<(Item, Source)>> =
        Default::default();
    let mut unplaced = 0;
    for (_, path, stamp) in &listing.files {
        if !(crate::media_date::is_photo(path) || crate::media_date::is_video(path)) {
            unplaced += 1;
            continue;
        }
        let Some((date, source)) = crate::media_date::captured(path) else {
            unplaced += 1;
            continue;
        };
        buckets
            .entry((date.year(), date.month()))
            .or_default()
            .push((Item { path: path.clone(), stamp: *stamp }, source));
    }
    let mut groups = vec![];
    for ((year, month), items) in buckets.into_iter().rev() {
        let year_name = year.to_string();
        // The year folder: as an existing one spells it; a file named "2024"
        // keeps the whole year where it is.
        let Some(year_folder) = listing.destination(&year_name) else {
            unplaced += items.len();
            continue;
        };
        let month_name = format!("{month:02} {}", MONTHS[month as usize - 1]);
        let mut counts = std::collections::BTreeMap::<Source, usize>::new();
        for (_, source) in &items {
            *counts.entry(*source).or_default() += 1;
        }
        let detail = counts
            .iter()
            .map(|(source, n)| format!("{n} by {}", source.label()))
            .collect::<Vec<_>>()
            .join(" · ");
        groups.push(Group {
            rule: None,
            name: format!("{year_folder}\\{month_name}"),
            detail,
            items: items.into_iter().map(|(item, _)| item).collect(),
        });
    }
    Proposal {
        folder: folder.to_path_buf(),
        groups,
        unplaced,
    }
}

/// A numbered camera or screen-capture sequence's folder: "IMG_0042.jpg",
/// "DSC01234.JPG", "PXL_2024….jpg", "Screenshot 2024-….png".
fn sequence(name: &str) -> Option<&'static str> {
    let lower = name.to_lowercase();
    const FAMILIES: [(&str, &str); 9] = [
        ("screenshot", "Screenshots"),
        ("screen shot", "Screenshots"),
        ("img_", "Camera IMG"),
        ("img-", "Camera IMG"),
        ("dsc", "Camera DSC"),
        ("pxl_", "Camera PXL"),
        ("vid_", "Camera videos"),
        ("vid-", "Camera videos"),
        ("gopr", "GoPro"),
    ];
    let digits = lower.chars().filter(char::is_ascii_digit).count();
    FAMILIES
        .iter()
        .find(|(prefix, _)| lower.starts_with(prefix))
        .filter(|_| digits >= 3)
        .map(|(_, folder)| *folder)
}

/// A name without its version marks: "report v2", "Report_final",
/// "report (1)", "report - Copy", "report rev3" all read "report".
/// Returns the base and whether any mark was removed.
fn version_base(stem: &str) -> (String, bool) {
    let words: Vec<&str> = stem
        .split(|c: char| c.is_whitespace() || c == '_' || c == '-' || c == '.')
        .filter(|w| !w.is_empty())
        .collect();
    let is_mark = |w: &str| {
        let lower = w.trim_matches(['(', ')', '[', ']']).to_lowercase();
        let numbered = |prefix: &str| {
            lower
                .strip_prefix(prefix)
                .is_some_and(|n| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit() || c == '.'))
        };
        matches!(lower.as_str(), "final" | "draft" | "copy" | "latest" | "old" | "new" | "edited" | "revised")
            || numbered("v")
            || numbered("ver")
            || numbered("version")
            || numbered("rev")
            || numbered("r")
            || (w.starts_with('(') && w.ends_with(')') && lower.chars().all(|c| c.is_ascii_digit()))
    };
    let kept: Vec<&str> = words.iter().copied().filter(|w| !is_mark(w)).collect();
    let marked = kept.len() < words.len();
    (kept.join(" "), marked)
}

/// Camera sequences and versioned names, each as its own group.
fn by_names(folder: &Path, listing: Listing) -> Proposal {
    let mut sequences: std::collections::BTreeMap<&'static str, Vec<Item>> = Default::default();
    let mut versions: std::collections::BTreeMap<String, (String, Vec<Item>, bool)> =
        Default::default();
    let in_sequence_folder = |family: &str| {
        folder
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n.eq_ignore_ascii_case(family))
    };
    for (name, path, stamp) in &listing.files {
        let item = Item { path: path.clone(), stamp: *stamp };
        if let Some(family) = sequence(name) {
            if !in_sequence_folder(family) {
                sequences.entry(family).or_default().push(item);
            }
            continue;
        }
        let stem = Path::new(name)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or(name);
        let (base, marked) = version_base(stem);
        if base.chars().filter(|c| c.is_alphabetic()).count() < 3 {
            continue;
        }
        let entry = versions
            .entry(base.to_lowercase())
            .or_insert_with(|| (base.clone(), vec![], false));
        entry.1.push(item);
        entry.2 |= marked;
    }
    let mut groups = vec![];
    for (family, items) in sequences {
        // A sequence is worth a folder from three pictures.
        if items.len() < 3 {
            continue;
        }
        let Some(name) = listing.destination(family) else {
            continue;
        };
        groups.push(Group {
            rule: None,
            name,
            detail: format!("A numbered {} sequence", if family == "Screenshots" { "screenshot" } else { "camera" }),
            items,
        });
    }
    for (_, (base, items, marked)) in versions {
        // Versions of one document: two or more files sharing a name once
        // marks like v2, final and (1) are set aside.
        if items.len() < 2 || !marked {
            continue;
        }
        let Some(name) = listing.destination(&base) else {
            continue;
        };
        groups.push(Group {
            rule: None,
            name,
            detail: "Versions of one name (v1, v2, final, copy…)".into(),
            items,
        });
    }
    let placed: usize = groups.iter().map(|g| g.items.len()).sum();
    Proposal {
        folder: folder.to_path_buf(),
        unplaced: listing.files.len() - placed,
        groups,
    }
}

/// A free name for `name` inside `folder`, keeping both files on a clash:
/// "report.pdf", then "report (2).pdf", "report (3).pdf", ...
pub fn keep_both(folder: &Path, name: &str, taken: &dyn Fn(&Path) -> bool) -> Option<PathBuf> {
    let first = folder.join(name);
    if !taken(&first) {
        return Some(first);
    }
    let (stem, extension) = match name.rfind('.') {
        Some(dot) if dot > 0 => (&name[..dot], &name[dot..]),
        _ => (name, ""),
    };
    (2..10_000).find_map(|n| {
        let candidate = format!("{stem} ({n}){extension}");
        let path = folder.join(&candidate);
        (crate::file_ops::valid_name(&candidate) && !taken(&path)).then_some(path)
    })
}

/// Whether anything exists at `path`, without following links.
pub fn exists(path: &Path) -> bool {
    !matches!(std::fs::symlink_metadata(path), Err(e) if e.kind() == std::io::ErrorKind::NotFound)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn folder() -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "seek-organize-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).unwrap();
        root
    }

    #[test]
    fn groups_by_kind_and_recognises_screenshots() {
        let root = folder();
        for name in [
            "Screenshot 2026-11-01 101010.png",
            "photo.JPG",
            "scan0012.pdf",
            "notes.md",
            "budget.xlsx",
            "talk.pptx",
            "bundle.zip",
            "setup.exe",
            "clip.mp4",
            "song.mp3",
            "mystery.bin",
            "README",
        ] {
            std::fs::write(root.join(name), b"x").unwrap();
        }
        std::fs::create_dir(root.join("documents")).unwrap();
        let plan = propose(&root).unwrap();
        let names = |rule| {
            plan.groups
                .iter()
                .find(|g| g.rule == Some(rule))
                .map(|g| {
                    g.items
                        .iter()
                        .map(|i| i.path.file_name().unwrap().to_string_lossy().into_owned())
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default()
        };
        assert_eq!(
            names(Rule::Screenshots),
            ["Screenshot 2026-11-01 101010.png"]
        );
        assert_eq!(names(Rule::Images), ["photo.JPG"]);
        assert_eq!(names(Rule::Documents), ["notes.md", "scan0012.pdf"]);
        // An existing folder keeps its own spelling.
        assert_eq!(
            plan.groups
                .iter()
                .find(|g| g.rule == Some(Rule::Documents))
                .unwrap()
                .name,
            "documents"
        );
        assert_eq!(plan.unplaced, 2);
        assert_eq!(plan.files(), 10);
        // Folders are never moved.
        assert!(
            plan.groups
                .iter()
                .all(|g| g.items.iter().all(|i| i.path.is_file()))
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn screenshots_folder_is_already_sorted_and_files_named_like_groups_block_them() {
        let root = folder();
        let shots = root.join("Pictures").join("Screenshots");
        std::fs::create_dir_all(&shots).unwrap();
        std::fs::write(shots.join("Screenshot 1.png"), b"x").unwrap();
        std::fs::write(shots.join("capture.png"), b"x").unwrap();
        let plan = propose(&shots).unwrap();
        assert!(plan.groups.is_empty());
        assert_eq!(plan.unplaced, 2);
        // A file named "Images" means no Images folder can be made.
        std::fs::write(root.join("Images"), b"x").unwrap();
        std::fs::write(root.join("a.png"), b"x").unwrap();
        let plan = propose(&root).unwrap();
        assert!(plan.groups.iter().all(|g| g.rule != Some(Rule::Images)));
        assert_eq!(plan.unplaced, 2);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn by_name_finds_camera_sequences_and_versions() {
        let root = folder();
        for name in [
            "IMG_0001.jpg",
            "IMG_0002.jpg",
            "IMG_0003.jpg",
            "DSC0001.jpg",
            "Budget v1.xlsx",
            "Budget v2.xlsx",
            "Budget final.xlsx",
            "notes.txt",
            "notes (1).txt",
            "letter.docx",
        ] {
            std::fs::write(root.join(name), b"x").unwrap();
        }
        let plan = propose_by(&root, Mode::Names).unwrap();
        let group = |name: &str| {
            plan.groups
                .iter()
                .find(|g| g.name == name)
                .map(|g| g.items.len())
        };
        assert_eq!(group("Camera IMG"), Some(3));
        // One DSC picture is no sequence.
        assert_eq!(group("Camera DSC"), None);
        assert_eq!(group("Budget"), Some(3));
        assert_eq!(group("notes"), Some(2));
        assert_eq!(plan.unplaced, 2);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn by_date_uses_year_and_month_and_says_which_date() {
        let root = folder();
        std::fs::write(root.join("a.jpg"), b"\xff\xd8\xff\xd9").unwrap();
        std::fs::write(root.join("notes.txt"), b"x").unwrap();
        let plan = propose_by(&root, Mode::Date).unwrap();
        assert_eq!(plan.groups.len(), 1);
        let group = &plan.groups[0];
        let now = chrono::Local::now();
        use chrono::Datelike;
        assert_eq!(
            group.name,
            format!("{}\\{:02} {}", now.year(), now.month(), MONTHS[now.month() as usize - 1])
        );
        assert_eq!(group.detail, "1 by date modified");
        assert_eq!(plan.unplaced, 1);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn version_marks_are_set_aside() {
        assert_eq!(version_base("Report v2"), ("Report".into(), true));
        assert_eq!(version_base("report_final"), ("report".into(), true));
        assert_eq!(version_base("report - Copy"), ("report".into(), true));
        assert_eq!(version_base("Plan rev3"), ("Plan".into(), true));
        assert_eq!(version_base("Plan"), ("Plan".into(), false));
    }

    #[test]
    fn keep_both_never_reuses_a_taken_name() {
        let root = folder();
        std::fs::write(root.join("a.pdf"), b"x").unwrap();
        std::fs::write(root.join("a (2).pdf"), b"x").unwrap();
        let free = keep_both(&root, "a.pdf", &|p| exists(p)).unwrap();
        assert_eq!(free.file_name().unwrap(), "a (3).pdf");
        let free = keep_both(&root, ".env", &|p| exists(p)).unwrap();
        assert_eq!(free.file_name().unwrap(), ".env");
        std::fs::write(root.join(".env"), b"x").unwrap();
        let free = keep_both(&root, ".env", &|p| exists(p)).unwrap();
        assert_eq!(free.file_name().unwrap(), ".env (2)");
        std::fs::remove_dir_all(root).unwrap();
    }
}
