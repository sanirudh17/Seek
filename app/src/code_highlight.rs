//! Small lexical highlighter for common local text formats; unknown languages stay plain.
use gpui::{Context, HighlightStyle, SharedString, Window};
use gpui_component::input::{FoldRange, HighlightStyleResolver, InputEdit, InputHighlighter, Rope};
use std::ops::Range;
pub fn language(path: &std::path::Path) -> &'static str {
    match path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase()
        .as_str()
    {
        "rs" => "rust",
        "js" | "jsx" | "ts" | "tsx" => "javascript",
        "json" | "jsonc" => "json",
        "py" => "python",
        "c" | "h" | "cpp" | "hpp" | "cs" | "java" => "c",
        "sh" | "bash" | "ps1" => "shell",
        "toml" | "yaml" | "yml" => "config",
        "md" | "markdown" => "markdown",
        _ => "text",
    }
}
pub fn tokens(text: &str, language: &str) -> Vec<(Range<usize>, &'static str)> {
    if language == "text" || language == "markdown" {
        return vec![];
    }
    let bytes = text.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    let hashes = matches!(language, "python" | "shell" | "config");
    let words = match language {
        "rust" => {
            "fn let mut pub use impl struct enum trait match if else for while loop in return Some None Self self const static mod crate super async await move where as true false"
        }
        "javascript" => {
            "const let var function class extends new if else for while of in return import from export default async await try catch throw true false null undefined"
        }
        "python" => {
            "def class if else elif for while in return import from as with async await try except raise pass lambda yield True False None and or not"
        }
        "c" => {
            "int char void bool long double float struct class public private static const if else for while return switch case break new using namespace true false null"
        }
        "json" => "true false null",
        "shell" => "if then else fi for do done function param return switch foreach true false",
        _ => "true false null",
    };
    while i < bytes.len() {
        let start = i;
        if hashes && bytes[i] == b'#'
            || !hashes && language != "json" && text[i..].starts_with("//")
        {
            i = text[i..].find('\n').map_or(text.len(), |n| i + n);
            out.push((start..i, "comment"));
        } else if !hashes && language != "json" && text[i..].starts_with("/*") {
            i = text[i + 2..]
                .find("*/")
                .map_or(text.len(), |n| i + 2 + n + 2);
            out.push((start..i, "comment"));
        } else if matches!(bytes[i], b'"' | b'\'' | b'`') {
            let quote = bytes[i];
            // Rust lifetimes are identifiers, not an unterminated character literal.
            if language == "rust"
                && quote == b'\''
                && !text[i + 1..]
                    .split('\n')
                    .next()
                    .unwrap_or("")
                    .contains('\'')
            {
                i += 1;
                continue;
            }
            i += 1;
            while i < bytes.len() {
                if bytes[i] == b'\\' {
                    i += 1;
                    if i < bytes.len() {
                        i += text[i..].chars().next().unwrap().len_utf8();
                    }
                } else if bytes[i] == quote {
                    i += 1;
                    break;
                } else {
                    i += text[i..].chars().next().unwrap().len_utf8();
                }
            }
            out.push((start..i, "string"));
        } else if bytes[i].is_ascii_digit() {
            i += 1;
            while i < bytes.len()
                && (bytes[i].is_ascii_alphanumeric() || matches!(bytes[i], b'.' | b'_'))
            {
                i += 1;
            }
            out.push((start..i, "number"));
        } else if bytes[i].is_ascii_alphabetic() || bytes[i] == b'_' {
            i += 1;
            while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                i += 1;
            }
            if words.split_whitespace().any(|word| word == &text[start..i]) {
                out.push((start..i, "keyword"));
            }
        } else {
            i += text[i..].chars().next().unwrap().len_utf8();
        }
    }
    out
}
pub struct Highlighter {
    language: SharedString,
    tokens: Vec<(Range<usize>, &'static str)>,
}
impl Highlighter {
    pub fn new(language: &str) -> Self {
        Self {
            language: language.to_owned().into(),
            tokens: vec![],
        }
    }
}
impl InputHighlighter for Highlighter {
    fn language(&self) -> SharedString {
        self.language.clone()
    }
    fn update(
        &mut self,
        _: Option<InputEdit>,
        text: &Rope,
        _: bool,
        _: &mut Window,
        _: &mut Context<gpui_component::input::EditorState>,
    ) {
        self.tokens = tokens(&text.to_string(), &self.language);
    }
    fn styles(
        &self,
        range: &Range<usize>,
        resolver: &dyn HighlightStyleResolver,
    ) -> Vec<(Range<usize>, HighlightStyle)> {
        let mut runs = vec![];
        let mut position = range.start;
        for (token, name) in &self.tokens {
            let start = token.start.max(range.start);
            let end = token.end.min(range.end);
            if start >= end {
                continue;
            }
            if position < start {
                runs.push((position..start, HighlightStyle::default()));
            }
            runs.push((start..end, resolver.style(name).unwrap_or_default()));
            position = end;
        }
        if position < range.end {
            runs.push((position..range.end, HighlightStyle::default()));
        }
        runs
    }
    fn fold_ranges(&self, _: &Rope) -> Vec<FoldRange> {
        vec![]
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unicode_strings_comments_and_keywords_have_ordered_valid_boundaries() {
        let text = "pub fn main() { let x = \"\u{4e2d}\u{6587}\"; // note\nlet y = 42; }";
        let spans = tokens(text, "rust");
        assert!(
            spans
                .iter()
                .any(|(r, n)| *n == "string" && text[r.clone()].contains('\u{4e2d}'))
        );
        assert!(
            spans
                .iter()
                .any(|(r, n)| *n == "comment" && &text[r.clone()] == "// note")
        );
        let mut end = 0;
        for (r, _) in spans {
            assert!(
                r.start >= end
                    && r.end <= text.len()
                    && text.is_char_boundary(r.start)
                    && text.is_char_boundary(r.end)
            );
            end = r.end;
        }
    }
    #[test]
    fn plain_text_and_unknown_files_are_not_misrepresented_as_parsed_code() {
        assert_eq!(language(std::path::Path::new("owned.weird")), "text");
        assert!(tokens("fn \"example\"", "text").is_empty());
        assert_eq!(tokens("true 42 \"text\"", "json").len(), 3);
    }
}
