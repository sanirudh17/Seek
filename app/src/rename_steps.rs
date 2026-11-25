//! Advanced rename (Phase 8.2): a stack of steps applied in order to each
//! name, with variables and saved presets. Pure planning only: the result is
//! a list of names that `batch_rename::preview` validates (Windows rules,
//! conflicts, existing files) and the guarded batch rename applies as one
//! undoable operation.
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "action")]
pub enum Step {
    /// Replace every match of `find` (text, or a regular expression).
    Replace {
        find: String,
        with: String,
        regex: bool,
    },
    /// Add text at the start or the end.
    Append { text: String, at_start: bool },
    /// Write `text` over the characters from position `at` (0-based).
    Overwrite { text: String, at: usize },
    /// Remove `count` characters from position `at` (0-based).
    Remove { at: usize, count: usize },
    /// Remove the last `count` characters.
    RemoveLast { count: usize },
    /// Replace the whole name with a template, such as `Photo $INDEX`.
    NewName { template: String },
    CapitalizeFirst,
    CapitalizeWords,
    Uppercase,
    Lowercase,
    /// Remove every occurrence of `text`.
    RemoveText { text: String },
}

impl Step {
    pub const KINDS: [&'static str; 11] = [
        "Replace",
        "Append",
        "Overwrite",
        "Remove",
        "Remove Last",
        "New Name",
        "Capitalize First",
        "Capitalize Words",
        "Uppercase",
        "Lowercase",
        "Remove Text",
    ];
    pub fn of_kind(kind: usize) -> Step {
        match kind {
            0 => Step::Replace {
                find: String::new(),
                with: String::new(),
                regex: false,
            },
            1 => Step::Append {
                text: String::new(),
                at_start: false,
            },
            2 => Step::Overwrite {
                text: String::new(),
                at: 0,
            },
            3 => Step::Remove { at: 0, count: 1 },
            4 => Step::RemoveLast { count: 1 },
            5 => Step::NewName {
                template: "$NAME".into(),
            },
            6 => Step::CapitalizeFirst,
            7 => Step::CapitalizeWords,
            8 => Step::Uppercase,
            9 => Step::Lowercase,
            _ => Step::RemoveText {
                text: String::new(),
            },
        }
    }
    pub fn kind(&self) -> usize {
        match self {
            Step::Replace { .. } => 0,
            Step::Append { .. } => 1,
            Step::Overwrite { .. } => 2,
            Step::Remove { .. } => 3,
            Step::RemoveLast { .. } => 4,
            Step::NewName { .. } => 5,
            Step::CapitalizeFirst => 6,
            Step::CapitalizeWords => 7,
            Step::Uppercase => 8,
            Step::Lowercase => 9,
            Step::RemoveText { .. } => 10,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Options {
    pub case_sensitive: bool,
    /// Steps see and change the extension too.
    pub include_extension: bool,
    /// Add " (2)", " (3)"… instead of reporting a name conflict.
    pub auto_increment: bool,
    /// $INDEX starts here.
    pub index_start: u64,
    /// Count from the last item (tail) instead of the first (head).
    pub index_from_tail: bool,
    /// Minimum digits for $INDEX, padded with zeros.
    pub index_digits: u8,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            case_sensitive: false,
            include_extension: false,
            auto_increment: false,
            index_start: 1,
            index_from_tail: false,
            index_digits: 1,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Preset {
    pub name: String,
    pub steps: Vec<Step>,
    pub options: Options,
}

/// The variables a template may use, with what each becomes.
pub const VARIABLES: &[(&str, &str)] = &[
    ("$NAME", "Original name, without extension"),
    ("$EXT", "Extension, without the dot"),
    ("$INDEX", "Position in the list"),
    ("$CRT_DATE", "Date created"),
    ("$MOD_DATE", "Date modified"),
    ("$CUR_DATE", "Today's date"),
    ("$CUR_DTIME", "Date and time now"),
    ("$UUID", "A new unique id"),
    ("$WIDTH", "Image width in pixels"),
    ("$HEIGHT", "Image height in pixels"),
    ("$$", "A dollar sign"),
];

/// What the variables read for one file. Built off the interface thread.
#[derive(Clone, Debug, Default)]
pub struct Facts {
    pub stem: String,
    pub extension: Option<String>,
    pub index: u64,
    pub created: Option<chrono::DateTime<chrono::Local>>,
    pub modified: Option<chrono::DateTime<chrono::Local>>,
    pub now: Option<chrono::DateTime<chrono::Local>>,
    pub dimensions: Option<(u32, u32)>,
}

impl Facts {
    /// Reads metadata and, for images only, the header's size. Never reads a
    /// cloud placeholder's contents.
    pub fn read(path: &Path, index: u64, now: chrono::DateTime<chrono::Local>) -> Facts {
        let name = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        let folder = std::fs::symlink_metadata(path).is_ok_and(|m| m.is_dir());
        let (stem, extension) = split(&name, folder);
        let metadata = std::fs::symlink_metadata(path).ok();
        let local = |time: std::io::Result<std::time::SystemTime>| {
            time.ok().map(chrono::DateTime::<chrono::Local>::from)
        };
        let placeholder = metadata.as_ref().is_some_and(|m| {
            use std::os::windows::fs::MetadataExt;
            m.file_attributes() & (0x400000 | 0x40000 | 0x1000) != 0
        });
        let image = !folder
            && extension.is_some_and(|ext| {
                matches!(
                    ext.to_ascii_lowercase().as_str(),
                    "png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp"
                )
            });
        Facts {
            stem: stem.into(),
            extension: extension.map(str::to_string),
            index,
            created: metadata.as_ref().and_then(|m| local(m.created())),
            modified: metadata.as_ref().and_then(|m| local(m.modified())),
            now: Some(now),
            dimensions: (image && !placeholder)
                .then(|| image::image_dimensions(path).ok())
                .flatten(),
        }
    }
}

/// `report.final.pdf` → (`report.final`, `pdf`). Folders and dot-files keep
/// their whole name.
pub fn split(name: &str, folder: bool) -> (&str, Option<&str>) {
    if folder {
        return (name, None);
    }
    match name.rfind('.') {
        Some(dot) if dot > 0 && dot + 1 < name.len() => (&name[..dot], Some(&name[dot + 1..])),
        _ => (name, None),
    }
}

/// .NET-style date patterns: yyyy, yy, MM, dd, HH, mm, ss; other text stays.
fn format_date(date: &chrono::DateTime<chrono::Local>, pattern: &str) -> String {
    use chrono::{Datelike, Timelike};
    let mut out = String::new();
    let mut rest = pattern;
    while !rest.is_empty() {
        let token = ["yyyy", "yy", "MM", "dd", "HH", "mm", "ss"]
            .into_iter()
            .find(|token| rest.starts_with(token));
        match token {
            Some("yyyy") => out.push_str(&format!("{:04}", date.year())),
            Some("yy") => out.push_str(&format!("{:02}", date.year() % 100)),
            Some("MM") => out.push_str(&format!("{:02}", date.month())),
            Some("dd") => out.push_str(&format!("{:02}", date.day())),
            Some("HH") => out.push_str(&format!("{:02}", date.hour())),
            Some("mm") => out.push_str(&format!("{:02}", date.minute())),
            Some("ss") => out.push_str(&format!("{:02}", date.second())),
            _ => {
                let c = rest.chars().next().unwrap();
                out.push(c);
                rest = &rest[c.len_utf8()..];
                continue;
            }
        }
        rest = &rest[token.unwrap().len()..];
    }
    out
}

fn uuid() -> String {
    let guid = unsafe { windows::Win32::System::Com::CoCreateGuid() }.unwrap_or_default();
    format!(
        "{:08x}-{:04x}-{:04x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        guid.data1,
        guid.data2,
        guid.data3,
        guid.data4[0],
        guid.data4[1],
        guid.data4[2],
        guid.data4[3],
        guid.data4[4],
        guid.data4[5],
        guid.data4[6],
        guid.data4[7]
    )
}

/// Expands the variables in `template`. Unknown `$WORDS` stay as written.
pub fn expand(template: &str, facts: &Facts, options: &Options) -> String {
    let mut out = String::new();
    let mut rest = template;
    while let Some(dollar) = rest.find('$') {
        out.push_str(&rest[..dollar]);
        rest = &rest[dollar..];
        if let Some(after) = rest.strip_prefix("$$") {
            out.push('$');
            rest = after;
            continue;
        }
        let word: String = rest[1..]
            .chars()
            .take_while(|c| c.is_ascii_uppercase() || *c == '_')
            .collect();
        let mut used = 1 + word.len();
        // An optional {format} right after a date.
        let format = rest[used..].strip_prefix('{').and_then(|inner| {
            inner.find('}').map(|end| {
                used += end + 2;
                &inner[..end]
            })
        });
        let date = |date: Option<chrono::DateTime<chrono::Local>>, default: &str| {
            date.map(|d| format_date(&d, format.unwrap_or(default)))
                .unwrap_or_default()
        };
        let value = match word.as_str() {
            "NAME" => Some(facts.stem.clone()),
            "EXT" => Some(facts.extension.clone().unwrap_or_default()),
            "INDEX" => Some(format!(
                "{:0width$}",
                facts.index,
                width = options.index_digits.clamp(1, 9) as usize
            )),
            "CRT_DATE" => Some(date(facts.created, "yyyy-MM-dd")),
            "MOD_DATE" => Some(date(facts.modified, "yyyy-MM-dd")),
            "CUR_DATE" => Some(date(facts.now, "yyyy-MM-dd")),
            "CUR_DTIME" => Some(date(facts.now, "yyyy-MM-dd HH.mm.ss")),
            "UUID" => Some(uuid()),
            "WIDTH" => Some(facts.dimensions.map(|d| d.0.to_string()).unwrap_or_default()),
            "HEIGHT" => Some(facts.dimensions.map(|d| d.1.to_string()).unwrap_or_default()),
            _ => None,
        };
        match value {
            Some(value) => {
                out.push_str(&value);
                rest = &rest[used..];
            }
            None => {
                out.push('$');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

fn chars_at(text: &str, at: usize) -> usize {
    text.char_indices().nth(at).map_or(text.len(), |(i, _)| i)
}

fn find_ci(haystack: &str, needle: &str) -> Vec<(usize, usize)> {
    // Case-insensitive matches by char positions on a lowercase copy whose
    // characters line up with the original (one-to-one lowercase only).
    let lower: Vec<char> = haystack.chars().flat_map(char::to_lowercase).collect();
    let original: Vec<(usize, char)> = haystack.char_indices().collect();
    let needle: Vec<char> = needle.chars().flat_map(char::to_lowercase).collect();
    if needle.is_empty() || lower.len() != original.len() {
        return haystack
            .match_indices(&needle.iter().collect::<String>())
            .map(|(i, m)| (i, i + m.len()))
            .collect();
    }
    let mut found = vec![];
    let mut i = 0;
    while i + needle.len() <= lower.len() {
        if lower[i..i + needle.len()] == needle[..] {
            let start = original[i].0;
            let end = original
                .get(i + needle.len())
                .map_or(haystack.len(), |(byte, _)| *byte);
            found.push((start, end));
            i += needle.len();
        } else {
            i += 1;
        }
    }
    found
}

fn replace_text(text: &str, find: &str, with: &str, case_sensitive: bool) -> String {
    if find.is_empty() {
        return text.into();
    }
    if case_sensitive {
        return text.replace(find, with);
    }
    let mut out = String::new();
    let mut last = 0;
    for (start, end) in find_ci(text, find) {
        out.push_str(&text[last..start]);
        out.push_str(with);
        last = end;
    }
    out.push_str(&text[last..]);
    out
}

/// Applies one step. Errors are for the step's own input (a bad regex).
pub fn apply(step: &Step, text: &str, facts: &Facts, options: &Options) -> Result<String, String> {
    Ok(match step {
        Step::Replace { find, with, regex } => {
            let with = expand(with, facts, options);
            if *regex {
                if find.is_empty() {
                    return Ok(text.into());
                }
                let pattern = regex::RegexBuilder::new(find)
                    .case_insensitive(!options.case_sensitive)
                    .size_limit(1 << 20)
                    .build()
                    .map_err(|_| "This regular expression is not valid".to_string())?;
                pattern.replace_all(text, with.as_str()).into_owned()
            } else {
                replace_text(text, find, &with, options.case_sensitive)
            }
        }
        Step::Append { text: add, at_start } => {
            let add = expand(add, facts, options);
            if *at_start {
                format!("{add}{text}")
            } else {
                format!("{text}{add}")
            }
        }
        Step::Overwrite { text: over, at } => {
            let over = expand(over, facts, options);
            let start = chars_at(text, *at);
            let end = chars_at(text, at + over.chars().count());
            let pad = at.saturating_sub(text.chars().count());
            format!("{}{}{over}{}", &text[..start], " ".repeat(pad), &text[end..])
        }
        Step::Remove { at, count } => {
            let start = chars_at(text, *at);
            let end = chars_at(text, at.saturating_add(*count));
            format!("{}{}", &text[..start], &text[end..])
        }
        Step::RemoveLast { count } => {
            let keep = text.chars().count().saturating_sub(*count);
            text[..chars_at(text, keep)].into()
        }
        Step::NewName { template } => expand(template, facts, options),
        Step::CapitalizeFirst => {
            let mut chars = text.chars();
            match chars.next() {
                Some(first) => first.to_uppercase().chain(chars).collect(),
                None => String::new(),
            }
        }
        Step::CapitalizeWords => {
            let mut out = String::new();
            let mut start = true;
            for c in text.chars() {
                if start {
                    out.extend(c.to_uppercase());
                } else {
                    out.push(c);
                }
                start = c.is_whitespace() || matches!(c, '-' | '_' | '.' | '(' | '[');
            }
            out
        }
        Step::Uppercase => text.to_uppercase(),
        Step::Lowercase => text.to_lowercase(),
        Step::RemoveText { text: remove } => replace_text(text, remove, "", options.case_sensitive),
    })
}

/// The new name for one file: the steps in order on the name (or the stem,
/// keeping the extension, unless the options include it).
pub fn rename(steps: &[Step], facts: &Facts, options: &Options) -> Result<String, String> {
    let original = match &facts.extension {
        Some(ext) => format!("{}.{ext}", facts.stem),
        None => facts.stem.clone(),
    };
    let mut text = if options.include_extension {
        original
    } else {
        facts.stem.clone()
    };
    for step in steps {
        text = apply(step, &text, facts, options)?;
    }
    Ok(match (&facts.extension, options.include_extension) {
        (Some(ext), false) => format!("{text}.{ext}"),
        _ => text,
    })
}

/// "name (2).ext": the auto-increment form.
fn numbered(name: &str, n: u32) -> String {
    match name.rfind('.') {
        Some(dot) if dot > 0 => format!("{} ({n}){}", &name[..dot], &name[dot..]),
        _ => format!("{name} ({n})"),
    }
}

/// Names for every path, in order. `$INDEX` counts from the head or the
/// tail. With auto increment, a name taken by another item in the batch or
/// by an existing file that is not being renamed gets " (2)", " (3)"…
pub fn plan(
    paths: &[PathBuf],
    facts: &[Facts],
    steps: &[Step],
    options: &Options,
) -> Vec<Result<String, String>> {
    let mut names: Vec<Result<String, String>> =
        facts.iter().map(|facts| rename(steps, facts, options)).collect();
    if !options.auto_increment {
        return names;
    }
    let key = |path: &Path| path.to_string_lossy().to_lowercase();
    let sources: std::collections::HashSet<String> = paths.iter().map(|p| key(p)).collect();
    let mut taken = std::collections::HashSet::new();
    for (path, name) in paths.iter().zip(names.iter_mut()) {
        let Ok(name) = name else { continue };
        let free = |candidate: &str, taken: &std::collections::HashSet<String>| {
            let target = path.with_file_name(candidate);
            let k = key(&target);
            // The item itself (a case-only rename) is always free.
            (!taken.contains(&k))
                && (sources.contains(&k) || std::fs::symlink_metadata(&target).is_err())
        };
        if !free(name, &taken)
            && let Some(n) = (2..10_000).find(|n| free(&numbered(name, *n), &taken))
        {
            *name = numbered(name, n);
        }
        taken.insert(key(&path.with_file_name(&*name)));
    }
    names
}

/// Facts for every path; `$INDEX` from the head or the tail.
pub fn facts(paths: &[PathBuf], options: &Options) -> Vec<Facts> {
    let now = chrono::Local::now();
    let count = paths.len() as u64;
    paths
        .iter()
        .enumerate()
        .map(|(position, path)| {
            let offset = if options.index_from_tail {
                count - 1 - position as u64
            } else {
                position as u64
            };
            Facts::read(path, options.index_start.saturating_add(offset), now)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn facts(stem: &str, ext: Option<&str>) -> Facts {
        use chrono::TimeZone;
        let day = chrono::Local.with_ymd_and_hms(2026, 3, 9, 14, 5, 7).unwrap();
        Facts {
            stem: stem.into(),
            extension: ext.map(str::to_string),
            index: 7,
            created: Some(day),
            modified: Some(day),
            now: Some(day),
            dimensions: Some((1920, 1080)),
        }
    }

    #[test]
    fn steps_run_in_order_on_the_name_and_keep_the_extension() {
        let o = Options::default();
        let steps = vec![
            Step::Replace {
                find: "IMG".into(),
                with: "Photo".into(),
                regex: false,
            },
            Step::Append {
                text: " $INDEX".into(),
                at_start: false,
            },
        ];
        assert_eq!(
            rename(&steps, &facts("img_001", Some("JPG")), &o).unwrap(),
            "Photo_001 7.JPG",
            "case-insensitive by default; the extension is untouched"
        );
        let strict = Options {
            case_sensitive: true,
            ..o.clone()
        };
        assert_eq!(
            rename(&steps, &facts("img_001", Some("JPG")), &strict).unwrap(),
            "img_001 7.JPG"
        );
        let with_ext = Options {
            include_extension: true,
            ..o
        };
        assert_eq!(
            rename(&[Step::Uppercase], &facts("a", Some("txt")), &with_ext).unwrap(),
            "A.TXT"
        );
    }

    #[test]
    fn every_action_does_what_it_says() {
        let o = Options::default();
        let f = facts("hello world", None);
        let run = |step: Step| apply(&step, "hello world", &f, &o).unwrap();
        assert_eq!(run(Step::CapitalizeFirst), "Hello world");
        assert_eq!(run(Step::CapitalizeWords), "Hello World");
        assert_eq!(run(Step::Uppercase), "HELLO WORLD");
        assert_eq!(run(Step::Lowercase), "hello world");
        assert_eq!(run(Step::Remove { at: 5, count: 6 }), "hello");
        assert_eq!(run(Step::RemoveLast { count: 6 }), "hello");
        assert_eq!(run(Step::RemoveLast { count: 99 }), "");
        assert_eq!(
            run(Step::Overwrite {
                text: "J".into(),
                at: 0
            }),
            "Jello world"
        );
        assert_eq!(
            run(Step::RemoveText { text: "O".into() }),
            "hell wrld",
            "case-insensitive"
        );
        assert_eq!(
            run(Step::Append {
                text: "x-".into(),
                at_start: true
            }),
            "x-hello world"
        );
        assert_eq!(
            run(Step::NewName {
                template: "$NAME!".into()
            }),
            "hello world!"
        );
        assert_eq!(
            run(Step::Replace {
                find: r"(\w+) (\w+)".into(),
                with: "$2-$1".into(),
                regex: true
            }),
            "world-hello",
            "regex groups; unknown variables stay as written"
        );
        assert!(
            apply(
                &Step::Replace {
                    find: "(".into(),
                    with: String::new(),
                    regex: true
                },
                "a",
                &f,
                &o
            )
            .is_err()
        );
    }

    #[test]
    fn variables_expand_with_formats_and_a_literal_dollar() {
        let o = Options {
            index_digits: 3,
            ..Options::default()
        };
        let f = facts("scan", Some("png"));
        assert_eq!(expand("$NAME.$EXT", &f, &o), "scan.png");
        assert_eq!(expand("$INDEX", &f, &o), "007");
        assert_eq!(expand("$CUR_DATE", &f, &o), "2026-03-09");
        assert_eq!(expand("$MOD_DATE{dd.MM.yy}", &f, &o), "09.03.26");
        assert_eq!(expand("$CUR_DTIME", &f, &o), "2026-03-09 14.05.07");
        assert_eq!(expand("$WIDTHx$HEIGHT", &f, &o), "1920x1080");
        assert_eq!(expand("$$5 $$NAME", &f, &o), "$5 $NAME");
        assert_eq!(expand("$UUID", &f, &o).len(), 36);
        assert_eq!(expand("$NOPE", &f, &o), "$NOPE");
    }

    #[test]
    fn auto_increment_avoids_batch_and_disk_conflicts() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a.txt");
        let b = dir.path().join("b.txt");
        let c = dir.path().join("c.txt");
        for path in [&a, &b, &c] {
            std::fs::write(path, "x").unwrap();
        }
        std::fs::write(dir.path().join("same.txt"), "kept").unwrap();
        let paths = vec![a, b, c];
        let options = Options {
            auto_increment: true,
            ..Options::default()
        };
        let steps = vec![Step::NewName {
            template: "same".into(),
        }];
        let facts = super::facts(&paths, &options);
        let names: Vec<String> = plan(&paths, &facts, &steps, &options)
            .into_iter()
            .map(Result::unwrap)
            .collect();
        assert_eq!(names, ["same (2).txt", "same (3).txt", "same (4).txt"]);
        // Without it, the conflicts are left for the preview to report.
        let plain = Options::default();
        let names: Vec<String> = plan(&paths, &facts, &steps, &plain)
            .into_iter()
            .map(Result::unwrap)
            .collect();
        assert_eq!(names, ["same.txt", "same.txt", "same.txt"]);
    }

    #[test]
    fn index_counts_from_the_tail_and_presets_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let paths: Vec<PathBuf> = (0..3).map(|i| dir.path().join(format!("{i}.txt"))).collect();
        let options = Options {
            index_from_tail: true,
            index_start: 10,
            ..Options::default()
        };
        let indexes: Vec<u64> = super::facts(&paths, &options).iter().map(|f| f.index).collect();
        assert_eq!(indexes, [12, 11, 10]);
        let preset = Preset {
            name: "Photos".into(),
            steps: vec![Step::Uppercase, Step::of_kind(0)],
            options,
        };
        let json = serde_json::to_string(&preset).unwrap();
        assert_eq!(serde_json::from_str::<Preset>(&json).unwrap(), preset);
        for kind in 0..Step::KINDS.len() {
            assert_eq!(Step::of_kind(kind).kind(), kind);
        }
    }

    #[test]
    fn split_keeps_dot_files_and_folders_whole() {
        assert_eq!(split("a.tar.gz", false), ("a.tar", Some("gz")));
        assert_eq!(split(".gitignore", false), (".gitignore", None));
        assert_eq!(split("v1.2", true), ("v1.2", None));
        assert_eq!(split("trailing.", false), ("trailing.", None));
    }
}
