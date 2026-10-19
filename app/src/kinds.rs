#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Kind {
    #[default]
    Any,
    Folder,
    Document,
    Image,
    Movie,
    Audio,
    Code,
    Archive,
}
impl Kind {
    pub const ALL: [Self; 8] = [
        Self::Any,
        Self::Folder,
        Self::Document,
        Self::Image,
        Self::Movie,
        Self::Audio,
        Self::Code,
        Self::Archive,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Self::Any => "Any",
            Self::Folder => "Folder",
            Self::Document => "Document",
            Self::Image => "Image",
            Self::Movie => "Movie",
            Self::Audio => "Audio",
            Self::Code => "Code",
            Self::Archive => "Archive",
        }
    }
    pub fn filter(self) -> &'static str {
        match self {
            Self::Any => "",
            Self::Folder => "kind:folder",
            Self::Document => "type:doc",
            Self::Image => "type:image",
            Self::Movie => "type:video",
            Self::Audio => "type:audio",
            Self::Code => "type:code",
            Self::Archive => "type:archive",
        }
    }
}
pub fn replace_kind(text: &str, filter: &str) -> String {
    let mut text = text.to_string();
    for (range, word) in seek_engine::query::query_words(&text).into_iter().rev() {
        if ["kind:", "type:", "ext:"]
            .iter()
            .any(|p| word.starts_with(p))
        {
            text.replace_range(range, "");
        }
    }
    format!(
        "{}{}{}",
        text.trim(),
        if text.trim().is_empty() || filter.is_empty() {
            ""
        } else {
            " "
        },
        filter
    )
    .trim()
    .to_string()
        + " "
}
pub fn filter_label(filter: &str) -> String {
    if let Some(kind) = Kind::ALL.into_iter().find(|k| k.filter() == filter) {
        return format!("Type: {}", kind.label());
    }
    let label = match filter.strip_prefix("ext:") {
        Some("jpg" | "jpeg" | "jpeg,jpg") => "JPEG image",
        Some("png") => "PNG image",
        Some("pdf") => "PDF document",
        Some("md") => "Markdown document",
        _ => filter.split_once(':').map(|(_, v)| v).unwrap_or(filter),
    };
    format!("Type: {label}")
}
pub fn suggestions(word: &str) -> Vec<(String, String)> {
    let word = word.trim().to_ascii_lowercase();
    if word.is_empty() || word.contains(':') {
        return vec![];
    }
    let mut out = Kind::ALL
        .into_iter()
        .filter(|k| *k != Kind::Any && k.label().to_ascii_lowercase().contains(&word))
        .map(|k| (format!("Type: {}", k.label()), k.filter().into()))
        .collect::<Vec<_>>();
    for (ext, label) in [
        ("jpeg,jpg", "JPEG image"),
        ("png", "PNG image"),
        ("gif", "GIF image"),
        ("pdf", "PDF document"),
        ("docx", "Word document"),
        ("xlsx", "Excel workbook"),
        ("mp4", "MPEG-4 movie"),
        ("mp3", "MP3 audio"),
        ("md", "Markdown document"),
        ("zip", "ZIP archive"),
    ] {
        if ext.contains(&word) || label.to_ascii_lowercase().contains(&word) {
            out.push((format!("Type: {label}"), format!("ext:{ext}")));
        }
    }
    out.truncate(8);
    out
}
pub fn name_contains_query(text: &str) -> String {
    if !text.contains('"') {
        return format!("name:\"{text}\"");
    }
    let mut escaped = String::new();
    for c in text.chars() {
        if c == '"' {
            escaped.push_str("\\x22");
        } else {
            if ".*+?^$()[]{}|\\".contains(c) {
                escaped.push('\\');
            }
            escaped.push(c);
        }
    }
    format!("re:\"{escaped}\"")
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn suggestions_use_real_engine_filters() {
        for k in Kind::ALL {
            assert!(seek_engine::Query::parse(k.filter(), "").is_ok());
        }
        for term in ["jpeg", "jpe", "jpg"] {
            let rows = suggestions(term);
            assert_eq!(rows.len(), 1);
            let query = seek_engine::Query::parse(&rows[0].1, "").unwrap();
            assert!(query.exts.contains(&b"jpeg".to_vec()));
            assert!(query.exts.contains(&b"jpg".to_vec()));
        }
        assert_eq!(
            replace_kind("hello ext:pdf kind:file ", "type:image"),
            "hello type:image "
        );
    }
    #[test]
    fn name_contains_is_literal_and_only_matches_the_filename() {
        let q = seek_engine::Query::parse(&name_contains_query("a+b"), "").unwrap();
        assert!(q.match_path(b"/docs/a+b.txt", 0, 0, 0).is_some());
        assert!(q.match_path(b"/a+b/other.txt", 0, 0, 0).is_none());
        assert!(q.match_path(b"/docs/ab.txt", 0, 0, 0).is_none());
    }
}
