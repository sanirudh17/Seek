//! Streaming, token-preserving JSON and XML pretty-printer for previews and
//! explicit editor formatting (Phase 11.2).
//!
//! - JSON: preserves exact key order, exact number text (`1.00`, `1e+10`, `-0`),
//!   and exact string escapes. Rejects JSON with comments (JSONC) and malformed
//!   input, leaving them raw with an explanatory note.
//! - XML: streams through `quick-xml`, preserving CDATA, comments, processing
//!   instructions, declarations, DOCTYPE, entity references, and `xml:space="preserve"`
//!   subtrees untouched.
//! - Oversized files (> 5 MiB) are never formatted and show raw with a note.
use std::path::Path;

pub const MAX_BYTES: usize = 5 * 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Json,
    Xml,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    Formatted { text: String, was_minified: bool },
    Raw { note: Option<&'static str> },
}

pub fn kind(path: &Path) -> Option<Kind> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    match ext.as_str() {
        "json" | "jsonc" => Some(Kind::Json),
        "xml" | "xaml" | "csproj" | "fsproj" | "vbproj" | "props" | "targets" | "plist" | "rss"
        | "atom" | "svg" => Some(Kind::Xml),
        _ => None,
    }
}

/// Analyze a preview text buffer (off the UI thread for large documents).
/// Returns `None` when `path` is neither JSON nor XML.
pub fn analyze(path: &Path, input: &str) -> Option<Outcome> {
    let kind = kind(path)?;
    Some(analyze_kind(kind, input))
}

pub fn analyze_kind(kind: Kind, input: &str) -> Outcome {
    if input.len() > MAX_BYTES {
        return Outcome::Raw {
            note: Some("Large file (> 5 MB) is shown raw."),
        };
    }
    if input.trim().is_empty() {
        return Outcome::Raw { note: None };
    }
    let result = match kind {
        Kind::Json => format_json(input),
        Kind::Xml => format_xml(input),
    };
    match result {
        Ok(formatted) => {
            let was_minified = is_minified(input, &formatted);
            Outcome::Formatted {
                text: formatted,
                was_minified,
            }
        }
        Err(note) => Outcome::Raw { note: Some(note) },
    }
}

/// Explicit editor command: formats `input` or returns a clear error without
/// touching the document.
pub fn format_for_editor(path: &Path, input: &str) -> Result<String, &'static str> {
    let kind = kind(path).ok_or("Format document is available for JSON and XML files.")?;
    if input.len() > MAX_BYTES {
        return Err("Large file (> 5 MB) cannot be formatted.");
    }
    match kind {
        Kind::Json => format_json(input),
        Kind::Xml => format_xml(input),
    }
}

fn is_minified(raw: &str, formatted: &str) -> bool {
    let raw_trimmed = raw.trim();
    let fmt_trimmed = formatted.trim();
    if raw_trimmed == fmt_trimmed {
        return false;
    }
    let raw_lines: Vec<&str> = raw_trimmed
        .lines()
        .filter(|l| !l.trim().is_empty())
        .collect();
    let fmt_lines = fmt_trimmed.lines().count();
    if raw_lines.len() <= 2 && fmt_lines >= 3 {
        return true;
    }
    let max_line = raw_lines.iter().map(|l| l.len()).max().unwrap_or(0);
    max_line >= 160 && fmt_lines >= raw_lines.len().saturating_mul(2).max(3)
}

// ============================================================================
// Streaming token-preserving JSON re-indenter
// ============================================================================

#[derive(Clone, Copy, PartialEq, Eq)]
enum JsonToken<'a> {
    LBrace,
    RBrace,
    LBracket,
    RBracket,
    Colon,
    Comma,
    String(&'a str),
    Literal(&'a str),
}

struct JsonLexer<'a> {
    src: &'a str,
    bytes: &'a [u8],
    pos: usize,
}

impl<'a> JsonLexer<'a> {
    fn new(src: &'a str) -> Self {
        let mut pos = 0;
        if src.as_bytes().starts_with(&[0xef, 0xbb, 0xbf]) {
            pos = 3;
        }
        Self {
            src,
            bytes: src.as_bytes(),
            pos,
        }
    }

    fn skip_ws(&mut self) -> Result<(), &'static str> {
        while self.pos < self.bytes.len() {
            match self.bytes[self.pos] {
                b' ' | b'\t' | b'\r' | b'\n' => self.pos += 1,
                b'/' if self.pos + 1 < self.bytes.len()
                    && matches!(self.bytes[self.pos + 1], b'/' | b'*') =>
                {
                    return Err("JSON with comments is shown raw.");
                }
                _ => break,
            }
        }
        Ok(())
    }

    fn next_token(&mut self) -> Result<Option<JsonToken<'a>>, &'static str> {
        self.skip_ws()?;
        if self.pos >= self.bytes.len() {
            return Ok(None);
        }
        let b = self.bytes[self.pos];
        match b {
            b'{' => {
                self.pos += 1;
                Ok(Some(JsonToken::LBrace))
            }
            b'}' => {
                self.pos += 1;
                Ok(Some(JsonToken::RBrace))
            }
            b'[' => {
                self.pos += 1;
                Ok(Some(JsonToken::LBracket))
            }
            b']' => {
                self.pos += 1;
                Ok(Some(JsonToken::RBracket))
            }
            b':' => {
                self.pos += 1;
                Ok(Some(JsonToken::Colon))
            }
            b',' => {
                self.pos += 1;
                Ok(Some(JsonToken::Comma))
            }
            b'"' => self.scan_string().map(|s| Some(JsonToken::String(s))),
            b'-' | b'0'..=b'9' => self.scan_number().map(|s| Some(JsonToken::Literal(s))),
            b't' if self.bytes[self.pos..].starts_with(b"true") => {
                let s = &self.src[self.pos..self.pos + 4];
                self.pos += 4;
                Ok(Some(JsonToken::Literal(s)))
            }
            b'f' if self.bytes[self.pos..].starts_with(b"false") => {
                let s = &self.src[self.pos..self.pos + 5];
                self.pos += 5;
                Ok(Some(JsonToken::Literal(s)))
            }
            b'n' if self.bytes[self.pos..].starts_with(b"null") => {
                let s = &self.src[self.pos..self.pos + 4];
                self.pos += 4;
                Ok(Some(JsonToken::Literal(s)))
            }
            _ => Err("Not valid JSON; shown raw."),
        }
    }

    fn scan_string(&mut self) -> Result<&'a str, &'static str> {
        let start = self.pos;
        self.pos += 1; // opening `"`
        while self.pos < self.bytes.len() {
            let b = self.bytes[self.pos];
            if b == b'"' {
                self.pos += 1;
                return Ok(&self.src[start..self.pos]);
            }
            if b < 0x20 {
                return Err("Not valid JSON; shown raw.");
            }
            if b == b'\\' {
                self.pos += 1;
                if self.pos >= self.bytes.len() {
                    return Err("Not valid JSON; shown raw.");
                }
                match self.bytes[self.pos] {
                    b'"' | b'\\' | b'/' | b'b' | b'f' | b'n' | b'r' | b't' => self.pos += 1,
                    b'u' => {
                        if self.pos + 5 > self.bytes.len()
                            || !self.bytes[self.pos + 1..self.pos + 5]
                                .iter()
                                .all(u8::is_ascii_hexdigit)
                        {
                            return Err("Not valid JSON; shown raw.");
                        }
                        self.pos += 5;
                    }
                    _ => return Err("Not valid JSON; shown raw."),
                }
            } else {
                self.pos += 1;
            }
        }
        Err("Not valid JSON; shown raw.")
    }

    fn scan_number(&mut self) -> Result<&'a str, &'static str> {
        let start = self.pos;
        if self.bytes[self.pos] == b'-' {
            self.pos += 1;
        }
        if self.pos >= self.bytes.len() {
            return Err("Not valid JSON; shown raw.");
        }
        if self.bytes[self.pos] == b'0' {
            self.pos += 1;
            if self.pos < self.bytes.len() && self.bytes[self.pos].is_ascii_digit() {
                return Err("Not valid JSON; shown raw.");
            }
        } else if self.bytes[self.pos].is_ascii_digit() {
            while self.pos < self.bytes.len() && self.bytes[self.pos].is_ascii_digit() {
                self.pos += 1;
            }
        } else {
            return Err("Not valid JSON; shown raw.");
        }
        if self.pos < self.bytes.len() && self.bytes[self.pos] == b'.' {
            self.pos += 1;
            if self.pos >= self.bytes.len() || !self.bytes[self.pos].is_ascii_digit() {
                return Err("Not valid JSON; shown raw.");
            }
            while self.pos < self.bytes.len() && self.bytes[self.pos].is_ascii_digit() {
                self.pos += 1;
            }
        }
        if self.pos < self.bytes.len() && matches!(self.bytes[self.pos], b'e' | b'E') {
            self.pos += 1;
            if self.pos < self.bytes.len() && matches!(self.bytes[self.pos], b'+' | b'-') {
                self.pos += 1;
            }
            if self.pos >= self.bytes.len() || !self.bytes[self.pos].is_ascii_digit() {
                return Err("Not valid JSON; shown raw.");
            }
            while self.pos < self.bytes.len() && self.bytes[self.pos].is_ascii_digit() {
                self.pos += 1;
            }
        }
        Ok(&self.src[start..self.pos])
    }
}

fn push_indent(out: &mut String, depth: usize) {
    for _ in 0..depth {
        out.push_str("  ");
    }
}

pub fn format_json(input: &str) -> Result<String, &'static str> {
    let mut lexer = JsonLexer::new(input);
    let mut out = String::with_capacity(input.len() + input.len() / 4);
    let first = lexer
        .next_token()?
        .ok_or("Not valid JSON; shown raw.")?;
    write_json_value(first, &mut lexer, &mut out, 0)?;
    if lexer.next_token()?.is_some() {
        return Err("Not valid JSON; shown raw.");
    }
    Ok(out)
}

fn write_json_value(
    first: JsonToken<'_>,
    lexer: &mut JsonLexer<'_>,
    out: &mut String,
    depth: usize,
) -> Result<(), &'static str> {
    if depth > 256 {
        return Err("JSON nesting is too deep; shown raw.");
    }
    match first {
        JsonToken::String(s) | JsonToken::Literal(s) => {
            out.push_str(s);
            Ok(())
        }
        JsonToken::LBrace => {
            let next = lexer
                .next_token()?
                .ok_or("Not valid JSON; shown raw.")?;
            if next == JsonToken::RBrace {
                out.push_str("{}");
                return Ok(());
            }
            out.push_str("{\n");
            let mut tok = next;
            loop {
                let JsonToken::String(key) = tok else {
                    return Err("Not valid JSON; shown raw.");
                };
                push_indent(out, depth + 1);
                out.push_str(key);
                if lexer.next_token()? != Some(JsonToken::Colon) {
                    return Err("Not valid JSON; shown raw.");
                }
                out.push_str(": ");
                let val = lexer
                    .next_token()?
                    .ok_or("Not valid JSON; shown raw.")?;
                write_json_value(val, lexer, out, depth + 1)?;
                match lexer.next_token()? {
                    Some(JsonToken::Comma) => {
                        out.push_str(",\n");
                        tok = lexer
                            .next_token()?
                            .ok_or("Not valid JSON; shown raw.")?;
                    }
                    Some(JsonToken::RBrace) => {
                        out.push('\n');
                        push_indent(out, depth);
                        out.push('}');
                        return Ok(());
                    }
                    _ => return Err("Not valid JSON; shown raw."),
                }
            }
        }
        JsonToken::LBracket => {
            let next = lexer
                .next_token()?
                .ok_or("Not valid JSON; shown raw.")?;
            if next == JsonToken::RBracket {
                out.push_str("[]");
                return Ok(());
            }
            out.push_str("[\n");
            let mut tok = next;
            loop {
                push_indent(out, depth + 1);
                write_json_value(tok, lexer, out, depth + 1)?;
                match lexer.next_token()? {
                    Some(JsonToken::Comma) => {
                        out.push_str(",\n");
                        tok = lexer
                            .next_token()?
                            .ok_or("Not valid JSON; shown raw.")?;
                    }
                    Some(JsonToken::RBracket) => {
                        out.push('\n');
                        push_indent(out, depth);
                        out.push(']');
                        return Ok(());
                    }
                    _ => return Err("Not valid JSON; shown raw."),
                }
            }
        }
        _ => Err("Not valid JSON; shown raw."),
    }
}

// ============================================================================
// Streaming XML re-indenter preserving CDATA, comments, PIs & xml:space="preserve"
// ============================================================================

#[derive(Clone, Copy)]
struct XmlFrame {
    preserve_space: bool,
    has_child_elements: bool,
    has_non_ws_text: bool,
    opened_multiline: bool,
}

fn xml_space_attr(e: &quick_xml::events::BytesStart<'_>) -> Option<bool> {
    for attr in e.attributes().flatten() {
        if attr.key.as_ref() == b"xml:space" {
            let val = attr.value.as_ref();
            if val == b"preserve" {
                return Some(true);
            }
            if val == b"default" {
                return Some(false);
            }
        }
    }
    None
}

pub fn format_xml(input: &str) -> Result<String, &'static str> {
    use quick_xml::{Reader, events::Event};

    let src = input.strip_prefix('\u{feff}').unwrap_or(input);
    let mut reader = Reader::from_str(src);
    reader.config_mut().trim_text(false);
    reader.config_mut().check_end_names = true;

    let mut out = String::with_capacity(src.len() + src.len() / 4);
    let mut stack: Vec<XmlFrame> = Vec::new();
    let mut saw_root = false;
    let mut root_closed = false;
    // Buffer simple text inside an open element so `<tag>value</tag>` stays inline
    // when no child elements follow.
    let mut pending_text = String::new();

    let flush_pending_multiline = |out: &mut String,
                                   stack: &mut Vec<XmlFrame>,
                                   pending_text: &mut String| {
        let depth = stack.len();
        if let Some(parent) = stack.last_mut()
            && !pending_text.is_empty()
        {
            if !parent.preserve_space {
                let trimmed = pending_text.trim();
                if !trimmed.is_empty() {
                    out.push('\n');
                    push_indent(out, depth);
                    out.push_str(trimmed);
                    parent.has_non_ws_text = true;
                    parent.opened_multiline = true;
                }
            } else {
                out.push_str(pending_text);
                if !pending_text.trim().is_empty() {
                    parent.has_non_ws_text = true;
                }
            }
            pending_text.clear();
        }
    };

    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) => {
                if stack.is_empty() {
                    if root_closed {
                        return Err("Not valid XML; shown raw.");
                    }
                    saw_root = true;
                }
                let parent_preserve = stack.last().is_some_and(|f| f.preserve_space);
                flush_pending_multiline(&mut out, &mut stack, &mut pending_text);
                if let Some(parent) = stack.last_mut() {
                    parent.has_child_elements = true;
                    if !parent_preserve {
                        parent.opened_multiline = true;
                    }
                }
                if !parent_preserve && !out.is_empty() {
                    out.push('\n');
                    push_indent(&mut out, stack.len());
                }
                let raw = std::str::from_utf8(e.as_ref()).map_err(|_| "Not valid XML; shown raw.")?;
                out.push('<');
                out.push_str(raw);
                out.push('>');
                let preserve_space = xml_space_attr(&e).unwrap_or(parent_preserve);
                stack.push(XmlFrame {
                    preserve_space,
                    has_child_elements: false,
                    has_non_ws_text: false,
                    opened_multiline: false,
                });
            }
            Ok(Event::Empty(e)) => {
                if stack.is_empty() {
                    if root_closed {
                        return Err("Not valid XML; shown raw.");
                    }
                    saw_root = true;
                    root_closed = true;
                }
                let parent_preserve = stack.last().is_some_and(|f| f.preserve_space);
                flush_pending_multiline(&mut out, &mut stack, &mut pending_text);
                if let Some(parent) = stack.last_mut() {
                    parent.has_child_elements = true;
                    if !parent_preserve {
                        parent.opened_multiline = true;
                    }
                }
                if !parent_preserve && !out.is_empty() {
                    out.push('\n');
                    push_indent(&mut out, stack.len());
                }
                let raw = std::str::from_utf8(e.as_ref()).map_err(|_| "Not valid XML; shown raw.")?;
                out.push('<');
                out.push_str(raw);
                out.push_str("/>");
            }
            Ok(Event::End(e)) => {
                let Some(frame) = stack.pop() else {
                    return Err("Not valid XML; shown raw.");
                };
                let raw = std::str::from_utf8(e.as_ref()).map_err(|_| "Not valid XML; shown raw.")?;
                if frame.preserve_space {
                    out.push_str(&pending_text);
                    pending_text.clear();
                    out.push_str("</");
                    out.push_str(raw);
                    out.push('>');
                } else if !frame.has_child_elements && !frame.opened_multiline {
                    // Leaf element: keep `<tag>text</tag>` on one line unless text contains newlines.
                    if pending_text.contains('\n') {
                        let trimmed = pending_text.trim();
                        if !trimmed.is_empty() {
                            out.push('\n');
                            push_indent(&mut out, stack.len() + 1);
                            out.push_str(trimmed);
                            out.push('\n');
                            push_indent(&mut out, stack.len());
                        }
                    } else {
                        let trimmed = pending_text.trim();
                        out.push_str(trimmed);
                    }
                    pending_text.clear();
                    out.push_str("</");
                    out.push_str(raw);
                    out.push('>');
                } else {
                    if !pending_text.trim().is_empty() {
                        out.push('\n');
                        push_indent(&mut out, stack.len() + 1);
                        out.push_str(pending_text.trim());
                    }
                    pending_text.clear();
                    out.push('\n');
                    push_indent(&mut out, stack.len());
                    out.push_str("</");
                    out.push_str(raw);
                    out.push('>');
                }
                if stack.is_empty() {
                    root_closed = true;
                }
            }
            Ok(Event::Text(e)) => {
                let raw = std::str::from_utf8(e.as_ref()).map_err(|_| "Not valid XML; shown raw.")?;
                if stack.is_empty() {
                    if !raw.trim().is_empty() {
                        return Err("Not valid XML; shown raw.");
                    }
                } else {
                    pending_text.push_str(raw);
                }
            }
            Ok(Event::GeneralRef(e)) => {
                let raw = std::str::from_utf8(e.as_ref()).map_err(|_| "Not valid XML; shown raw.")?;
                if stack.is_empty() {
                    return Err("Not valid XML; shown raw.");
                }
                pending_text.push('&');
                pending_text.push_str(raw);
                pending_text.push(';');
            }
            Ok(Event::CData(e)) => {
                let parent_preserve = stack.last().is_some_and(|f| f.preserve_space);
                let raw = std::str::from_utf8(e.as_ref()).map_err(|_| "Not valid XML; shown raw.")?;
                if parent_preserve {
                    out.push_str(&pending_text);
                    pending_text.clear();
                    out.push_str("<![CDATA[");
                    out.push_str(raw);
                    out.push_str("]]>");
                } else {
                    flush_pending_multiline(&mut out, &mut stack, &mut pending_text);
                    if let Some(parent) = stack.last_mut() {
                        parent.opened_multiline = true;
                    }
                    if !out.is_empty() {
                        out.push('\n');
                        push_indent(&mut out, stack.len());
                    }
                    out.push_str("<![CDATA[");
                    out.push_str(raw);
                    out.push_str("]]>");
                }
            }
            Ok(Event::Comment(e)) => {
                let parent_preserve = stack.last().is_some_and(|f| f.preserve_space);
                let raw = std::str::from_utf8(e.as_ref()).map_err(|_| "Not valid XML; shown raw.")?;
                if parent_preserve {
                    out.push_str(&pending_text);
                    pending_text.clear();
                } else {
                    flush_pending_multiline(&mut out, &mut stack, &mut pending_text);
                    if let Some(parent) = stack.last_mut() {
                        parent.opened_multiline = true;
                    }
                    if !out.is_empty() {
                        out.push('\n');
                        push_indent(&mut out, stack.len());
                    }
                }
                out.push_str("<!--");
                out.push_str(raw);
                out.push_str("-->");
            }
            Ok(Event::Decl(e)) => {
                let raw = std::str::from_utf8(e.as_ref()).map_err(|_| "Not valid XML; shown raw.")?;
                if !out.is_empty() {
                    out.push('\n');
                }
                out.push_str("<?");
                out.push_str(raw);
                out.push_str("?>");
            }
            Ok(Event::PI(e)) => {
                let parent_preserve = stack.last().is_some_and(|f| f.preserve_space);
                let raw = std::str::from_utf8(e.as_ref()).map_err(|_| "Not valid XML; shown raw.")?;
                if parent_preserve {
                    out.push_str(&pending_text);
                    pending_text.clear();
                } else {
                    flush_pending_multiline(&mut out, &mut stack, &mut pending_text);
                    if let Some(parent) = stack.last_mut() {
                        parent.opened_multiline = true;
                    }
                    if !out.is_empty() {
                        out.push('\n');
                        push_indent(&mut out, stack.len());
                    }
                }
                out.push_str("<?");
                out.push_str(raw);
                out.push_str("?>");
            }
            Ok(Event::DocType(e)) => {
                let raw = std::str::from_utf8(e.as_ref()).map_err(|_| "Not valid XML; shown raw.")?;
                if !out.is_empty() {
                    out.push('\n');
                }
                out.push_str("<!DOCTYPE");
                out.push_str(raw);
                out.push('>');
            }
            Ok(Event::Eof) => break,
            Err(_) => return Err("Not valid XML; shown raw."),
        }
    }

    if !saw_root || !stack.is_empty() {
        return Err("Not valid XML; shown raw.");
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_preserves_key_order_numbers_escapes_and_is_idempotent() {
        let raw = r#"{"z":1.00,"a":"caf\u00e9\/bar","n":-0,"big":1e+10,"empty":{},"arr":[1,2,[]]}"#;
        let Outcome::Formatted { text, was_minified } =
            analyze(Path::new("config.json"), raw).unwrap()
        else {
            panic!("expected formatted json");
        };
        assert!(was_minified);
        // Exact parsed data equality
        let parsed_raw: serde_json::Value = serde_json::from_str(raw).unwrap();
        let parsed_fmt: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(parsed_raw, parsed_fmt);
        // Exact key order, number text, and escape preservation
        assert!(text.find("\"z\"").unwrap() < text.find("\"a\"").unwrap());
        assert!(text.contains("1.00"));
        assert!(text.contains(r#""caf\u00e9\/bar""#));
        assert!(text.contains("-0"));
        assert!(text.contains("1e+10"));
        assert!(text.contains("{}"));
        assert!(text.contains("[]"));
        // Idempotent
        let again = format_json(&text).unwrap();
        assert_eq!(text, again);
    }

    #[test]
    fn json_with_comments_malformed_and_oversized_stay_raw() {
        let jsonc = r#"{"a": 1 /* comment */}"#;
        assert_eq!(
            analyze(Path::new("a.json"), jsonc),
            Some(Outcome::Raw {
                note: Some("JSON with comments is shown raw."),
            })
        );
        let line_comment = "// hi\n{\"a\": 1}";
        assert_eq!(
            analyze(Path::new("a.jsonc"), line_comment),
            Some(Outcome::Raw {
                note: Some("JSON with comments is shown raw."),
            })
        );
        let invalid = r#"{"a": 1,}"#;
        assert_eq!(
            analyze(Path::new("a.json"), invalid),
            Some(Outcome::Raw {
                note: Some("Not valid JSON; shown raw."),
            })
        );
        assert!(format_for_editor(Path::new("a.json"), invalid).is_err());

        let huge = format!("[{}]", "1,".repeat(MAX_BYTES / 2 + 10));
        assert_eq!(
            analyze(Path::new("huge.json"), &huge),
            Some(Outcome::Raw {
                note: Some("Large file (> 5 MB) is shown raw."),
            })
        );
    }

    #[test]
    fn xml_preserves_cdata_comments_pi_entities_and_xml_space_preserve() {
        let raw = concat!(
            r#"<?xml version="1.0" encoding="UTF-8"?>"#,
            r#"<?app hint="1"?>"#,
            r#"<!-- lead comment -->"#,
            r#"<root><item id="1">hello &amp; world</item>"#,
            r#"<raw><![CDATA[  keep <raw> & text  ]]></raw>"#,
            "<pre xml:space=\"preserve\">  line 1\n    <span>  keep  </span>\n</pre>",
            r#"<empty/></root>"#
        );
        let Outcome::Formatted { text, was_minified } =
            analyze(Path::new("data.xml"), raw).unwrap()
        else {
            panic!("expected formatted xml");
        };
        assert!(was_minified);
        assert!(text.contains(r#"<?xml version="1.0" encoding="UTF-8"?>"#));
        assert!(text.contains(r#"<?app hint="1"?>"#));
        assert!(text.contains("<!-- lead comment -->"));
        assert!(text.contains("<![CDATA[  keep <raw> & text  ]]>"));
        assert!(text.contains("hello &amp; world"));
        assert!(text.contains("<pre xml:space=\"preserve\">  line 1\n    <span>  keep  </span>\n</pre>"));
        assert!(text.contains("<empty/>"));
        // Idempotent
        let again = format_xml(&text).unwrap();
        assert_eq!(text, again);
    }

    #[test]
    fn malformed_xml_stays_raw_and_editor_format_fails_safely() {
        let bad = "<root><unclosed></root>";
        assert_eq!(
            analyze(Path::new("bad.xml"), bad),
            Some(Outcome::Raw {
                note: Some("Not valid XML; shown raw."),
            })
        );
        assert!(format_for_editor(Path::new("bad.xml"), bad).is_err());
    }
}
