//! Markdown upgrade (Phase 8.4): front matter shown as its own block, remote
//! images held until the reader asks for them, and the format bar's edits.

/// The front matter's `key: value` lines and the body after it. Only a block
/// that opens on the first line with `---` and closes with `---` or `...`.
pub fn front_matter(text: &str) -> Option<(Vec<(String, String)>, &str)> {
    let rest = text
        .strip_prefix("\u{feff}")
        .unwrap_or(text)
        .strip_prefix("---")?;
    let rest = rest.strip_prefix("\r\n").or_else(|| rest.strip_prefix('\n'))?;
    let mut offset = 0;
    let mut fields = vec![];
    for line in rest.split_inclusive('\n') {
        let trimmed = line.trim_end_matches(['\r', '\n']);
        offset += line.len();
        if trimmed == "---" || trimmed == "..." {
            return Some((fields, &rest[offset..]));
        }
        if fields.len() >= 64 || offset > 16 * 1024 {
            return None;
        }
        match trimmed.split_once(':') {
            Some((key, value)) if !key.trim().is_empty() && !key.starts_with([' ', '\t', '-']) => {
                fields.push((key.trim().to_string(), value.trim().to_string()))
            }
            // Nested or list values continue the previous field.
            _ if !trimmed.trim().is_empty() => {
                if let Some((_, value)) = fields.last_mut() {
                    if !value.is_empty() {
                        value.push(' ');
                    }
                    value.push_str(trimmed.trim().trim_start_matches("- "));
                }
            }
            _ => {}
        }
    }
    None
}

/// The text the formatted view shows: without its front matter.
pub fn body(text: &str) -> &str {
    front_matter(text).map_or(text, |(_, body)| body)
}

/// Whether the document points at images on the web.
pub fn has_remote_images(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    lower.contains("](http://")
        || lower.contains("](https://")
        || lower.contains("src=\"http")
        || lower.contains("src='http")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    Bold,
    Italic,
    Code,
    Link,
    Heading,
    Bullet,
    Quote,
}

/// The selection after a format-bar action. Wraps inline marks around the
/// text; prefixes each line for headings, bullets and quotes.
pub fn apply(format: Format, selected: &str) -> String {
    let wrap = |mark: &str| format!("{mark}{selected}{mark}");
    let lines = |prefix: &str| {
        selected
            .split('\n')
            .map(|line| {
                if line.trim().is_empty() {
                    line.to_string()
                } else {
                    format!("{prefix}{line}")
                }
            })
            .collect::<Vec<_>>()
            .join("\n")
    };
    match format {
        Format::Bold => wrap("**"),
        Format::Italic => wrap("*"),
        Format::Code if selected.contains('\n') => format!("```\n{selected}\n```"),
        Format::Code => wrap("`"),
        Format::Link => format!("[{selected}](https://)"),
        Format::Heading => lines("## "),
        Format::Bullet => lines("- "),
        Format::Quote => lines("> "),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn front_matter_is_split_from_the_body() {
        let text = "---\ntitle: Trip notes\ntags:\n  - travel\n  - 2026\ndraft: false\n---\n# Day one\n";
        let (fields, body) = front_matter(text).unwrap();
        assert_eq!(
            fields,
            [
                ("title".into(), "Trip notes".into()),
                ("tags".into(), "travel 2026".into()),
                ("draft".into(), "false".into())
            ]
        );
        assert_eq!(body, "# Day one\n");
        let crlf = "---\r\na: 1\r\n---\r\nbody";
        assert_eq!(super::body(crlf), "body");
        // Not front matter: a rule later in the file, or never closed.
        assert!(front_matter("# Title\n---\na: 1\n---\n").is_none());
        assert!(front_matter("---\na: 1\nno end").is_none());
        assert_eq!(super::body("plain"), "plain");
    }

    #[test]
    fn remote_images_are_detected() {
        assert!(has_remote_images("![x](https://example.com/a.png)"));
        assert!(has_remote_images("<img src=\"http://a/b.png\">"));
        assert!(!has_remote_images("![x](./local.png) and https://plain.link"));
    }

    #[test]
    fn format_bar_wraps_and_prefixes() {
        assert_eq!(apply(Format::Bold, "word"), "**word**");
        assert_eq!(apply(Format::Italic, "word"), "*word*");
        assert_eq!(apply(Format::Code, "x"), "`x`");
        assert_eq!(apply(Format::Code, "a\nb"), "```\na\nb\n```");
        assert_eq!(apply(Format::Link, "Seek"), "[Seek](https://)");
        assert_eq!(apply(Format::Bullet, "a\n\nb"), "- a\n\n- b");
        assert_eq!(apply(Format::Quote, "a"), "> a");
        assert_eq!(apply(Format::Heading, "Title"), "## Title");
        assert_eq!(apply(Format::Bold, ""), "****");
    }
}
