//! Suggest Names: names read from what files say about themselves, by fixed
//! rules only (no model). Every suggestion says where it came from and how
//! sure it is; the same file always gets the same suggestion.
//!
//! Sources, strongest first:
//! - a document's own title: PDF and Office metadata, HTML <title>, Markdown
//!   front matter;
//! - its title or first heading: Markdown `#`, Word Title/Heading 1, a
//!   presentation's first slide title;
//! - what kind of document it is, with its number and date ("Invoice 4471
//!   2026-03-12") when it opens like an invoice, receipt, statement…;
//! - music tags (artist and title);
//! - the date taken or recorded, for photos and videos still carrying a
//!   camera's name (IMG_1234, DSC_0042, PXL_…);
//! - an opening line that reads as a title (medium);
//! - a code file's leading comment or a table's column names (low: offered,
//!   never filled in).
//!
//! A suggestion goes through the same validation as any rename; a file's text
//! is never treated as anything but text.
//! Adapted from Monodrive's naming (MIT, see THIRD-PARTY-NOTICES).
use std::io::Read;
use std::path::Path;

/// How much of a text file is read; enough for a title, a document's kind,
/// its number and date.
const READ: u64 = 8192;
/// How much of a PDF is scanned for its title metadata, from each end.
const PDF_SCAN: u64 = 256 * 1024;
/// The most read from one Office part.
const OFFICE_PART: u64 = 4 * 1024 * 1024;
/// The longest a suggested name gets before its extension.
const LONGEST: usize = 60;
const TITLE_CHARS: std::ops::RangeInclusive<usize> = 3..=80;
const NOISE: [&str; 6] = ["final", "copy", "draft", "new", "untitled", "edited"];

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Confidence {
    /// A guess: shown beside the file, not filled in.
    Low,
    /// Read from the file's opening text.
    Medium,
    /// The file's own title, heading, tags, kind or capture date.
    High,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Suggestion {
    /// The full new name, extension kept.
    pub name: String,
    /// Where it came from, for the review list ("Document title").
    pub source: &'static str,
    pub confidence: Confidence,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Format {
    Markdown,
    Html,
    Pdf,
    Table,
    Text,
    Code,
    Word,
    Slides,
    Workbook,
    Music,
    Picture,
    Movie,
}

impl Format {
    fn of(extension: &str) -> Option<Self> {
        Some(match extension {
            "md" | "markdown" | "mdown" => Self::Markdown,
            "html" | "htm" | "xhtml" => Self::Html,
            "pdf" => Self::Pdf,
            "csv" | "tsv" => Self::Table,
            "txt" | "text" | "log" => Self::Text,
            "rs" | "py" | "js" | "jsx" | "ts" | "tsx" | "go" | "c" | "h" | "cc" | "cpp" | "cs"
            | "swift" | "java" | "kt" | "rb" | "sh" | "ps1" | "css" | "sql" | "lua" | "php" => {
                Self::Code
            }
            "docx" | "docm" => Self::Word,
            "pptx" | "pptm" => Self::Slides,
            "xlsx" | "xlsm" => Self::Workbook,
            "mp3" | "flac" => Self::Music,
            "jpg" | "jpeg" | "jfif" | "heic" | "heif" | "tif" | "tiff" | "dng" | "cr2" | "nef"
            | "arw" | "webp" | "png" => Self::Picture,
            "mp4" | "mov" | "m4v" | "3gp" => Self::Movie,
            _ => return None,
        })
    }
}

type Found = Option<(String, &'static str, Confidence)>;

/// A new name for `path`, keeping its extension, or why it keeps its name.
pub fn suggest(path: &Path) -> Result<Suggestion, String> {
    let meta = std::fs::symlink_metadata(path).map_err(|e| e.to_string())?;
    if !meta.is_file() {
        return Err("Only files get suggestions".into());
    }
    if crate::content::unavailable(path, &meta) {
        return Err("Not downloaded".into());
    }
    let extension = path
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or_default();
    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or_default();
    let format = Format::of(&extension.to_lowercase())
        .ok_or("No rule reads a name from this kind of file")?;
    let found: Found = match format {
        Format::Pdf => pdf(path, &meta)?,
        Format::Word | Format::Slides | Format::Workbook => office(path, format)?,
        Format::Music => music(path),
        Format::Picture | Format::Movie => {
            if !camera_name(stem) {
                return Err("Photos and videos are renamed only from a camera's name".into());
            }
            let (date, source) = crate::media_date::embedded(path)
                .ok_or("No date taken inside the file")?;
            let kind = if format == Format::Picture { "Photo" } else { "Video" };
            Some((
                format!("{kind} {}", date.format("%Y-%m-%d %H.%M.%S")),
                if source == crate::media_date::Source::Taken {
                    "Date taken"
                } else {
                    "Date recorded"
                },
                Confidence::High,
            ))
        }
        _ => from_text(&start_of(path)?, format),
    };
    let (title, source, mut confidence) = found.ok_or("Nothing in the file reads as a name")?;
    // Only a name that says nothing (untitled, scan0001, Document1, a camera
    // or download id) is replaced on its own; a name someone chose is kept,
    // and the suggestion is offered beside it. A title from the file can be
    // stale or a series name ("Intro to Statistics" on every lecture), so
    // it never overrides a person's choice.
    if !generic_name(stem) {
        confidence = Confidence::Low;
    }
    let new_stem = if format == Format::Picture || format == Format::Movie {
        Some(title)
    } else {
        // A title that is a file name ("AGENTS.md") loses the extension the
        // file already has.
        let title = title.trim();
        let title = match title.len().checked_sub(extension.len() + 1) {
            Some(cut) if !extension.is_empty()
                && title.is_char_boundary(cut)
                && title[cut..].eq_ignore_ascii_case(&format!(".{extension}")) => &title[..cut],
            _ => title,
        };
        clean(title)
    }
    .ok_or("Nothing in the file reads as a name")?;
    // The current name already says it, perhaps with more ("Budget final"
    // for a file titled "Budget"): a rename would only lose words.
    let current = comparable(&clean(stem).unwrap_or_else(|| stem.to_string()));
    if comparable(stem).contains(&comparable(&new_stem)) || current == comparable(&new_stem) {
        return Err("Already named after its contents".into());
    }
    let name = match extension {
        "" => new_stem,
        extension => format!("{new_stem}.{extension}"),
    };
    if !crate::file_ops::valid_name(&name) {
        return Err("No valid name".into());
    }
    Ok(Suggestion {
        name,
        source,
        confidence,
    })
}

/// Names that say nothing about the contents: what apps and scanners give a
/// file nobody named ("untitled", "New Text Document (2)", "scan0001",
/// "Document1", "download (3)"), or a name with almost no letters.
fn generic_name(stem: &str) -> bool {
    let lower = stem.to_lowercase();
    let letters: String = lower.chars().filter(|c| c.is_alphabetic()).collect();
    const GENERIC: [&str; 24] = [
        "untitled", "newtextdocument", "newdocument", "newmicrosoftworddocument", "document",
        "doc", "scan", "scanned", "file", "text", "notes", "note", "download", "image", "page", "output",
        "track", "audio", "recording", "newrecording", "presentation", "book", "export", "attachment",
    ];
    // A download or app id: 65bc5616-5141-42e2-ad17-b3ad3fb2895a.
    let hex = lower.chars().filter(|c| c.is_ascii_hexdigit()).count();
    let id = hex >= 16 && lower.chars().all(|c| c.is_ascii_hexdigit() || "-_ ".contains(c));
    letters.len() < 3 || GENERIC.contains(&letters.as_str()) || camera_name(stem) || id
}

/// Letters and digits only, lowercase: "Quarterly-plan" equals "quarterly plan".
fn comparable(text: &str) -> String {
    text.chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

/// Names cameras, phones and messengers give: IMG_1234, DSC01234, PXL_2024…,
/// VID-2024…, GOPR0042, WhatsApp Image 2024-…, or a bare run of digits.
fn camera_name(stem: &str) -> bool {
    let lower = stem.to_lowercase();
    let digits = lower.chars().filter(char::is_ascii_digit).count();
    const PREFIXES: [&str; 19] = [
        "img_", "img-", "img", "dsc", "dscn", "dscf", "dji_", "gopr", "gx0", "mvi_", "mov_",
        "pxl_", "vid_", "vid-", "vid", "sam_", "wp_", "whatsapp image", "whatsapp video",
    ];
    digits >= 3
        && (PREFIXES.iter().any(|prefix| lower.starts_with(prefix))
            || (digits >= 6 && lower.chars().all(|c| c.is_ascii_digit() || "_- ".contains(c))))
}

fn start_of(path: &Path) -> Result<String, String> {
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .and_then(|file| file.take(READ).read_to_end(&mut bytes))
        .map_err(|e| e.to_string())?;
    if bytes.is_empty() {
        return Err("Empty file".into());
    }
    // The read can end inside a character; cut it there so the text still
    // reads as UTF-8 instead of falling back to a legacy code page ("â€”").
    if let Err(error) = std::str::from_utf8(&bytes)
        && error.error_len().is_none()
        && error.valid_up_to() > 0
    {
        bytes.truncate(error.valid_up_to());
    }
    crate::text_decode::decode(&bytes, true).map_err(|_| "Not text".to_string())
}

/// A PDF's title metadata, then the text of its first lines: from the shared
/// text cache when this exact file was read before, otherwise the PDF filter.
fn pdf(path: &Path, meta: &std::fs::Metadata) -> Result<Found, String> {
    use std::io::{Seek, SeekFrom};
    let mut file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let length = meta.len();
    let mut head = Vec::new();
    (&mut file)
        .take(PDF_SCAN)
        .read_to_end(&mut head)
        .map_err(|e| e.to_string())?;
    if !head.starts_with(b"%PDF") {
        return Err("Not a PDF".into());
    }
    let mut tail = Vec::new();
    if length > PDF_SCAN {
        file.seek(SeekFrom::Start(
            length.saturating_sub(PDF_SCAN).max(PDF_SCAN),
        ))
        .map_err(|e| e.to_string())?;
        file.take(PDF_SCAN)
            .read_to_end(&mut tail)
            .map_err(|e| e.to_string())?;
    }
    // An encrypted PDF's strings are ciphertext.
    let encrypted = [&tail, &head].iter().any(|bytes| find(bytes, b"/Encrypt").is_some());
    let property = (!encrypted)
        .then(|| [&tail, &head].into_iter().find_map(|bytes| pdf_title(bytes)))
        .flatten()
        .filter(|title| reads_as_title(title) && !looks_generated(title) && !garbled(title));
    if length > 8 * 1024 * 1024 {
        // Too big to read the text: the title can only be offered.
        return Ok(property.map(|title| (title, "Document title", Confidence::Medium)));
    }
    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
    let text = match crate::text_cache::lookup(path, meta, &bytes) {
        Some(text) => text,
        None => crate::content::pdf(bytes, &|| false).unwrap_or_default(),
    };
    // The title property is often a browser tab ("… - Google Gemini"), a
    // converter's ("Converted Document") or a series name; it counts only
    // when the first pages bear it out.
    let opening: String = text.chars().take(4000).collect();
    if let Some(title) = property {
        if opening.trim().is_empty() {
            return Ok(Some((title, "Document title", Confidence::Medium)));
        }
        if borne_out(&title, &opening) {
            return Ok(Some((title, "Document title", Confidence::High)));
        }
    }
    Ok(from_text(&text, Format::Text))
}

/// The /Title of a PDF's Info dictionary, or an XMP dc:title. Bookmarks
/// carry a /Title too ("Chapter 1"), so a /Title counts only inside the
/// dictionary the trailer names as /Info, or, without one, outside any
/// outline entry.
fn pdf_title(bytes: &[u8]) -> Option<String> {
    if let Some(object) = info_object(bytes) {
        return find(object, b"/Title")
            .and_then(|at| title_at(object, at))
            .or_else(|| xmp_title(bytes));
    }
    let mut from = 0;
    while let Some(at) = find(&bytes[from..], b"/Title").map(|at| at + from) {
        if !in_outline(bytes, at) {
            return title_at(bytes, at).or_else(|| xmp_title(bytes));
        }
        from = at + 6;
    }
    xmp_title(bytes)
}

/// The body of the object the trailer's `/Info N G R` points at, when both
/// are in `bytes`.
fn info_object(bytes: &[u8]) -> Option<&[u8]> {
    let at = rfind(bytes, b"/Info")?;
    let rest: String = bytes[at + 5..]
        .iter()
        .take(40)
        .take_while(|b| b.is_ascii())
        .map(|&b| b as char)
        .collect();
    let mut words = rest.split(|c: char| c.is_ascii_whitespace() || c == '/' || c == '>').filter(|w| !w.is_empty());
    let (number, generation) = (words.next()?, words.next()?);
    if words.next()? != "R" || !number.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let header = format!("{number} {generation} obj");
    let mut search = bytes;
    // The last definition wins (incremental updates); the number must not be
    // the tail of a longer one ("11 0 obj" is not "1 0 obj").
    while let Some(start) = rfind(search, header.as_bytes()) {
        if start == 0 || !bytes[start - 1].is_ascii_digit() {
            let body = &bytes[start..];
            let end = find(body, b"endobj").unwrap_or(body.len().min(8192));
            return Some(&body[..end]);
        }
        search = &bytes[..start];
    }
    None
}

/// Whether the /Title at `at` belongs to a bookmark: its dictionary links to
/// a parent or neighbours.
fn in_outline(bytes: &[u8], at: usize) -> bool {
    let low = at.saturating_sub(1024);
    let start = rfind(&bytes[low..at], b"<<").map_or(at, |s| s + low);
    let high = (at + 1024).min(bytes.len());
    let end = find(&bytes[at..high], b">>").map_or(high, |e| e + at);
    let dict = &bytes[start..end];
    [&b"/Parent"[..], b"/Dest", b"/Prev", b"/Next", b"/First"]
        .iter()
        .any(|key| find(dict, key).is_some())
}

/// The string value after the /Title at `at`.
fn title_at(bytes: &[u8], at: usize) -> Option<String> {
    let rest = &bytes[at + 6..];
    let start = rest.iter().position(|b| !b.is_ascii_whitespace())?;
    let rest = &rest[start..];
    let raw = match rest.first()? {
        b'(' => literal(&rest[1..])?,
        b'<' => hex(&rest[1..])?,
        _ => return None,
    };
    let text = if let Some(utf8) = raw.strip_prefix(&[0xef, 0xbb, 0xbf]) {
        String::from_utf8_lossy(utf8).into_owned()
    } else if raw.starts_with(&[0xfe, 0xff]) {
        let units: Vec<u16> = raw[2..]
            .chunks_exact(2)
            .map(|pair| u16::from_be_bytes([pair[0], pair[1]]))
            .collect();
        String::from_utf16_lossy(&units)
    } else {
        raw.iter().map(|&b| b as char).collect()
    };
    let text = text.trim().to_string();
    (!text.is_empty()).then_some(text)
}

fn xmp_title(bytes: &[u8]) -> Option<String> {
    let at = find(bytes, b"<dc:title")?;
    let rest = &bytes[at..(at + 2048).min(bytes.len())];
    let li = find(rest, b"<rdf:li")?;
    let open = li + rest[li..].iter().position(|&b| b == b'>')? + 1;
    let close = open + find(&rest[open..], b"</rdf:li>")?;
    let text = String::from_utf8_lossy(&rest[open..close])
        .trim()
        .to_string();
    (!text.is_empty()).then_some(unescape(&text))
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

fn rfind(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).rposition(|w| w == needle)
}

/// A PDF literal string up to its balancing ')', with escapes resolved.
fn literal(bytes: &[u8]) -> Option<Vec<u8>> {
    let mut out = vec![];
    let mut depth = 0;
    let mut i = 0;
    while i < bytes.len() && out.len() < 1024 {
        match bytes[i] {
            b'\\' => {
                i += 1;
                match *bytes.get(i)? {
                    b'n' => out.push(b' '),
                    b'r' | b't' | b'b' | b'f' => out.push(b' '),
                    d @ b'0'..=b'7' => {
                        let mut value = (d - b'0') as u32;
                        for _ in 0..2 {
                            match bytes.get(i + 1) {
                                Some(d @ b'0'..=b'7') => {
                                    value = value * 8 + (d - b'0') as u32;
                                    i += 1;
                                }
                                _ => break,
                            }
                        }
                        out.push(value as u8);
                    }
                    other => out.push(other),
                }
            }
            b'(' => {
                depth += 1;
                out.push(b'(');
            }
            b')' if depth == 0 => return Some(out),
            b')' => {
                depth -= 1;
                out.push(b')');
            }
            other => out.push(other),
        }
        i += 1;
    }
    None
}

fn hex(bytes: &[u8]) -> Option<Vec<u8>> {
    let end = bytes.iter().position(|&b| b == b'>')?;
    let digits: Vec<u8> = bytes[..end]
        .iter()
        .filter(|b| b.is_ascii_hexdigit())
        .copied()
        .collect();
    digits
        .chunks(2)
        .map(|pair| {
            let text = std::str::from_utf8(pair).ok()?;
            u8::from_str_radix(&format!("{text:0<2}"), 16).ok()
        })
        .collect()
}

/// Text decoded with the wrong code page or not text at all: replacement
/// marks, control characters, or a run of Latin-1 accents.
fn garbled(title: &str) -> bool {
    let letters = title.chars().filter(|c| c.is_alphabetic()).count().max(1);
    let accents = title.chars().filter(|c| ('\u{80}'..='\u{ff}').contains(c)).count();
    title.chars().any(|c| c == '\u{fffd}' || c.is_control()) || accents * 2 > letters
}

/// What a tool leaves in a title when nobody wrote one.
fn looks_generated(title: &str) -> bool {
    let lower = title.to_lowercase();
    [
        "google gemini",
        "chatgpt",
        "converted document",
        "anonymous",
        "microsoft word",
        "microsoft powerpoint",
        "powerpoint presentation",
        ".doc",
        ".pdf",
        ".ppt",
        ".xls",
        "untitled",
        "slide 1",
        "document1",
        "presentation1",
        "book1",
    ]
    .iter()
    .any(|mark| lower.contains(mark))
        || ["presentation", "document", "title", "slide", "untitled document", "new document"]
            .contains(&lower.trim())
}

/// XML text with its five entities and numeric references resolved.
fn unescape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find('&') {
        out.push_str(&rest[..at]);
        rest = &rest[at..];
        let Some(end) = rest.find(';').filter(|end| *end <= 10) else {
            out.push('&');
            rest = &rest[1..];
            continue;
        };
        let entity = &rest[1..end];
        let decoded = match entity {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            _ => entity
                .strip_prefix("#x")
                .and_then(|hex| u32::from_str_radix(hex, 16).ok())
                .or_else(|| entity.strip_prefix('#').and_then(|n| n.parse().ok()))
                .and_then(char::from_u32),
        };
        match decoded {
            Some(c) => {
                out.push(c);
                rest = &rest[end + 1..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// The text runs of an XML fragment: the contents of every `<tag>` or
/// `<tag …>` element (Word's `w:t`, DrawingML's `a:t`), joined.
fn runs(xml: &str, tag: &str) -> String {
    let open = format!("<{tag}");
    let close = format!("</{tag}>");
    let mut out = String::new();
    let mut rest = xml;
    while let Some(at) = rest.find(&open) {
        // A line break, tab or new paragraph between two runs is a space
        // ("UNIT I" / "INTRODUCTION" on two lines of one title).
        let between = &rest[..at];
        if !out.is_empty()
            && !out.ends_with(' ')
            && ["<a:br", "<w:br", "<w:tab", "<w:cr", "</a:p>", "</w:p>"].iter().any(|m| between.contains(m))
        {
            out.push(' ');
        }
        rest = &rest[at + open.len()..];
        // `<w:t>` or `<w:t xml:space=…>`, not `<w:tab/>` or `<w:tbl>`.
        if !rest.starts_with(['>', ' ']) {
            continue;
        }
        let Some(start) = rest.find('>') else { break };
        if rest[..start].ends_with('/') {
            continue;
        }
        let Some(end) = rest[start..].find(&close) else { break };
        out.push_str(&unescape(&rest[start + 1..start + end]));
        rest = &rest[start + end + close.len()..];
    }
    out
}

fn zip_part(archive: &mut zip::ZipArchive<std::fs::File>, name: &str) -> Option<String> {
    let entry = archive.by_name(name).ok()?;
    if entry.size() > OFFICE_PART {
        return None;
    }
    let mut text = String::new();
    entry.take(OFFICE_PART).read_to_string(&mut text).ok()?;
    Some(text)
}

/// Word, PowerPoint and Excel files: the title in their properties, then a
/// Title or Heading 1 paragraph, the first slide's title, or an opening line
/// and the document's kind.
fn office(path: &Path, format: Format) -> Result<Found, String> {
    let file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let mut archive = zip::ZipArchive::new(file).map_err(|_| "Not an Office file".to_string())?;
    // The title property is often left over from a template or an older
    // document, so it counts only when the text bears it out.
    let property = zip_part(&mut archive, "docProps/core.xml")
        .map(|core| runs(&core, "dc:title").trim().to_string())
        .filter(|title| reads_as_title(title) && !looks_generated(title) && !boilerplate(title));
    if let Some(title) = property {
        let text = match format {
            Format::Word => zip_part(&mut archive, "word/document.xml").map(|b| runs(&b, "w:t")),
            Format::Slides => zip_part(&mut archive, "ppt/slides/slide1.xml").map(|b| runs(&b, "a:t")),
            _ => None,
        }
        .unwrap_or_default();
        if text.trim().is_empty() {
            // Nothing to check it against: trusted for a document with no
            // text yet, only offered for a workbook.
            let confidence = if format == Format::Workbook { Confidence::Medium } else { Confidence::High };
            return Ok(Some((title, "Document title", confidence)));
        }
        if borne_out(&title, &text) {
            return Ok(Some((title, "Document title", Confidence::High)));
        }
    }
    match format {
        Format::Word => {
            let Some(body) = zip_part(&mut archive, "word/document.xml") else {
                return Ok(None);
            };
            let paragraphs: Vec<(String, String)> = body
                .split("</w:p>")
                .take(200)
                .map(|paragraph| {
                    let style = paragraph
                        .find("<w:pStyle w:val=\"")
                        .and_then(|at| {
                            let rest = &paragraph[at + 17..];
                            rest.find('"').map(|end| rest[..end].to_lowercase())
                        })
                        .unwrap_or_default();
                    (style, runs(paragraph, "w:t").trim().to_string())
                })
                .filter(|(_, text)| !text.is_empty())
                .collect();
            // A Title paragraph names the document; a Heading 1 does only
            // when it opens it, and not when numbered ("1. Aim"), which
            // makes it a section.
            let titled = paragraphs
                .iter()
                .find(|(style, text)| style == "title" && reads_as_title(text) && !boilerplate(text))
                .or_else(|| {
                    paragraphs
                        .iter()
                        .take(3)
                        .filter(|(_, text)| !boilerplate(text))
                        .find(|(style, text)| {
                            (style.starts_with("heading1") || style == "heading 1")
                                && reads_as_title(text)
                                && !text.trim_start().starts_with(|c: char| c.is_ascii_digit())
                        })
                });
            if let Some((_, title)) = titled {
                return Ok(Some((title.clone(), "Title heading", Confidence::High)));
            }
            let text: String = paragraphs
                .iter()
                .take(40)
                .map(|(_, text)| text.as_str())
                .collect::<Vec<_>>()
                .join("\n");
            Ok(from_text(&text, Format::Text))
        }
        Format::Slides => {
            let Some(slide) = zip_part(&mut archive, "ppt/slides/slide1.xml") else {
                return Ok(None);
            };
            for shape in slide.split("</p:sp>") {
                if shape.contains("type=\"title\"") || shape.contains("type=\"ctrTitle\"") {
                    let title = runs(shape, "a:t");
                    let title = title.trim();
                    if reads_as_title(title) && !boilerplate(title) {
                        return Ok(Some((title.to_string(), "First slide title", Confidence::High)));
                    }
                }
            }
            let first = slide
                .split("</a:p>")
                .map(|paragraph| runs(paragraph, "a:t").trim().to_string())
                .find(|text| reads_as_title(text) && !boilerplate(text));
            Ok(first.map(|title| (title, "First slide text", Confidence::Medium)))
        }
        _ => Ok(None),
    }
}

/// Whether most of a title's words appear in the text.
fn borne_out(title: &str, text: &str) -> bool {
    let text = text.to_lowercase();
    let words: Vec<String> = title
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| w.chars().count() >= 3)
        .map(str::to_lowercase)
        .collect();
    !words.is_empty() && words.iter().filter(|w| text.contains(w.as_str())).count() * 3 >= words.len() * 2
}

/// "Artist - Title" from an MP3's ID3v2 tag or a FLAC's Vorbis comments.
fn music(path: &Path) -> Found {
    let mut bytes = vec![];
    std::fs::File::open(path)
        .ok()?
        .take(512 * 1024)
        .read_to_end(&mut bytes)
        .ok()?;
    let (title, artist) = if bytes.starts_with(b"ID3") {
        id3(&bytes)?
    } else if bytes.starts_with(b"fLaC") {
        vorbis(&bytes)?
    } else {
        return None;
    };
    let title = title.trim();
    if title.is_empty() {
        return None;
    }
    let artist = artist.trim();
    let name = if artist.is_empty() {
        title.to_string()
    } else {
        format!("{artist} - {title}")
    };
    Some((name, "Music tags", Confidence::High))
}

fn id3(bytes: &[u8]) -> Option<(String, String)> {
    let version = *bytes.get(3)?;
    let flags = *bytes.get(5)?;
    let syncsafe = |b: &[u8]| -> usize {
        b.iter().fold(0usize, |n, &x| (n << 7) | usize::from(x & 0x7f))
    };
    let size = syncsafe(bytes.get(6..10)?);
    let end = (10 + size).min(bytes.len());
    let mut at = 10;
    if flags & 0x40 != 0 && version >= 3 {
        let extended = if version == 4 {
            syncsafe(bytes.get(10..14)?)
        } else {
            u32::from_be_bytes(bytes.get(10..14)?.try_into().ok()?) as usize + 4
        };
        at += extended;
    }
    let (id_len, header_len) = if version == 2 { (3, 6) } else { (4, 10) };
    let (mut title, mut artist) = (String::new(), String::new());
    while at + header_len <= end {
        let id = &bytes[at..at + id_len];
        if id[0] == 0 {
            break;
        }
        let size = match version {
            2 => bytes[at + 3..at + 6]
                .iter()
                .fold(0usize, |n, &x| (n << 8) | usize::from(x)),
            3 => u32::from_be_bytes(bytes[at + 4..at + 8].try_into().ok()?) as usize,
            _ => syncsafe(&bytes[at + 4..at + 8]),
        };
        let body = bytes.get(at + header_len..(at + header_len + size).min(end))?;
        match id {
            b"TIT2" | b"TT2" => title = id3_text(body),
            b"TPE1" | b"TP1" => artist = id3_text(body),
            _ => {}
        }
        at += header_len + size;
    }
    Some((title, artist))
}

fn id3_text(body: &[u8]) -> String {
    let Some((&encoding, text)) = body.split_first() else {
        return String::new();
    };
    let decoded = match encoding {
        0 => text.iter().map(|&b| b as char).collect(),
        1 | 2 => {
            let (big, text) = match text {
                [0xfe, 0xff, rest @ ..] => (true, rest),
                [0xff, 0xfe, rest @ ..] => (false, rest),
                rest => (encoding == 2, rest),
            };
            let units: Vec<u16> = text
                .chunks_exact(2)
                .map(|pair| {
                    if big {
                        u16::from_be_bytes([pair[0], pair[1]])
                    } else {
                        u16::from_le_bytes([pair[0], pair[1]])
                    }
                })
                .collect();
            String::from_utf16_lossy(&units)
        }
        _ => String::from_utf8_lossy(text).into_owned(),
    };
    // Several values are separated by NUL; the first is the one shown.
    decoded.split('\0').next().unwrap_or_default().to_string()
}

fn vorbis(bytes: &[u8]) -> Option<(String, String)> {
    let mut at = 4;
    loop {
        let header = *bytes.get(at)?;
        let length = bytes
            .get(at + 1..at + 4)?
            .iter()
            .fold(0usize, |n, &x| (n << 8) | usize::from(x));
        let body = bytes.get(at + 4..at + 4 + length)?;
        if header & 0x7f == 4 {
            let le = |b: &[u8]| u32::from_le_bytes(b.try_into().unwrap_or_default()) as usize;
            let vendor = le(body.get(0..4)?);
            let mut i = 4 + vendor;
            let count = le(body.get(i..i + 4)?);
            i += 4;
            let (mut title, mut artist) = (String::new(), String::new());
            for _ in 0..count.min(256) {
                let len = le(body.get(i..i + 4)?);
                let comment = String::from_utf8_lossy(body.get(i + 4..i + 4 + len)?);
                i += 4 + len;
                if let Some((key, value)) = comment.split_once('=') {
                    match key.to_ascii_uppercase().as_str() {
                        "TITLE" if title.is_empty() => title = value.to_string(),
                        "ARTIST" if artist.is_empty() => artist = value.to_string(),
                        _ => {}
                    }
                }
            }
            return Some((title, artist));
        }
        if header & 0x80 != 0 {
            return None;
        }
        at += 4 + length;
    }
}

fn from_text(text: &str, format: Format) -> Found {
    match format {
        Format::Code => {
            return doc_comment(text).map(|t| (t, "Leading comment", Confidence::Low));
        }
        Format::Table => {
            return table_header(text).map(|t| (t, "Column names", Confidence::Low));
        }
        Format::Markdown => {
            if let Some(found) = markdown_title(text) {
                return Some(found);
            }
        }
        Format::Html => {
            if let Some(title) = html_title(text) {
                return Some((title, "Page title", Confidence::High));
            }
        }
        _ => {}
    }
    let body = match format {
        Format::Html => strip_tags(text),
        Format::Markdown => without_code(text),
        _ => text.to_string(),
    };
    document_kind(&body)
        .map(|name| (name, "Document type and date", Confidence::High))
        .or_else(|| title_line(&body).map(|t| (t, "Opening line", Confidence::Medium)))
}

fn markdown_title(text: &str) -> Found {
    let mut lines = text.lines().peekable();
    if lines.peek().is_some_and(|line| line.trim_start_matches('\u{feff}').trim() == "---") {
        lines.next();
        for line in lines.by_ref() {
            if line.trim() == "---" {
                break;
            }
            if let Some(title) = line.strip_prefix("title:") {
                let title = title.trim().trim_matches(['"', '\'']);
                if !title.is_empty() {
                    return Some((title.to_string(), "Front matter title", Confidence::High));
                }
            }
        }
    }
    // The title is a heading at the top: within the first three lines of
    // content, not counting badges, images and comments. A heading further
    // down ("# Installation") names a section, not the document; a `#` inside
    // a code block is not a heading at all.
    let mut content = 0;
    let mut previous: Option<&str> = None;
    for line in lines {
        let line = line.trim_start_matches('\u{feff}').trim_end();
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            return None;
        }
        if trimmed.is_empty() {
            continue;
        }
        // A Setext heading: its text, then a line of =.
        if let Some(text) = previous
            && trimmed.len() >= 3
            && trimmed.chars().all(|c| c == '=')
        {
            return Some((markdown_inline(text), "First heading", Confidence::High));
        }
        if ["![", "[![", "<!--", "<img", "<p", "<div", "<a ", "<br"].iter().any(|m| trimmed.starts_with(m)) {
            continue;
        }
        if let Some(heading) = line.strip_prefix("# ") {
            // An optional closing run of #, after a space ("# Title ##"),
            // not the # of "C#".
            let heading = heading.trim();
            let heading = match heading.trim_end_matches('#') {
                open if open.len() < heading.len() && open.ends_with(' ') => open.trim(),
                _ => heading,
            };
            let heading = markdown_inline(heading);
            return (!heading.is_empty()).then_some((heading, "First heading", Confidence::High));
        }
        content += 1;
        if content > 3 {
            return None;
        }
        previous = Some(trimmed);
    }
    None
}

/// Markdown's inline marks taken out: `[text](link)` keeps its text, and
/// emphasis and code marks go.
fn markdown_inline(text: &str) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some(open) = rest.find('[') {
        out.push_str(&rest[..open]);
        let after = &rest[open + 1..];
        match after.find("](").zip(after.find(')')) {
            Some((mid, close)) if mid < close => {
                out.push_str(&after[..mid]);
                rest = &after[close + 1..];
            }
            _ => {
                out.push('[');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out.replace("**", "").replace("__", "").replace(['`', '*'], "").trim().to_string()
}

/// An HTML page's <title>, without a trailing " | Site name".
fn html_title(text: &str) -> Option<String> {
    let lower = text.to_ascii_lowercase();
    let start = lower.find("<title")?;
    let open = start + lower[start..].find('>')? + 1;
    let close = open + lower[open..].find("</title")?;
    let title = unescape(text[open..close].trim());
    let title = title
        .split(" | ")
        .next()
        .filter(|part| part.chars().filter(|c| c.is_alphabetic()).count() >= 3)
        .unwrap_or(&title)
        .trim()
        .to_string();
    (!title.is_empty()).then_some(title)
}

/// Markdown without its fenced code blocks.
fn without_code(text: &str) -> String {
    let mut fenced = false;
    text.lines()
        .filter(|line| {
            let fence = ["```", "~~~"].iter().any(|f| line.trim_start().starts_with(f));
            if fence {
                fenced = !fenced;
            }
            !fence && !fenced
        })
        .collect::<Vec<_>>()
        .join("
")
}

fn strip_tags(text: &str) -> String {
    let mut out = String::new();
    let mut inside = false;
    for c in text.chars() {
        match c {
            '<' => inside = true,
            '>' => {
                inside = false;
                out.push('\n');
            }
            c if !inside => out.push(c),
            _ => {}
        }
    }
    out
}

/// Code says little in its first line; only a leading comment names it:
/// the first line of the comment block at the top that reads as a title,
/// passing over licences, encodings and tool directives.
fn doc_comment(text: &str) -> Option<String> {
    const SKIP: [&str; 14] = [
        "copyright", "(c)", "©", "spdx", "license", "licence", "eslint", "@ts-", "prettier",
        "-*-", "coding:", "noqa", "type: ignore", "all rights reserved",
    ];
    const DIRECTIVES: [&str; 10] = [
        "#include", "#define", "#pragma", "#if", "#import", "#!", "#[", "#region", "#endregion", "#requires",
    ];
    for line in text.lines().take(30) {
        let line = line.trim();
        let lower = line.to_lowercase();
        if line.is_empty() || DIRECTIVES.iter().any(|d| lower.starts_with(d)) {
            continue;
        }
        let Some(said) = ["//!", "///", "//", "/**", "/*", "\"\"\"", "#", "--", "<#", "*"]
            .iter()
            .find_map(|mark| line.strip_prefix(mark))
        else {
            // Code before any comment: nothing names the file.
            return None;
        };
        let said = said
            .trim()
            .trim_end_matches("*/")
            .trim_end_matches("\"\"\"")
            .trim_end_matches("#>")
            .trim();
        let lower = said.to_lowercase();
        if said.is_empty() || SKIP.iter().any(|mark| lower.contains(mark)) {
            continue;
        }
        let sentence = said.split(['.', '!', '?']).next().unwrap_or(said).trim();
        if reads_as_title(sentence) {
            return Some(sentence.to_string());
        }
    }
    None
}

/// A table's columns say what it holds.
fn table_header(text: &str) -> Option<String> {
    let header = text.lines().next()?;
    let columns: Vec<&str> = header
        .split([',', '\t', ';'])
        .map(|column| column.trim().trim_matches('"'))
        .filter(|column| !column.is_empty())
        .take(4)
        .collect();
    let joined = columns.join(" ");
    reads_as_title(&joined).then_some(joined)
}

/// What kind of document this opens as, with its number and date: "Invoice
/// 4471 2026-03-12", "Receipt 2026-01-05", "Bank statement 2026-02-28".
/// Only the first lines count, and only a line that starts with the word.
fn document_kind(text: &str) -> Option<String> {
    const KINDS: [(&str, &str); 22] = [
        ("tax invoice", "Invoice"),
        ("invoice", "Invoice"),
        ("receipt", "Receipt"),
        ("payment receipt", "Receipt"),
        ("bank statement", "Bank statement"),
        ("account statement", "Statement"),
        ("statement of account", "Statement"),
        ("quotation", "Quotation"),
        ("purchase order", "Purchase order"),
        ("payslip", "Payslip"),
        ("pay slip", "Payslip"),
        ("salary slip", "Payslip"),
        ("curriculum vitae", "CV"),
        ("resume", "Resume"),
        ("résumé", "Resume"),
        ("cover letter", "Cover letter"),
        ("offer letter", "Offer letter"),
        ("minutes of", "Meeting minutes"),
        ("meeting minutes", "Meeting minutes"),
        ("certificate", "Certificate"),
        ("rental agreement", "Rental agreement"),
        ("lease agreement", "Lease agreement"),
    ];
    let lines: Vec<&str> = text
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .take(40)
        .collect();
    let (kind, line, keyword) = lines.iter().take(12).find_map(|line| {
        let lower = line.to_lowercase();
        // Longer phrases first: "tax invoice" before "invoice".
        let mut kinds = KINDS.to_vec();
        kinds.sort_by_key(|(word, _)| std::cmp::Reverse(word.len()));
        kinds.into_iter().find_map(|(word, kind)| {
            let rest = lower.strip_prefix(word)?;
            // A whole word, not "invoices are due".
            if rest.starts_with(|c: char| c.is_alphabetic()) {
                return None;
            }
            // The line is the heading of that kind: after the word come only
            // its number, date and labels, not a sentence ("Resume building
            // tips", "Certificate programs we offer").
            const LABELS: [&str; 16] = [
                "no", "number", "nr", "num", "id", "ref", "date", "dated", "original", "duplicate",
                "triplicate", "copy", "for", "recipient", "of", "page",
            ];
            let words: Vec<&str> = rest
                .split(|c: char| c.is_whitespace() || ":#|,()".contains(c))
                .map(|w| w.trim_matches(|c: char| ".-–—".contains(c)))
                .filter(|w| !w.is_empty())
                .collect();
            let plain = word == "minutes of"
                || (words.len() <= 8
                    && words.iter().all(|w| {
                        w.chars().any(|c| c.is_ascii_digit())
                            || LABELS.contains(w)
                            || month_name(w).is_some()
                    }));
            plain.then_some((kind, *line, word))
        })
    })?;
    let mut name = kind.to_string();
    if let Some(number) = document_number(&line[keyword.len().min(line.len())..]) {
        name.push(' ');
        name.push_str(&number);
    }
    if let Some(date) = lines.iter().find_map(|line| first_date(line)) {
        name.push(' ');
        name.push_str(&date);
    }
    Some(name)
}

/// "4471" from " #4471", " No. INV-0042", ": 4471 dated…".
fn document_number(rest: &str) -> Option<String> {
    rest.split(|c: char| c.is_whitespace() || c == ':')
        .map(|word| word.trim_matches(|c: char| "#.,;()".contains(c)))
        .filter(|word| !word.is_empty())
        .take(3)
        .find(|word| {
            let lower = word.to_lowercase();
            !matches!(lower.as_str(), "no" | "number" | "nr" | "num" | "id" | "ref")
                && word.chars().any(|c| c.is_ascii_digit())
                && word.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '/')
                && word.len() <= 20
                && first_date(word).is_none()
        })
        .map(|word| word.replace('/', "-"))
}

/// The first unambiguous date in a line, as YYYY-MM-DD: 2026-03-12,
/// 12/03/2026 only when one side is over 12, 12 March 2026, March 12, 2026.
fn first_date(line: &str) -> Option<String> {
    let valid = |y: i32, m: u32, d: u32| {
        (1990..=2100).contains(&y) && chrono::NaiveDate::from_ymd_opt(y, m, d).is_some()
    };
    let words: Vec<String> = line
        .split(|c: char| c.is_whitespace() || c == ',')
        .filter(|w| !w.is_empty())
        .map(|w| w.trim_matches(|c: char| ".()".contains(c)).to_lowercase())
        .collect();
    for (i, word) in words.iter().enumerate() {
        let parts: Vec<&str> = word.split(['-', '/', '.']).collect();
        if parts.len() == 3 && parts.iter().all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_digit())) {
            let n: Vec<u32> = parts.iter().filter_map(|p| p.parse().ok()).collect();
            if n.len() != 3 {
                continue;
            }
            if parts[0].len() == 4 && valid(n[0] as i32, n[1], n[2]) {
                return Some(format!("{:04}-{:02}-{:02}", n[0], n[1], n[2]));
            }
            if parts[2].len() == 4 {
                let (day, month) = if n[0] > 12 && n[1] <= 12 {
                    (n[0], n[1])
                } else if n[1] > 12 && n[0] <= 12 {
                    (n[1], n[0])
                } else {
                    continue;
                };
                if valid(n[2] as i32, month, day) {
                    return Some(format!("{:04}-{month:02}-{day:02}", n[2]));
                }
            }
        }
        let month_of = |w: &str| month_name(w);
        let number = |w: Option<&String>| {
            w.and_then(|w| w.trim_end_matches(|c: char| c.is_alphabetic()).parse::<u32>().ok())
        };
        if let Some(month) = month_of(word) {
            // March 12 2026
            if let (Some(day), Some(year)) = (number(words.get(i + 1)), number(words.get(i + 2)))
                && valid(year as i32, month, day)
            {
                return Some(format!("{year:04}-{month:02}-{day:02}"));
            }
            // 12 March 2026
            if i > 0
                && let (Some(day), Some(year)) = (number(words.get(i - 1)), number(words.get(i + 1)))
                && valid(year as i32, month, day)
            {
                return Some(format!("{year:04}-{month:02}-{day:02}"));
            }
        }
    }
    None
}

/// A month written as a word: "March", "mar", "Sept"; not "marketing".
fn month_name(word: &str) -> Option<u32> {
    const NAMES: [&str; 12] = [
        "january", "february", "march", "april", "may", "june", "july", "august", "september",
        "october", "november", "december",
    ];
    let word = word.to_lowercase();
    let word = word.trim_end_matches('.');
    NAMES
        .iter()
        .position(|name| {
            *name == word
                || (word.len() == 3 && name.starts_with(word))
                || (word == "sept" && *name == "september")
        })
        .map(|m| m as u32 + 1)
}

/// The first line near the top that a person would call a title.
fn title_line(text: &str) -> Option<String> {
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .take(5)
        .find(|line| {
            let letters = line.chars().filter(|c| c.is_alphabetic()).count();
            let digits = line.chars().filter(char::is_ascii_digit).count();
            reads_as_title(line)
                && !boilerplate(line)
                && !line.ends_with(['.', ',', ';', ':'])
                && line.split_whitespace().count() <= 12
                // A timestamped log line is not a title.
                && digits * 3 <= letters
        })
        .map(str::to_string)
}

fn reads_as_title(line: &str) -> bool {
    let chars = line.chars().count();
    let letters = line.chars().filter(|c| c.is_alphabetic()).count();
    TITLE_CHARS.contains(&chars)
        && letters * 2 >= chars
        && letters >= 3
        && !is_date_or_page(line)
        && !line.contains("://")
        && !line.contains('@')
        && !secret_like(line)
}

/// Codes, keys and tokens ("Tq7wmzpa", "58kd-rhvn", "Xb4Kq9TzLm2…"): words
/// that mix letters and digits as often as there are plain words, or one
/// long run of them. Never a name, and never to be put in one.
fn secret_like(line: &str) -> bool {
    let words: Vec<&str> = line
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .collect();
    let mixed = |w: &str| w.chars().any(|c| c.is_ascii_digit()) && w.chars().any(char::is_alphabetic);
    let plain = words.iter().filter(|w| w.chars().all(char::is_alphabetic)).count();
    let tokens = words.iter().filter(|w| mixed(w)).count();
    let long = words.iter().any(|w| w.len() >= 16 && mixed(w));
    long || (tokens > 0 && tokens >= plain)
}

/// Header lines that name an institution, a greeting or a section, not the
/// document: "Department of Computer Science", "Northfield Engineering
/// College", "To whom it may concern", "Table of contents".
fn boilerplate(line: &str) -> bool {
    let lower = line.to_lowercase();
    let lower = lower.trim();
    const STARTS: [&str; 27] = [
        "department of", "school of", "faculty of", "institute of", "to whom", "to whomsoever",
        "dear ", "table of contents", "contents", "abstract", "acknowledg", "bonafide",
        // Form fields on a cover page: "Name: …", "Roll no: …".
        "name:", "name :", "name -", "date:", "roll no", "reg no", "register no", "register number",
        "registration no", "class:", "section:", "submitted by", "submitted to", "prepared by", "guided by",
    ];
    const ANYWHERE: [&str; 7] = [
        "university", "college", "institute of technology", "school of engineering",
        "autonomous institution", "affiliated to", "approved by aicte",
    ];
    STARTS.iter().any(|s| lower.starts_with(s)) || ANYWHERE.iter().any(|s| lower.contains(s))
}

fn is_date_or_page(line: &str) -> bool {
    let lower = line.to_lowercase();
    let without_digits: String = lower.chars().filter(|c| !c.is_ascii_digit()).collect();
    let words: Vec<&str> = without_digits
        .split(|c: char| !c.is_alphabetic())
        .filter(|word| !word.is_empty())
        .collect();
    const MONTHS: [&str; 12] = [
        "jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec",
    ];
    let digits = lower.chars().any(|c| c.is_ascii_digit());
    match words.as_slice() {
        ["page", ..] if digits => true,
        ["page"] | ["of"] => true,
        ["page", "of"] => true,
        [month] if digits => MONTHS.iter().any(|m| month.starts_with(m)),
        _ => false,
    }
}

/// The text as a stem: only what a Windows name can hold, without words that
/// say nothing, cut at a word near 60 characters. The source's own casing is
/// kept; text in capitals becomes sentence case.
fn clean(title: &str) -> Option<String> {
    let title = &unmangle(title);
    // En and em dashes are dashes: "Weeks 1–12" stays "Weeks 1-12".
    let title = title.replace(['\u{2013}', '\u{2014}', '\u{2212}'], "-");
    let mut words: Vec<String> = title
        .split(|c: char| c.is_whitespace() || "/\\:|_*?\"<>".contains(c))
        .filter_map(|word| {
            // A lone "-" or "&" separates words ("Artist - Title").
            if matches!(word, "-" | "&") {
                return Some(word.to_string());
            }
            // "(1)" and "(2)" are copies' marks, not words.
            if (3..=4).contains(&word.len())
                && word.starts_with('(')
                && word.ends_with(')')
                && word[1..word.len() - 1].chars().all(|c| c.is_ascii_digit())
            {
                return Some("(n)".to_string());
            }
            // Punctuation at the edges goes; "C++", "C#", "Node.js" and
            // "v1.2" keep theirs.
            let word = word
                .trim_start_matches(|c: char| !c.is_alphanumeric())
                .trim_end_matches(|c: char| !c.is_alphanumeric() && !"+#".contains(c));
            let word: String = word
                .chars()
                .filter(|c| c.is_alphanumeric() || "-'&,()+#.".contains(*c))
                .collect();
            (!word.is_empty()).then_some(word)
        })
        .collect();
    // "Copy of …" (from a copy in Drive or OneDrive) says nothing.
    if words.len() > 2 && words[0].eq_ignore_ascii_case("copy") && words[1].eq_ignore_ascii_case("of") {
        words.drain(..2);
    }
    // Marks that say nothing ("final", "copy", "v2", "(1)") go from the end
    // only: "New York trip" keeps "New", "Copy editing guide" keeps "Copy".
    while words
        .last()
        .is_some_and(|w| w == "(n)" || says_nothing(w) || matches!(w.as_str(), "-" | "&"))
    {
        words.pop();
    }
    words.retain(|w| w != "(n)");
    while words.first().is_some_and(|w| matches!(w.as_str(), "-" | "&")) {
        words.remove(0);
    }
    let mut kept: Vec<String> = Vec::new();
    let mut length = 0;
    for word in words {
        length += word.chars().count() + usize::from(!kept.is_empty());
        if length > LONGEST {
            break;
        }
        kept.push(word);
    }
    while kept.last().is_some_and(|w| matches!(w.as_str(), "-" | "&")) {
        kept.pop();
    }
    if kept.is_empty() {
        return None;
    }
    let joined = kept.join(" ");
    let letters: Vec<char> = joined.chars().filter(|c| c.is_alphabetic()).collect();
    let shouting = kept.len() > 1 && letters.len() > 4 && letters.iter().all(|c| !c.is_lowercase());
    // Text set in capitals becomes sentence case, but acronyms and numerals
    // keep their capitals: words of up to three letters that are not common
    // words (AI, LAN, CSS, III), and well-known longer ones (HTML, JSON).
    let joined = if shouting {
        const COMMON: [&str; 40] = [
            "a", "an", "and", "the", "of", "to", "in", "on", "for", "with", "by", "at", "or", "as",
            "is", "be", "it", "its", "we", "you", "our", "my", "your", "new", "how", "why", "all",
            "are", "not", "can", "get", "use", "one", "two", "day", "end", "out", "up", "no", "yes",
        ];
        const LONG: [&str; 12] = [
            "HTML", "JSON", "HTTP", "HTTPS", "UNIX", "IEEE", "ASCII", "NASA", "YAML", "TOML", "AJAX", "UIUX",
        ];
        joined
            .split(' ')
            .enumerate()
            .map(|(i, word)| {
                let lower = word.to_lowercase();
                let letters = word.chars().filter(|c| c.is_alphabetic()).count();
                let acronym = (letters <= 3 && !COMMON.contains(&lower.as_str()))
                    || LONG.contains(&word.trim_matches(|c: char| !c.is_alphanumeric()));
                if i > 0 && acronym { word.to_string() } else { lower }
            })
            .collect::<Vec<_>>()
            .join(" ")
    } else {
        joined
    };
    let mut chars = joined.chars();
    let first: String = chars.next()?.to_uppercase().collect();
    Some(first + chars.as_str())
}

/// Punctuation saved as UTF-8 and read back as Windows-1252, then saved
/// again ("Release â€” notes"): put back as it was meant.
fn unmangle(text: &str) -> String {
    const MANGLED: [(&str, &str); 8] = [
        ("\u{e2}\u{20ac}\u{201d}", "\u{2014}"),
        ("\u{e2}\u{20ac}\u{201c}", "\u{2013}"),
        ("\u{e2}\u{20ac}\u{2122}", "'"),
        ("\u{e2}\u{20ac}\u{2dc}", "'"),
        ("\u{e2}\u{20ac}\u{153}", "\""),
        ("\u{e2}\u{20ac}\u{9d}", "\""),
        ("\u{e2}\u{20ac}\u{a6}", "..."),
        ("\u{c3}\u{a9}", "\u{e9}"),
    ];
    let mut text = text.to_string();
    if text.contains('\u{e2}') || text.contains('\u{c3}') {
        for (bad, good) in MANGLED {
            text = text.replace(bad, good);
        }
    }
    text
}

/// `final`, `copy`, and version marks like `v2`.
fn says_nothing(word: &str) -> bool {
    let lower = word.to_lowercase();
    let version = lower
        .strip_prefix('v')
        .is_some_and(|n| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()));
    version || NOISE.contains(&lower.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn named(file: &str, contents: &[u8]) -> Result<String, String> {
        suggestion(file, contents).map(|s| s.name)
    }

    fn suggestion(file: &str, contents: &[u8]) -> Result<Suggestion, String> {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(file);
        std::fs::write(&path, contents).unwrap();
        suggest(&path)
    }

    #[test]
    fn each_source_is_tried_in_order() {
        let front = "---\ntitle: \"Quarterly Plan\"\n---\n# Other heading\n";
        let s = suggestion("a.md", front.as_bytes()).unwrap();
        assert_eq!(s.name, "Quarterly Plan.md");
        assert_eq!((s.source, s.confidence), ("Front matter title", Confidence::High));
        assert_eq!(
            named("untitled.md", b"intro\n# Trip planning for Lisbon\n").unwrap(),
            "Trip planning for Lisbon.md"
        );
        let page = "<html><head><TITLE>Lease Terms | Acme Homes</TITLE></head><h1>Other</h1></html>";
        assert_eq!(
            named("a.html", page.as_bytes()).unwrap(),
            "Lease Terms.html"
        );
        let opening = "Project Kickoff Notes\n\nWe met on Tuesday.\n";
        let s = suggestion("a.txt", opening.as_bytes()).unwrap();
        assert_eq!(s.name, "Project Kickoff Notes.txt");
        assert_eq!(s.confidence, Confidence::Medium);
        // No sentence fragments: a file that only has prose keeps its name.
        let prose =
            "This agreement is made between the landlord and the tenant named below, in full.\n";
        assert!(named("a.txt", prose.as_bytes()).is_err());
    }

    #[test]
    fn documents_are_named_by_kind_number_and_date() {
        let invoice = "Page 1 of 3\nMarch 12, 2026\nInvoice #4471\nAmount due: 300.\n";
        let s = suggestion("scan.txt", invoice.as_bytes()).unwrap();
        assert_eq!(s.name, "Invoice 4471 2026-03-12.txt");
        assert_eq!(s.source, "Document type and date");
        assert_eq!(
            named("a.txt", b"Tax Invoice No: INV-0042\nDate: 2026-01-05\n").unwrap(),
            "Invoice INV-0042 2026-01-05.txt"
        );
        assert_eq!(
            named("a.txt", b"RECEIPT\nPaid 14/02/2026\n").unwrap(),
            "Receipt 2026-02-14.txt"
        );
        // 03/04/2026 could be either; no date rather than a wrong one.
        assert_eq!(
            named("a.txt", b"Bank Statement\nPeriod ending 03/04/2026\n").unwrap(),
            "Bank statement.txt"
        );
        // A word inside a sentence is not a document's kind.
        assert!(named("a.txt", b"invoices are due on the first of the month and late fees apply.\n").is_err());
    }

    #[test]
    fn code_and_tables_are_only_offered() {
        let s = suggestion("a.rs", b"// Parses the budget file.\nfn main() {}\n").unwrap();
        assert_eq!(s.name, "Parses the budget file.rs");
        assert_eq!(s.confidence, Confidence::Low);
        assert!(named("a.rs", b"fn main() {}\n").is_err());
        let s = suggestion("a.csv", b"name,email,total\nAda,a@b.c,3\n").unwrap();
        assert_eq!(s.name, "Name email total.csv");
        assert_eq!(s.confidence, Confidence::Low);
    }

    #[test]
    fn office_files_use_their_title_or_first_heading() {
        use std::io::Write;
        let dir = tempfile::tempdir().unwrap();
        let docx = |name: &str, core: &str, body: &str| {
            let path = dir.path().join(name);
            let mut zip = zip::ZipWriter::new(std::fs::File::create(&path).unwrap());
            let options = zip::write::SimpleFileOptions::default();
            zip.start_file("docProps/core.xml", options).unwrap();
            zip.write_all(core.as_bytes()).unwrap();
            zip.start_file("word/document.xml", options).unwrap();
            zip.write_all(body.as_bytes()).unwrap();
            zip.finish().unwrap();
            suggest(&path)
        };
        let core = "<cp:coreProperties><dc:title>Budget Review &amp; Plan</dc:title></cp:coreProperties>";
        let s = docx("Document1.docx", core, "<w:document/>").unwrap();
        assert_eq!((s.name.as_str(), s.source), ("Budget Review & Plan.docx", "Document title"));
        let body = r#"<w:body><w:p><w:r><w:t>Draft only</w:t></w:r></w:p><w:p><w:pPr><w:pStyle w:val="Title"/></w:pPr><w:r><w:t xml:space="preserve">Hiring </w:t></w:r><w:r><w:t>Plan 2027</w:t></w:r></w:p></w:body>"#;
        let s = docx("Doc.docx", "<cp:coreProperties><dc:title>Microsoft Word - x.docx</dc:title></cp:coreProperties>", body).unwrap();
        assert_eq!((s.name.as_str(), s.source), ("Hiring Plan 2027.docx", "Title heading"));
    }

    #[test]
    fn music_is_named_from_its_tags() {
        let frame = |id: &[u8], text: &str| {
            let mut f = id.to_vec();
            f.extend(((text.len() + 1) as u32).to_be_bytes());
            f.extend([0, 0, 3]);
            f.extend(text.as_bytes());
            f
        };
        let mut frames = frame(b"TIT2", "Clair de Lune");
        frames.extend(frame(b"TPE1", "Debussy"));
        let mut tag = b"ID3\x03\x00\x00".to_vec();
        let size = frames.len();
        tag.extend([
            (size >> 21) as u8 & 0x7f,
            (size >> 14) as u8 & 0x7f,
            (size >> 7) as u8 & 0x7f,
            size as u8 & 0x7f,
        ]);
        tag.extend(frames);
        tag.extend([0u8; 64]);
        let s = suggestion("track01.mp3", &tag).unwrap();
        assert_eq!((s.name.as_str(), s.source), ("Debussy - Clair de Lune.mp3", "Music tags"));
    }

    #[test]
    fn only_camera_names_are_renamed_from_dates() {
        assert!(camera_name("IMG_1234"));
        assert!(camera_name("DSC01234"));
        assert!(camera_name("PXL_20240506_070809123"));
        assert!(camera_name("WhatsApp Image 2024-05-06 at 07.08.09"));
        assert!(camera_name("20240506_070809"));
        assert!(!camera_name("Holiday in Lisbon"));
        assert!(!camera_name("logo"));
        assert!(named("Holiday.jpg", b"\xff\xd8\xff\xd9").is_err());
        // A camera name without a date inside keeps its name.
        assert!(named("IMG_0001.jpg", b"\xff\xd8\xff\xd9").is_err());
    }

    #[test]
    fn names_are_safe_whatever_the_text_says() {
        assert_eq!(
            named("a.txt", b"../../.ssh/authorized keys\n").unwrap(),
            "Ssh authorized keys.txt"
        );
        assert_eq!(
            named("a.TXT", b"Budget FINAL v2 copy\n").unwrap(),
            "Budget.TXT"
        );
        assert_eq!(
            named("a.txt", b"QUARTERLY SALES REVIEW\n").unwrap(),
            "Quarterly sales review.txt"
        );
        let long = "word ".repeat(11);
        let name = named("a.txt", format!("{long}\n").as_bytes()).unwrap();
        assert!(name.len() <= LONGEST + ".txt".len(), "{name}");
        assert!(
            named("a.txt", b"CON\n").is_err() || named("a.txt", b"CON\n").unwrap() != "CON.txt"
        );
        // Already named after what it says.
        assert!(named("Trip plan.md", b"# Trip plan\n").is_err());
        // A chosen name is only offered a replacement from an opening line.
        let kept = suggestion("Budget v1.txt", b"Budget draft one\n").unwrap();
        assert_eq!(kept.confidence, Confidence::Low);
        assert_eq!(
            suggestion("New Text Document (2).txt", b"Budget draft one\n").unwrap().confidence,
            Confidence::Medium
        );
        // A timestamped log line is not a title.
        assert!(named("log.txt", b"AppListBackup cleanup started 08/08/2026 16:34:58\n").is_err());
    }

    #[test]
    fn what_has_nothing_to_say_keeps_its_name() {
        assert!(named("a.txt", &[1, 0, 2, 0, 3, 0, 0, 0]).is_err());
        assert!(named("a.txt", b"$$$ 1234 %%% ### 99\n").is_err());
        assert!(named("a.txt", b"").is_err());
        assert!(named("a.zip", b"Looks like a title\n").is_err());
        assert!(suggest(tempfile::tempdir().unwrap().path()).is_err());
    }

    #[test]
    fn a_pdf_uses_its_title_metadata() {
        let pdf = b"%PDF-1.4\n1 0 obj\n<< /Title (Annual Report \\(2026\\)) /Producer (x) >>\nendobj\n%%EOF\n";
        assert_eq!(
            named("scan0012.pdf", pdf).unwrap(),
            "Annual Report 2026.pdf"
        );
        let utf16 = b"%PDF-1.7\n<< /Title <FEFF005200E9007300FC006D00E9> >>\n%%EOF";
        assert_eq!(named("a.pdf", utf16).unwrap(), "R\u{e9}s\u{fc}m\u{e9}.pdf");
    }

    /// Real-world shapes that once produced a wrong name.
    #[test]
    fn names_are_not_mangled() {
        // Words that only say nothing at the end: "New", "Copy", "Draft" at
        // the start carry meaning.
        assert_eq!(named("untitled.txt", b"New York Trip\n").unwrap(), "New York Trip.txt");
        assert_eq!(named("untitled.txt", b"Copy Editing Guide\n").unwrap(), "Copy Editing Guide.txt");
        assert_eq!(named("untitled.txt", b"Copy of Budget Plan\n").unwrap(), "Budget Plan.txt");
        assert_eq!(named("untitled.txt", b"Budget Plan (2)\n").unwrap(), "Budget Plan.txt");
        // Punctuation that is part of a name.
        assert_eq!(named("untitled.md", b"# C++ Primer\n").unwrap(), "C++ Primer.md");
        assert_eq!(named("untitled.md", b"# Node.js Guide v1.2\n").unwrap(), "Node.js Guide v1.2.md");
        assert_eq!(named("untitled.md", b"# Learning C#\n").unwrap(), "Learning C#.md");
        // Markdown marks are not part of the title.
        assert_eq!(
            named("untitled.md", b"# **Seek** [Release notes](https://x.y/z)\n").unwrap(),
            "Seek Release notes.md"
        );
        assert_eq!(named("untitled.md", b"Trip Plan\n=========\n\nDay one\n").unwrap(), "Trip Plan.md");
    }

    #[test]
    fn a_name_is_not_made_worse() {
        // The current name holds the title and more: kept.
        assert!(named("Budget final.md", b"# Budget\n").is_err());
        assert!(named("Trip plan 2026.md", b"# Trip plan\n").is_err());
        assert!(named("Budget v2.txt", b"Budget\n").is_err());
    }

    #[test]
    fn only_a_heading_at_the_top_names_a_markdown_file() {
        let readme = "Some intro text.\n\nMore intro.\n\nAnd more.\n\nStill more.\n\n# Installation\n";
        assert_ne!(
            suggestion("untitled.md", readme.as_bytes()).map(|s| s.source).ok(),
            Some("First heading")
        );
        let code = "```python\n# set up the path\nimport os\n```\n";
        assert!(named("untitled.md", code.as_bytes()).is_err());
        let badges = "[![build](https://x.y/b.svg)](https://x.y)\n<!-- toc -->\n# Seek\n";
        assert_eq!(named("untitled.md", badges.as_bytes()).unwrap(), "Seek.md");
    }

    #[test]
    fn a_kind_is_a_heading_not_a_sentence_start() {
        assert_ne!(
            suggestion("untitled.txt", b"Resume building tips\nWrite clearly.\n").unwrap().source,
            "Document type and date"
        );
        assert_ne!(
            suggestion("untitled.txt", b"Certificate programs we offer\n").unwrap().source,
            "Document type and date"
        );
        assert_eq!(
            named("a.txt", b"Tax Invoice - Original for Recipient\nInvoice No: 88\nDate: 5 March 2026\n").unwrap(),
            "Invoice 2026-03-05.txt"
        );
        assert_eq!(
            named("a.txt", b"Minutes of the Board Meeting\nHeld on 2026-04-02\n").unwrap(),
            "Meeting minutes 2026-04-02.txt"
        );
        // "Marketing" is not March.
        assert_eq!(
            named("a.txt", b"Receipt\nMarketing 12 2026\n").unwrap(),
            "Receipt.txt"
        );
    }

    #[test]
    fn code_headers_that_say_nothing_are_passed_over() {
        assert!(named("a.c", b"#include <stdio.h>\nint main() {}\n").is_err());
        assert!(named("a.rs", b"// Copyright 2026 Acme Inc.\n// SPDX-License-Identifier: MIT\nfn main() {}\n").is_err());
        assert_eq!(
            named("a.py", b"#!/usr/bin/env python\n# -*- coding: utf-8 -*-\n# Sync the photo library.\nimport os\n").unwrap(),
            "Sync the photo library.py"
        );
    }

    #[test]
    fn a_pdf_bookmark_is_not_its_title() {
        let pdf = b"%PDF-1.4\n3 0 obj\n<< /Title (Chapter 1) /Parent 2 0 R /Dest [4 0 R /Fit] >>\nendobj\n9 0 obj\n<< /Title (Field Guide to Birds) /Producer (x) >>\nendobj\ntrailer\n<< /Root 1 0 R /Info 9 0 R >>\n%%EOF\n";
        assert_eq!(named("scan0003.pdf", pdf).unwrap(), "Field Guide to Birds.pdf");
        let no_info = b"%PDF-1.4\n3 0 obj\n<< /Title (Chapter 1) /Parent 2 0 R /Next 5 0 R >>\nendobj\n";
        assert_ne!(named("scan0004.pdf", no_info).ok().as_deref(), Some("Chapter 1.pdf"));
    }

    #[test]
    fn a_stale_office_title_is_not_trusted() {
        use std::io::Write;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("Document1.docx");
        let mut zip = zip::ZipWriter::new(std::fs::File::create(&path).unwrap());
        let options = zip::write::SimpleFileOptions::default();
        zip.start_file("docProps/core.xml", options).unwrap();
        zip.write_all(b"<cp:coreProperties><dc:title>Company Letterhead Template</dc:title></cp:coreProperties>").unwrap();
        zip.start_file("word/document.xml", options).unwrap();
        zip.write_all(br#"<w:body><w:p><w:pPr><w:pStyle w:val="Title"/></w:pPr><w:r><w:t>Garden Plan 2027</w:t></w:r></w:p></w:body>"#).unwrap();
        zip.finish().unwrap();
        let s = suggest(&path).unwrap();
        assert_eq!((s.name.as_str(), s.source), ("Garden Plan 2027.docx", "Title heading"));
    }

    #[test]
    fn the_same_file_always_gets_the_same_name() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("scan0001.txt");
        std::fs::write(&path, b"Invoice #12\nDate: 2026-02-03\n").unwrap();
        let first = suggest(&path).unwrap();
        for _ in 0..20 {
            assert_eq!(suggest(&path).unwrap(), first);
        }
    }

    /// Found by running Suggest Names over real Downloads and Documents.
    #[test]
    fn lessons_from_real_folders() {
        // A name someone chose is never replaced on its own, even by a
        // document's own title: that title is often stale or a series name.
        let pdf = b"%PDF-1.4\n1 0 obj\n<< /Title (Intro to Statistics) >>\nendobj\n";
        assert_eq!(suggestion("STAT101 L5.pdf", pdf).unwrap().confidence, Confidence::Low);
        assert_eq!(
            suggestion("65bc5616-5141-42e2-ad17-b3ad3fb2895a.txt", b"Experiment 7\n").unwrap().confidence,
            Confidence::Medium
        );
        // Keys, codes and tokens are never names.
        assert!(named("untitled.txt", b"Tq7wmzpa\n").is_err());
        assert!(named("untitled.txt", b"58kd-rhvn\n").is_err());
        assert!(named("untitled.txt", b"Xb4Kq9TzLm2Rv7Np0Wc5Hd8Ys3Fj6Ue1\n").is_err());
        // Cover-page headers name the college or a form field, not the file.
        assert!(named("untitled.txt", b"Northfield Engineering College\n").is_err());
        assert!(named("untitled.txt", b"Department of Computer Science\n").is_err());
        assert!(named("untitled.txt", b"Name: Alex Morgan\n").is_err());
        // Dashes stay dashes; double-encoded punctuation is repaired.
        assert_eq!(
            named("untitled.txt", "MCQ Bank Weeks 1\u{2013}12\n".as_bytes()).unwrap(),
            "MCQ Bank Weeks 1-12.txt"
        );
        assert_eq!(
            named("untitled.md", "# Release \u{e2}\u{20ac}\u{201d} notes fix\n".as_bytes()).unwrap(),
            "Release - notes fix.md"
        );
        // Capitals: acronyms survive, ordinary words don't shout.
        assert_eq!(
            named("untitled.txt", b"UNIT III WIRELESS LAN AND INTERNETWORKING\n").unwrap(),
            "Unit III wireless LAN and internetworking.txt"
        );
        assert_eq!(
            named("untitled.txt", b"SAVE YOUR BACKUP CODES\n").unwrap(),
            "Save your backup codes.txt"
        );
        // A title that is a file name doesn't double the extension.
        assert_eq!(named("untitled.md", b"# AGENTS.md\n").unwrap(), "AGENTS.md");
        // Files without an extension (LICENSE, .git/HEAD) are not documents.
        assert!(named("HEAD", b"ref: refs/heads/master\n").is_err());
        // An encrypted PDF's title is ciphertext.
        let encrypted = b"%PDF-1.6\n1 0 obj\n<< /Title (\xc0\xc6\xe8\xe1\xf1\xd6) >>\nendobj\ntrailer\n<< /Encrypt 5 0 R /Info 1 0 R >>\n";
        assert!(named("scan0001.pdf", encrypted).is_err());
    }

    /// Prints what every file under $SEEK_NAMING_SURVEY would be offered,
    /// for checking suggestions against real folders. Reads only.
    #[test]
    #[ignore]
    fn survey() {
        let Some(root) = std::env::var_os("SEEK_NAMING_SURVEY") else { return };
        let mut stack = vec![std::path::PathBuf::from(root)];
        let mut seen = 0;
        while let Some(dir) = stack.pop() {
            let Ok(entries) = std::fs::read_dir(&dir) else { continue };
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    if stack.len() < 50 {
                        stack.push(path);
                    }
                    continue;
                }
                seen += 1;
                if seen > 400 {
                    return;
                }
                if let Ok(s) = suggest(&path) {
                    println!(
                        "{:?}\t{}\t=> {}\t[{} / {:?}]",
                        path.file_name().unwrap_or_default(),
                        path.parent().and_then(|p| p.file_name()).map(|n| n.to_string_lossy()).unwrap_or_default(),
                        s.name,
                        s.source,
                        s.confidence
                    );
                }
            }
        }
    }

    #[test]
    fn suggestions_take_well_under_50_ms_per_file() {
        let dir = tempfile::tempdir().unwrap();
        let mut paths = vec![];
        for n in 0..200 {
            let path = dir.path().join(format!("scan{n:04}.md"));
            std::fs::write(
                &path,
                format!("# Meeting notes {n}\n\n{}", "Body text line.\n".repeat(400)),
            )
            .unwrap();
            paths.push(path);
        }
        let started = std::time::Instant::now();
        for path in &paths {
            suggest(path).unwrap();
        }
        let each = started.elapsed() / paths.len() as u32;
        eprintln!(
            "suggest names: {:?} per file over {} files",
            each,
            paths.len()
        );
        assert!(each < std::time::Duration::from_millis(50), "{each:?}");
    }
}
