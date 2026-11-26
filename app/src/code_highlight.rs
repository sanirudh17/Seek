//! Palette-aware lexical syntax highlighting for Seek's previews and editor.
//!
//! Covers 25 languages across eight semantic roles (`keyword`, `string`,
//! `number`, `comment`, `function`, `type`, `constant`, `punctuation`).
//! Highlights only the requested visible range (with a bounded lookback for
//! block comments) and caps work on very long lines so scrolling and opening
//! large files never stalls.
use gpui::{Context, HighlightStyle, SharedString, Window};
use gpui_base::input::{FoldRange, HighlightStyleResolver, InputEdit, InputHighlighter, RopeExt};
use gpui_component::input::Rope;
use std::{ops::Range, path::Path};

/// Skip tokenising any single line longer than this (e.g. raw minified blobs).
const MAX_HIGHLIGHT_LINE_BYTES: usize = 8 * 1024;
/// Skip highlighting if a caller asks for an unbounded giant slice at once.
const MAX_HIGHLIGHT_RANGE_BYTES: usize = 128 * 1024;
/// Maximum bytes scanned backwards to detect an open multi-line comment.
const LOOKBACK_BYTES: usize = 16 * 1024;

pub fn language(path: &Path) -> &'static str {
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    if is_env_name(&name) {
        return "env";
    }
    if name == "dockerfile" || name.starts_with("dockerfile.") {
        return "dockerfile";
    }
    if matches!(name.as_str(), "gemfile" | "rakefile") {
        return "ruby";
    }
    if matches!(name.as_str(), ".gitconfig" | ".editorconfig") {
        return "ini";
    }
    if matches!(name.as_str(), ".gitignore" | ".gitattributes") {
        return "shell";
    }
    match path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase()
        .as_str()
    {
        "rs" => "rust",
        "js" | "jsx" | "mjs" | "cjs" | "ts" | "tsx" | "mts" | "cts" => "js",
        "json" | "jsonc" => "json",
        "py" => "python",
        "c" | "cc" | "cpp" | "cxx" | "h" | "hpp" | "hxx" => "c",
        "cs" => "csharp",
        "java" => "java",
        "go" => "go",
        "kt" | "kts" => "kotlin",
        "swift" => "swift",
        "rb" | "rake" | "gemspec" => "ruby",
        "php" | "phtml" => "php",
        "lua" => "lua",
        "ps1" | "psm1" | "psd1" => "powershell",
        "sh" | "bash" | "zsh" | "fish" | "bat" | "cmd" => "shell",
        "sql" => "sql",
        "html" | "htm" | "xhtml" | "vue" | "svelte" => "html",
        "xml" | "svg" | "xaml" | "csproj" | "fsproj" | "vbproj" | "props" | "targets" | "plist"
        | "rss" | "atom" => "xml",
        "css" | "scss" | "sass" | "less" => "css",
        "dockerfile" => "dockerfile",
        "toml" | "yaml" | "yml" => "config",
        "ini" | "conf" | "cfg" | "properties" => "ini",
        "diff" | "patch" => "diff",
        "env" => "env",
        "md" | "markdown" => "markdown",
        _ => "text",
    }
}

pub fn is_env_name(lower_name: &str) -> bool {
    lower_name == ".env" || lower_name.starts_with(".env.") || lower_name.ends_with(".env")
}

/// Highlight a plain `&str` using the active palette's 8 roles (for info panel
/// and compact previews, guaranteeing identical colours to the editor).
pub fn highlight_text(
    language: &str,
    text: &str,
    syntax: &crate::appearance::SyntaxPalette,
) -> Vec<(Range<usize>, HighlightStyle)> {
    if text.is_empty() || language == "text" || text.len() > MAX_HIGHLIGHT_RANGE_BYTES {
        return vec![];
    }
    let mut out = vec![];
    let mut state = BlockState::Normal;
    let mut offset = 0usize;
    for line in text.split_inclusive('\n') {
        let len = line.len();
        let trimmed = line.trim_end_matches(['\r', '\n']);
        if trimmed.len() <= MAX_HIGHLIGHT_LINE_BYTES {
            let mut spans = vec![];
            state = tokenize_line(language, trimmed, state, &mut spans);
            for (r, role) in spans {
                if let Some(style) = syntax.style(role) {
                    out.push((offset + r.start..offset + r.end, style));
                }
            }
        } else {
            state = advance_block_state(language, trimmed, state);
        }
        offset += len;
    }
    out
}

pub struct Highlighter {
    language: SharedString,
    text: Rope,
}

impl Highlighter {
    pub fn new(language: &str) -> Self {
        Self {
            language: language.to_owned().into(),
            text: Rope::new(),
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
        _: &mut Context<gpui_base::input::EditorState>,
    ) {
        self.text = text.clone();
    }

    fn styles(
        &self,
        range: &Range<usize>,
        resolver: &dyn HighlightStyleResolver,
    ) -> Vec<(Range<usize>, HighlightStyle)> {
        let total = self.text.len();
        let start = range.start.min(total);
        let end = range.end.min(total);
        if start >= end
            || self.language.as_ref() == "text"
            || end.saturating_sub(start) > MAX_HIGHLIGHT_RANGE_BYTES
        {
            return vec![(start..end, HighlightStyle::default())];
        }

        let start_line = self.text.offset_to_point(start).row;
        let end_line = self
            .text
            .offset_to_point(end.saturating_sub(1))
            .row
            .min(self.text.lines_len().saturating_sub(1));

        // Detect whether we start inside a multi-line comment using a bounded lookback.
        let mut block_state = initial_block_state(self.language.as_ref(), &self.text, start_line);

        let mut raw_spans: Vec<(Range<usize>, &'static str)> = vec![];
        for line_idx in start_line..=end_line {
            let line_start = self.text.line_start_offset(line_idx);
            let line_str = self.text.slice_line(line_idx).to_string();
            let trimmed = line_str.trim_end_matches(['\r', '\n']);
            if trimmed.len() <= MAX_HIGHLIGHT_LINE_BYTES {
                let mut line_spans = vec![];
                block_state =
                    tokenize_line(self.language.as_ref(), trimmed, block_state, &mut line_spans);
                for (r, role) in line_spans {
                    let s = (line_start + r.start).max(start);
                    let e = (line_start + r.end).min(end);
                    if s < e {
                        raw_spans.push((s..e, role));
                    }
                }
            } else {
                block_state = advance_block_state(self.language.as_ref(), trimmed, block_state);
            }
        }

        // Fill gaps with default style so the returned runs cover `start..end` completely.
        let mut out = Vec::with_capacity(raw_spans.len() * 2 + 1);
        let mut cursor = start;
        for (span, role) in raw_spans {
            if span.start > cursor {
                out.push((cursor..span.start, HighlightStyle::default()));
            }
            let s = span.start.max(cursor);
            if s < span.end {
                let style = resolver.style(role).unwrap_or_default();
                out.push((s..span.end, style));
                cursor = span.end;
            }
        }
        if cursor < end {
            out.push((cursor..end, HighlightStyle::default()));
        }
        if out.is_empty() {
            out.push((start..end, HighlightStyle::default()));
        }
        out
    }

    fn fold_ranges(&self, _: &Rope) -> Vec<FoldRange> {
        vec![]
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum BlockState {
    Normal,
    CComment,
    HtmlComment,
    PsComment,
    LuaComment,
}

fn supports_block_comment(lang: &str) -> Option<(&'static str, &'static str, BlockState)> {
    match lang {
        "rust" | "js" | "json" | "c" | "csharp" | "java" | "go" | "kotlin" | "swift" | "php"
        | "css" | "sql" => Some(("/*", "*/", BlockState::CComment)),
        "html" | "xml" | "markdown" => Some(("<!--", "-->", BlockState::HtmlComment)),
        "powershell" => Some(("<#", "#>", BlockState::PsComment)),
        "lua" => Some(("--[[", "]]", BlockState::LuaComment)),
        _ => None,
    }
}

fn initial_block_state(lang: &str, rope: &Rope, start_line: usize) -> BlockState {
    if start_line == 0 || supports_block_comment(lang).is_none() {
        return BlockState::Normal;
    }
    let start_byte = rope.line_start_offset(start_line);
    let lookback_start = start_byte.saturating_sub(LOOKBACK_BYTES);
    let first_line = rope.offset_to_point(lookback_start).row;
    let mut state = BlockState::Normal;
    for line_idx in first_line..start_line {
        let line = rope.slice_line(line_idx).to_string();
        state = advance_block_state(lang, line.trim_end_matches(['\r', '\n']), state);
    }
    state
}

fn advance_block_state(lang: &str, line: &str, mut state: BlockState) -> BlockState {
    let Some((open, close, kind)) = supports_block_comment(lang) else {
        return BlockState::Normal;
    };
    let bytes = line.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if state == kind {
            if bytes[i..].starts_with(close.as_bytes()) {
                state = BlockState::Normal;
                i += close.len();
                continue;
            }
            i += 1;
            continue;
        }
        // Skip quoted strings so `/*` inside a string does not open a comment.
        if bytes[i] == b'"' || bytes[i] == b'\'' {
            let q = bytes[i];
            i += 1;
            while i < bytes.len() {
                if bytes[i] == b'\\' && i + 1 < bytes.len() {
                    i += 2;
                    continue;
                }
                if bytes[i] == q {
                    i += 1;
                    break;
                }
                i += 1;
            }
            continue;
        }
        if matches!(
            lang,
            "rust" | "js" | "json" | "c" | "csharp" | "java" | "go" | "kotlin" | "swift" | "php"
        ) && bytes[i..].starts_with(b"//")
        {
            break;
        }
        if bytes[i..].starts_with(open.as_bytes()) {
            state = kind;
            i += open.len();
            continue;
        }
        i += 1;
    }
    state
}

fn tokenize_line(
    lang: &str,
    line: &str,
    state: BlockState,
    out: &mut Vec<(Range<usize>, &'static str)>,
) -> BlockState {
    match lang {
        "diff" => {
            tokenize_diff(line, out);
            BlockState::Normal
        }
        "env" => {
            tokenize_env(line, out);
            BlockState::Normal
        }
        "ini" => {
            tokenize_ini(line, out);
            BlockState::Normal
        }
        "dockerfile" => {
            tokenize_dockerfile(line, out);
            BlockState::Normal
        }
        "html" | "xml" => tokenize_markup(line, state, out),
        "css" => tokenize_css(line, state, out),
        "markdown" => tokenize_markdown(line, state, out),
        "json" => tokenize_json(line, state, out),
        "config" => {
            tokenize_config(line, out);
            BlockState::Normal
        }
        _ => tokenize_code(lang, line, state, out),
    }
}

fn tokenize_diff(line: &str, out: &mut Vec<(Range<usize>, &'static str)>) {
    if line.is_empty() {
        return;
    }
    if line.starts_with("@@") {
        let end = line[2..]
            .find("@@")
            .map(|i| i + 4)
            .unwrap_or(line.len());
        out.push((0..end, "number"));
        if end < line.len() {
            out.push((end..line.len(), "comment"));
        }
    } else if line.starts_with("+++")
        || line.starts_with("---")
        || line.starts_with("diff ")
        || line.starts_with("index ")
    {
        out.push((0..line.len(), "comment"));
    } else if line.starts_with('+') {
        out.push((0..line.len(), "string"));
    } else if line.starts_with('-') {
        out.push((0..line.len(), "constant"));
    }
}

fn tokenize_env(line: &str, out: &mut Vec<(Range<usize>, &'static str)>) {
    let bytes = line.as_bytes();
    let mut i = 0;
    while i < bytes.len() && matches!(bytes[i], b' ' | b'\t') {
        i += 1;
    }
    if i >= bytes.len() {
        return;
    }
    if bytes[i] == b'#' {
        out.push((i..bytes.len(), "comment"));
        return;
    }
    if line[i..].starts_with("export ") || line[i..].starts_with("export\t") {
        out.push((i..i + 6, "keyword"));
        i += 6;
        while i < bytes.len() && matches!(bytes[i], b' ' | b'\t') {
            i += 1;
        }
    }
    if let Some(eq_rel) = line[i..].find('=') {
        let eq = i + eq_rel;
        let key_end = line[i..eq].trim_end().len() + i;
        if key_end > i {
            out.push((i..key_end, "function"));
        }
        out.push((eq..eq + 1, "punctuation"));
        let val_start = eq + 1;
        if val_start < bytes.len() {
            // Preserve trailing inline comments on unquoted values.
            let rest = &line[val_start..];
            let trimmed_start = val_start + (rest.len() - rest.trim_start().len());
            if trimmed_start < bytes.len() {
                let first = bytes[trimmed_start];
                if first == b'"' || first == b'\'' {
                    let end = scan_string(bytes, trimmed_start);
                    out.push((trimmed_start..end, "string"));
                    if let Some(hash) = line[end..].find('#') {
                        out.push((end + hash..bytes.len(), "comment"));
                    }
                } else if let Some(comment_rel) = rest.find(" #") {
                    let comment_pos = val_start + comment_rel + 1;
                    let val_end = line[trimmed_start..comment_pos].trim_end().len() + trimmed_start;
                    if val_end > trimmed_start {
                        out.push((trimmed_start..val_end, "string"));
                    }
                    out.push((comment_pos..bytes.len(), "comment"));
                } else {
                    out.push((trimmed_start..bytes.len(), "string"));
                }
            }
        }
    }
}

fn tokenize_ini(line: &str, out: &mut Vec<(Range<usize>, &'static str)>) {
    let bytes = line.as_bytes();
    let mut i = 0;
    while i < bytes.len() && matches!(bytes[i], b' ' | b'\t') {
        i += 1;
    }
    if i >= bytes.len() {
        return;
    }
    if matches!(bytes[i], b'#' | b';') {
        out.push((i..bytes.len(), "comment"));
        return;
    }
    if bytes[i] == b'[' {
        if let Some(close) = line[i..].find(']') {
            out.push((i..i + 1, "punctuation"));
            if close > 1 {
                out.push((i + 1..i + close, "type"));
            }
            out.push((i + close..i + close + 1, "punctuation"));
            return;
        }
    }
    if let Some(sep_rel) = line[i..].find(['=', ':']) {
        let sep = i + sep_rel;
        let key_end = line[i..sep].trim_end().len() + i;
        if key_end > i {
            out.push((i..key_end, "keyword"));
        }
        out.push((sep..sep + 1, "punctuation"));
        let val_start = sep + 1;
        let rest = &line[val_start..];
        let trimmed_start = val_start + (rest.len() - rest.trim_start().len());
        if trimmed_start < bytes.len() {
            let val = line[trimmed_start..].trim_end();
            let val_end = trimmed_start + val.len();
            let role = if matches!(
                val.to_ascii_lowercase().as_str(),
                "true" | "false" | "yes" | "no" | "on" | "off"
            ) {
                "constant"
            } else if !val.is_empty()
                && val
                    .bytes()
                    .all(|b| b.is_ascii_digit() || b == b'.' || b == b'-')
                && val.bytes().any(|b| b.is_ascii_digit())
            {
                "number"
            } else {
                "string"
            };
            out.push((trimmed_start..val_end, role));
        }
    }
}

fn tokenize_dockerfile(line: &str, out: &mut Vec<(Range<usize>, &'static str)>) {
    let bytes = line.as_bytes();
    let mut i = 0;
    while i < bytes.len() && matches!(bytes[i], b' ' | b'\t') {
        i += 1;
    }
    if i >= bytes.len() {
        return;
    }
    if bytes[i] == b'#' {
        out.push((i..bytes.len(), "comment"));
        return;
    }
    let word_start = i;
    while i < bytes.len() && bytes[i].is_ascii_alphabetic() {
        i += 1;
    }
    let word = &line[word_start..i];
    if matches!(
        word.to_ascii_uppercase().as_str(),
        "FROM"
            | "RUN"
            | "CMD"
            | "LABEL"
            | "MAINTAINER"
            | "EXPOSE"
            | "ENV"
            | "ADD"
            | "COPY"
            | "ENTRYPOINT"
            | "VOLUME"
            | "USER"
            | "WORKDIR"
            | "ARG"
            | "ONBUILD"
            | "STOPSIGNAL"
            | "HEALTHCHECK"
            | "SHELL"
    ) {
        out.push((word_start..i, "keyword"));
    } else {
        i = word_start;
    }
    while i < bytes.len() {
        match bytes[i] {
            b'"' | b'\'' => {
                let end = scan_string(bytes, i);
                out.push((i..end, "string"));
                i = end;
            }
            b'$' => {
                let start = i;
                i += 1;
                if i < bytes.len() && bytes[i] == b'{' {
                    while i < bytes.len() && bytes[i] != b'}' {
                        i += 1;
                    }
                    if i < bytes.len() {
                        i += 1;
                    }
                } else {
                    while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_')
                    {
                        i += 1;
                    }
                }
                out.push((start..i, "constant"));
            }
            b'-' if i + 1 < bytes.len() && bytes[i + 1] == b'-' => {
                let start = i;
                i += 2;
                while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'-') {
                    i += 1;
                }
                out.push((start..i, "function"));
            }
            b if b.is_ascii_digit()
                && (i == 0 || !bytes[i - 1].is_ascii_alphanumeric() && bytes[i - 1] != b'_') =>
            {
                let end = scan_number(bytes, i);
                out.push((i..end, "number"));
                i = end;
            }
            b if is_word_start(b) => {
                let start = i;
                while i < bytes.len() && is_word_continue(bytes[i]) {
                    i += 1;
                }
                if line[start..i].eq_ignore_ascii_case("AS") {
                    out.push((start..i, "keyword"));
                }
            }
            b if is_punct(b) => {
                out.push((i..i + 1, "punctuation"));
                i += 1;
            }
            _ => i += 1,
        }
    }
}

fn tokenize_markup(
    line: &str,
    mut state: BlockState,
    out: &mut Vec<(Range<usize>, &'static str)>,
) -> BlockState {
    let bytes = line.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if state == BlockState::HtmlComment {
            if let Some(rel) = line[i..].find("-->") {
                let end = i + rel + 3;
                out.push((i..end, "comment"));
                i = end;
                state = BlockState::Normal;
            } else {
                out.push((i..bytes.len(), "comment"));
                return state;
            }
            continue;
        }
        if bytes[i..].starts_with(b"<!--") {
            let start = i;
            i += 4;
            if let Some(rel) = line[i..].find("-->") {
                let end = i + rel + 3;
                out.push((start..end, "comment"));
                i = end;
            } else {
                out.push((start..bytes.len(), "comment"));
                return BlockState::HtmlComment;
            }
            continue;
        }
        if bytes[i..].starts_with(b"<!") || bytes[i..].starts_with(b"<?") {
            let start = i;
            let end = line[i..]
                .find('>')
                .map(|r| i + r + 1)
                .unwrap_or(bytes.len());
            out.push((start..end, "keyword"));
            i = end;
            continue;
        }
        if bytes[i] == b'<' {
            let open_start = i;
            i += 1;
            if i < bytes.len() && bytes[i] == b'/' {
                i += 1;
            }
            out.push((open_start..i, "punctuation"));
            // Tag name
            let name_start = i;
            while i < bytes.len()
                && (bytes[i].is_ascii_alphanumeric()
                    || matches!(bytes[i], b'-' | b'_' | b':' | b'.'))
            {
                i += 1;
            }
            if i > name_start {
                out.push((name_start..i, "type"));
            }
            // Attributes until `>` or end of line
            while i < bytes.len() {
                if bytes[i] == b'>' {
                    out.push((i..i + 1, "punctuation"));
                    i += 1;
                    break;
                }
                if bytes[i..].starts_with(b"/>") {
                    out.push((i..i + 2, "punctuation"));
                    i += 2;
                    break;
                }
                if bytes[i] == b'"' || bytes[i] == b'\'' {
                    let end = scan_string(bytes, i);
                    out.push((i..end, "string"));
                    i = end;
                    continue;
                }
                if bytes[i] == b'=' {
                    out.push((i..i + 1, "punctuation"));
                    i += 1;
                    continue;
                }
                if bytes[i].is_ascii_alphabetic() || matches!(bytes[i], b'_' | b':' | b'@' | b'#')
                {
                    let attr_start = i;
                    while i < bytes.len()
                        && (bytes[i].is_ascii_alphanumeric()
                            || matches!(bytes[i], b'-' | b'_' | b':' | b'.' | b'@' | b'#'))
                    {
                        i += 1;
                    }
                    out.push((attr_start..i, "function"));
                    continue;
                }
                i += 1;
            }
            continue;
        }
        if bytes[i] == b'&' {
            let start = i;
            if let Some(semi) = line[i..].find(';') {
                if semi > 1 && semi <= 12 {
                    out.push((start..start + semi + 1, "constant"));
                    i = start + semi + 1;
                    continue;
                }
            }
        }
        i += 1;
    }
    state
}

fn tokenize_css(
    line: &str,
    mut state: BlockState,
    out: &mut Vec<(Range<usize>, &'static str)>,
) -> BlockState {
    let bytes = line.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if state == BlockState::CComment {
            if let Some(rel) = line[i..].find("*/") {
                let end = i + rel + 2;
                out.push((i..end, "comment"));
                i = end;
                state = BlockState::Normal;
            } else {
                out.push((i..bytes.len(), "comment"));
                return state;
            }
            continue;
        }
        if bytes[i..].starts_with(b"/*") {
            let start = i;
            i += 2;
            if let Some(rel) = line[i..].find("*/") {
                let end = i + rel + 2;
                out.push((start..end, "comment"));
                i = end;
            } else {
                out.push((start..bytes.len(), "comment"));
                return BlockState::CComment;
            }
            continue;
        }
        if bytes[i..].starts_with(b"//") {
            out.push((i..bytes.len(), "comment"));
            return state;
        }
        if bytes[i] == b'"' || bytes[i] == b'\'' {
            let end = scan_string(bytes, i);
            out.push((i..end, "string"));
            i = end;
            continue;
        }
        if bytes[i] == b'@' {
            let start = i;
            i += 1;
            while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'-') {
                i += 1;
            }
            out.push((start..i, "keyword"));
            continue;
        }
        if bytes[i] == b'!' && line[i..].starts_with("!important") {
            out.push((i..i + 10, "keyword"));
            i += 10;
            continue;
        }
        if bytes[i] == b'$' || bytes[i..].starts_with(b"--") {
            let start = i;
            i += if bytes[i] == b'$' { 1 } else { 2 };
            while i < bytes.len()
                && (bytes[i].is_ascii_alphanumeric() || matches!(bytes[i], b'-' | b'_'))
            {
                i += 1;
            }
            out.push((start..i, "constant"));
            continue;
        }
        if bytes[i] == b'#' {
            let start = i;
            i += 1;
            while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'-') {
                i += 1;
            }
            let hex = i - start - 1;
            let is_hex = matches!(hex, 3 | 4 | 6 | 8)
                && bytes[start + 1..i].iter().all(u8::is_ascii_hexdigit);
            out.push((start..i, if is_hex { "number" } else { "type" }));
            continue;
        }
        if bytes[i].is_ascii_digit()
            || (bytes[i] == b'.' && i + 1 < bytes.len() && bytes[i + 1].is_ascii_digit())
        {
            let start = i;
            while i < bytes.len() && (bytes[i].is_ascii_digit() || bytes[i] == b'.') {
                i += 1;
            }
            // Include CSS unit (px, rem, em, %, vh, vw, s, ms, deg).
            while i < bytes.len() && (bytes[i].is_ascii_alphabetic() || bytes[i] == b'%') {
                i += 1;
            }
            out.push((start..i, "number"));
            continue;
        }
        if bytes[i] == b'.' && i + 1 < bytes.len() && bytes[i + 1].is_ascii_alphabetic() {
            let start = i;
            i += 1;
            while i < bytes.len()
                && (bytes[i].is_ascii_alphanumeric() || matches!(bytes[i], b'-' | b'_'))
            {
                i += 1;
            }
            out.push((start..i, "type"));
            continue;
        }
        if bytes[i].is_ascii_alphabetic() || bytes[i] == b'-' {
            let start = i;
            while i < bytes.len()
                && (bytes[i].is_ascii_alphanumeric() || matches!(bytes[i], b'-' | b'_'))
            {
                i += 1;
            }
            let rest = line[i..].trim_start();
            if rest.starts_with(':') && !rest.starts_with("::") {
                out.push((start..i, "function"));
            } else if rest.starts_with('(') {
                out.push((start..i, "function"));
            } else if matches!(
                &line[start..i],
                "none" | "auto" | "inherit" | "initial" | "unset" | "transparent" | "currentColor"
            ) {
                out.push((start..i, "constant"));
            }
            continue;
        }
        if is_punct(bytes[i]) {
            out.push((i..i + 1, "punctuation"));
        }
        i += 1;
    }
    state
}

fn tokenize_markdown(
    line: &str,
    mut state: BlockState,
    out: &mut Vec<(Range<usize>, &'static str)>,
) -> BlockState {
    if state == BlockState::HtmlComment {
        if let Some(end) = line.find("-->") {
            out.push((0..end + 3, "comment"));
            state = BlockState::Normal;
        } else {
            out.push((0..line.len(), "comment"));
            return state;
        }
    }
    let trimmed = line.trim_start();
    let indent = line.len() - trimmed.len();
    if trimmed.starts_with('#') {
        out.push((indent..line.len(), "keyword"));
        return state;
    }
    if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
        out.push((indent..line.len(), "string"));
        return state;
    }
    if trimmed.starts_with("<!--") {
        if let Some(end) = trimmed.find("-->") {
            out.push((indent..indent + end + 3, "comment"));
        } else {
            out.push((indent..line.len(), "comment"));
            return BlockState::HtmlComment;
        }
        return state;
    }
    let bytes = line.as_bytes();
    let mut i = indent;
    while i < bytes.len() {
        if bytes[i] == b'`' {
            let start = i;
            i += 1;
            while i < bytes.len() && bytes[i] != b'`' {
                i += 1;
            }
            if i < bytes.len() {
                i += 1;
            }
            out.push((start..i, "string"));
            continue;
        }
        i += 1;
    }
    state
}

fn tokenize_json(
    line: &str,
    mut state: BlockState,
    out: &mut Vec<(Range<usize>, &'static str)>,
) -> BlockState {
    let bytes = line.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if state == BlockState::CComment {
            if let Some(rel) = line[i..].find("*/") {
                let end = i + rel + 2;
                out.push((i..end, "comment"));
                i = end;
                state = BlockState::Normal;
            } else {
                out.push((i..bytes.len(), "comment"));
                return state;
            }
            continue;
        }
        if bytes[i..].starts_with(b"//") {
            out.push((i..bytes.len(), "comment"));
            return state;
        }
        if bytes[i..].starts_with(b"/*") {
            let start = i;
            i += 2;
            if let Some(rel) = line[i..].find("*/") {
                let end = i + rel + 2;
                out.push((start..end, "comment"));
                i = end;
            } else {
                out.push((start..bytes.len(), "comment"));
                return BlockState::CComment;
            }
            continue;
        }
        if bytes[i] == b'"' {
            let end = scan_string(bytes, i);
            let is_key = line[end..].trim_start().starts_with(':');
            out.push((i..end, if is_key { "function" } else { "string" }));
            i = end;
            continue;
        }
        if bytes[i].is_ascii_digit()
            || (bytes[i] == b'-' && i + 1 < bytes.len() && bytes[i + 1].is_ascii_digit())
        {
            let start = i;
            if bytes[i] == b'-' {
                i += 1;
            }
            let end = scan_number(bytes, i);
            out.push((start..end, "number"));
            i = end;
            continue;
        }
        if is_word_start(bytes[i]) {
            let start = i;
            while i < bytes.len() && is_word_continue(bytes[i]) {
                i += 1;
            }
            if matches!(&line[start..i], "true" | "false" | "null") {
                out.push((start..i, "constant"));
            }
            continue;
        }
        if is_punct(bytes[i]) {
            out.push((i..i + 1, "punctuation"));
        }
        i += 1;
    }
    state
}

fn tokenize_config(line: &str, out: &mut Vec<(Range<usize>, &'static str)>) {
    let bytes = line.as_bytes();
    let mut i = 0;
    while i < bytes.len() && matches!(bytes[i], b' ' | b'\t') {
        i += 1;
    }
    if i >= bytes.len() {
        return;
    }
    if bytes[i] == b'#' {
        out.push((i..bytes.len(), "comment"));
        return;
    }
    if bytes[i] == b'[' {
        if let Some(close) = line[i..].rfind(']') {
            out.push((i..i + 1, "punctuation"));
            if close > 1 {
                out.push((i + 1..i + close, "type"));
            }
            out.push((i + close..i + close + 1, "punctuation"));
            return;
        }
    }
    while i < bytes.len() {
        if bytes[i] == b'#' {
            out.push((i..bytes.len(), "comment"));
            return;
        }
        if bytes[i] == b'"' || bytes[i] == b'\'' {
            let end = scan_string(bytes, i);
            out.push((i..end, "string"));
            i = end;
            continue;
        }
        if bytes[i].is_ascii_digit()
            && (i == 0 || !bytes[i - 1].is_ascii_alphanumeric() && bytes[i - 1] != b'_')
        {
            let end = scan_number(bytes, i);
            out.push((i..end, "number"));
            i = end;
            continue;
        }
        if is_word_start(bytes[i]) {
            let start = i;
            while i < bytes.len()
                && (bytes[i].is_ascii_alphanumeric() || matches!(bytes[i], b'_' | b'-'))
            {
                i += 1;
            }
            let word = &line[start..i];
            let rest = line[i..].trim_start();
            if rest.starts_with('=') || rest.starts_with(':') {
                out.push((start..i, "function"));
            } else if matches!(word, "true" | "false" | "null" | "yes" | "no") {
                out.push((start..i, "constant"));
            }
            continue;
        }
        if is_punct(bytes[i]) {
            out.push((i..i + 1, "punctuation"));
        }
        i += 1;
    }
}

fn tokenize_code(
    lang: &str,
    line: &str,
    mut state: BlockState,
    out: &mut Vec<(Range<usize>, &'static str)>,
) -> BlockState {
    let bytes = line.as_bytes();
    let mut i = 0;

    let block = supports_block_comment(lang);
    let slash_comment = matches!(
        lang,
        "rust" | "js" | "c" | "csharp" | "java" | "go" | "kotlin" | "swift" | "php"
    );
    let hash_comment = matches!(lang, "python" | "ruby" | "php" | "shell" | "powershell");
    let dash_comment = matches!(lang, "sql" | "lua");

    // Preprocessor directive at start of C/C++/C# lines (`#include`, `#region`).
    if matches!(lang, "c" | "csharp") {
        let trimmed = line.trim_start();
        let indent = line.len() - trimmed.len();
        if trimmed.starts_with('#') {
            let mut end = indent + 1;
            while end < bytes.len() && bytes[end].is_ascii_alphabetic() {
                end += 1;
            }
            out.push((indent..end, "keyword"));
            i = end;
        }
    }

    while i < bytes.len() {
        if let Some((_, close, kind)) = block {
            if state == kind {
                if let Some(rel) = line[i..].find(close) {
                    let end = i + rel + close.len();
                    out.push((i..end, "comment"));
                    i = end;
                    state = BlockState::Normal;
                } else {
                    out.push((i..bytes.len(), "comment"));
                    return state;
                }
                continue;
            }
        }
        if let Some((open, close, kind)) = block {
            if bytes[i..].starts_with(open.as_bytes()) {
                let start = i;
                i += open.len();
                if let Some(rel) = line[i..].find(close) {
                    let end = i + rel + close.len();
                    out.push((start..end, "comment"));
                    i = end;
                } else {
                    out.push((start..bytes.len(), "comment"));
                    return kind;
                }
                continue;
            }
        }
        if slash_comment && bytes[i..].starts_with(b"//") {
            out.push((i..bytes.len(), "comment"));
            return state;
        }
        if dash_comment && bytes[i..].starts_with(b"--") {
            out.push((i..bytes.len(), "comment"));
            return state;
        }
        if hash_comment && bytes[i] == b'#' {
            out.push((i..bytes.len(), "comment"));
            return state;
        }
        // Strings and character literals.
        if bytes[i] == b'"'
            || (bytes[i] == b'`' && matches!(lang, "js" | "go"))
            || (bytes[i] == b'\''
                && (lang != "rust" || is_rust_char_literal(bytes, i)))
        {
            let end = scan_string(bytes, i);
            out.push((i..end, "string"));
            i = end;
            continue;
        }
        // Rust lifetime `'a`
        if lang == "rust" && bytes[i] == b'\'' && i + 1 < bytes.len() && is_word_start(bytes[i + 1])
        {
            let start = i;
            i += 1;
            while i < bytes.len() && is_word_continue(bytes[i]) {
                i += 1;
            }
            out.push((start..i, "type"));
            continue;
        }
        // Annotations/decorators (`@Override`, `@dataclass`)
        if matches!(lang, "java" | "kotlin" | "python" | "js" | "swift")
            && bytes[i] == b'@'
            && i + 1 < bytes.len()
            && is_word_start(bytes[i + 1])
        {
            let start = i;
            i += 1;
            while i < bytes.len() && is_word_continue(bytes[i]) {
                i += 1;
            }
            out.push((start..i, "keyword"));
            continue;
        }
        // Ruby symbols (`:ok`)
        if lang == "ruby"
            && bytes[i] == b':'
            && i + 1 < bytes.len()
            && is_word_start(bytes[i + 1])
            && (i == 0 || !is_word_continue(bytes[i - 1]))
        {
            let start = i;
            i += 1;
            while i < bytes.len() && is_word_continue(bytes[i]) {
                i += 1;
            }
            out.push((start..i, "constant"));
            continue;
        }
        // Variables (`$var` in PowerShell / PHP / Shell)
        if matches!(lang, "powershell" | "php" | "shell")
            && bytes[i] == b'$'
            && i + 1 < bytes.len()
            && (is_word_start(bytes[i + 1]) || bytes[i + 1] == b'{')
        {
            let start = i;
            i += 1;
            if bytes[i] == b'{' {
                while i < bytes.len() && bytes[i] != b'}' {
                    i += 1;
                }
                if i < bytes.len() {
                    i += 1;
                }
            } else {
                while i < bytes.len() && is_word_continue(bytes[i]) {
                    i += 1;
                }
            }
            let var = &line[start..i];
            let role = if lang == "powershell"
                && matches!(
                    var.to_ascii_lowercase().as_str(),
                    "$true" | "$false" | "$null"
                ) {
                "constant"
            } else {
                "constant"
            };
            out.push((start..i, role));
            continue;
        }
        // PowerShell `-Parameter`
        if lang == "powershell"
            && bytes[i] == b'-'
            && i + 1 < bytes.len()
            && bytes[i + 1].is_ascii_alphabetic()
            && (i == 0 || bytes[i - 1].is_ascii_whitespace())
        {
            let start = i;
            i += 1;
            while i < bytes.len() && is_word_continue(bytes[i]) {
                i += 1;
            }
            out.push((start..i, "function"));
            continue;
        }
        // Numbers
        if bytes[i].is_ascii_digit()
            && (i == 0 || !is_word_continue(bytes[i - 1]))
        {
            let end = scan_number(bytes, i);
            out.push((i..end, "number"));
            i = end;
            continue;
        }
        // Identifiers -> keyword / constant / type / function
        if is_word_start(bytes[i]) {
            let start = i;
            while i < bytes.len() && is_word_continue(bytes[i]) {
                i += 1;
            }
            let word = &line[start..i];
            let next_non_ws = bytes[i..]
                .iter()
                .copied()
                .find(|b| !matches!(b, b' ' | b'\t'));
            if let Some(role) = classify_word(lang, word, next_non_ws) {
                out.push((start..i, role));
            }
            continue;
        }
        if is_punct(bytes[i]) {
            out.push((i..i + 1, "punctuation"));
        }
        i += 1;
    }
    state
}

fn is_rust_char_literal(bytes: &[u8], i: usize) -> bool {
    if i + 2 < bytes.len() && bytes[i + 1] == b'\\' {
        // Escaped char like `'\n'` or `'\''` or `'\x7f'`
        let mut j = i + 2;
        while j < bytes.len() && j < i + 8 {
            if bytes[j] == b'\'' {
                return true;
            }
            j += 1;
        }
        return false;
    }
    i + 2 < bytes.len() && bytes[i + 2] == b'\''
}

fn scan_string(bytes: &[u8], start: usize) -> usize {
    let quote = bytes[start];
    let mut i = start + 1;
    while i < bytes.len() {
        if bytes[i] == b'\\' && i + 1 < bytes.len() {
            i += 2;
            continue;
        }
        if bytes[i] == quote {
            return i + 1;
        }
        i += 1;
    }
    bytes.len()
}

fn scan_number(bytes: &[u8], start: usize) -> usize {
    let mut i = start;
    if bytes[i..].starts_with(b"0x")
        || bytes[i..].starts_with(b"0X")
        || bytes[i..].starts_with(b"0b")
        || bytes[i..].starts_with(b"0B")
        || bytes[i..].starts_with(b"0o")
        || bytes[i..].starts_with(b"0O")
    {
        i += 2;
        while i < bytes.len() && (bytes[i].is_ascii_hexdigit() || bytes[i] == b'_') {
            i += 1;
        }
        return i;
    }
    while i < bytes.len() && (bytes[i].is_ascii_digit() || bytes[i] == b'_') {
        i += 1;
    }
    if i + 1 < bytes.len() && bytes[i] == b'.' && bytes[i + 1].is_ascii_digit() {
        i += 1;
        while i < bytes.len() && (bytes[i].is_ascii_digit() || bytes[i] == b'_') {
            i += 1;
        }
    }
    if i < bytes.len() && matches!(bytes[i], b'e' | b'E') {
        let exp = i;
        i += 1;
        if i < bytes.len() && matches!(bytes[i], b'+' | b'-') {
            i += 1;
        }
        if i < bytes.len() && bytes[i].is_ascii_digit() {
            while i < bytes.len() && (bytes[i].is_ascii_digit() || bytes[i] == b'_') {
                i += 1;
            }
        } else {
            i = exp;
        }
    }
    // Numeric suffixes like `u32`, `f64`, `i64`, `ul`, `L`
    while i < bytes.len() && bytes[i].is_ascii_alphanumeric() {
        i += 1;
    }
    i
}

fn is_word_start(b: u8) -> bool {
    b.is_ascii_alphabetic() || b == b'_'
}

fn is_word_continue(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

fn is_punct(b: u8) -> bool {
    matches!(
        b,
        b'(' | b')'
            | b'{'
            | b'}'
            | b'['
            | b']'
            | b','
            | b';'
            | b':'
            | b'.'
            | b'+'
            | b'-'
            | b'*'
            | b'/'
            | b'%'
            | b'='
            | b'!'
            | b'<'
            | b'>'
            | b'&'
            | b'|'
            | b'^'
            | b'~'
            | b'?'
    )
}

fn is_screaming_constant(word: &str) -> bool {
    word.len() >= 2
        && word.bytes().any(|b| b.is_ascii_uppercase())
        && word
            .bytes()
            .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_')
}

fn is_pascal_type(word: &str) -> bool {
    let bytes = word.as_bytes();
    bytes.len() >= 2
        && bytes[0].is_ascii_uppercase()
        && bytes[1..].iter().any(|b| b.is_ascii_lowercase())
}

fn classify_word(lang: &str, word: &str, next_non_ws: Option<u8>) -> Option<&'static str> {
    if is_keyword(lang, word) {
        return Some("keyword");
    }
    if is_constant(lang, word) {
        return Some("constant");
    }
    if is_builtin_type(lang, word) {
        return Some("type");
    }
    if next_non_ws == Some(b'(') || (lang == "rust" && next_non_ws == Some(b'!')) {
        return Some("function");
    }
    if is_screaming_constant(word) && lang != "sql" {
        return Some("constant");
    }
    if is_pascal_type(word) && !matches!(lang, "shell" | "sql") {
        return Some("type");
    }
    None
}

fn is_constant(lang: &str, word: &str) -> bool {
    match lang {
        "rust" => matches!(word, "true" | "false" | "self" | "None" | "Some" | "Ok" | "Err"),
        "js" => matches!(
            word,
            "true" | "false" | "null" | "undefined" | "NaN" | "Infinity" | "this" | "super"
        ),
        "python" => matches!(word, "True" | "False" | "None" | "self" | "cls"),
        "c" => matches!(word, "true" | "false" | "NULL" | "nullptr" | "this"),
        "csharp" => matches!(word, "true" | "false" | "null" | "this" | "base"),
        "java" => matches!(word, "true" | "false" | "null" | "this" | "super"),
        "go" => matches!(word, "true" | "false" | "nil" | "iota"),
        "kotlin" | "swift" => matches!(word, "true" | "false" | "null" | "nil" | "this" | "self" | "super"),
        "ruby" => matches!(word, "true" | "false" | "nil" | "self" | "super"),
        "php" => matches!(
            word.to_ascii_lowercase().as_str(),
            "true" | "false" | "null" | "self" | "parent"
        ),
        "lua" => matches!(word, "true" | "false" | "nil"),
        "shell" => matches!(word, "true" | "false"),
        "sql" => matches!(word.to_ascii_uppercase().as_str(), "NULL" | "TRUE" | "FALSE"),
        _ => false,
    }
}

fn is_builtin_type(lang: &str, word: &str) -> bool {
    match lang {
        "rust" => matches!(
            word,
            "bool"
                | "char"
                | "str"
                | "u8"
                | "u16"
                | "u32"
                | "u64"
                | "u128"
                | "usize"
                | "i8"
                | "i16"
                | "i32"
                | "i64"
                | "i128"
                | "isize"
                | "f32"
                | "f64"
                | "Self"
                | "String"
                | "Vec"
                | "Option"
                | "Result"
                | "Box"
                | "Arc"
                | "Rc"
        ),
        "js" => matches!(
            word,
            "string" | "number" | "boolean" | "void" | "any" | "unknown" | "never" | "object"
                | "symbol" | "bigint"
        ),
        "python" => matches!(
            word,
            "int" | "str" | "float" | "bool" | "list" | "dict" | "set" | "tuple" | "bytes"
                | "object"
        ),
        "c" => matches!(
            word,
            "void"
                | "char"
                | "short"
                | "int"
                | "long"
                | "float"
                | "double"
                | "bool"
                | "signed"
                | "unsigned"
                | "size_t"
                | "ssize_t"
                | "ptrdiff_t"
                | "int8_t"
                | "int16_t"
                | "int32_t"
                | "int64_t"
                | "uint8_t"
                | "uint16_t"
                | "uint32_t"
                | "uint64_t"
                | "auto"
        ),
        "csharp" => matches!(
            word,
            "void"
                | "bool"
                | "byte"
                | "sbyte"
                | "char"
                | "decimal"
                | "double"
                | "float"
                | "int"
                | "uint"
                | "nint"
                | "nuint"
                | "long"
                | "ulong"
                | "short"
                | "ushort"
                | "object"
                | "string"
                | "dynamic"
        ),
        "java" => matches!(
            word,
            "void" | "boolean" | "byte" | "char" | "short" | "int" | "long" | "float" | "double"
        ),
        "go" => matches!(
            word,
            "bool"
                | "string"
                | "int"
                | "int8"
                | "int16"
                | "int32"
                | "int64"
                | "uint"
                | "uint8"
                | "uint16"
                | "uint32"
                | "uint64"
                | "uintptr"
                | "byte"
                | "rune"
                | "float32"
                | "float64"
                | "complex64"
                | "complex128"
                | "error"
                | "any"
        ),
        "php" => matches!(
            word,
            "int" | "float" | "string" | "bool" | "array" | "object" | "mixed" | "void" | "never"
                | "callable" | "iterable"
        ),
        "sql" => matches!(
            word.to_ascii_uppercase().as_str(),
            "INT"
                | "INTEGER"
                | "BIGINT"
                | "SMALLINT"
                | "TINYINT"
                | "VARCHAR"
                | "NVARCHAR"
                | "CHAR"
                | "TEXT"
                | "BOOLEAN"
                | "BOOL"
                | "DATE"
                | "TIME"
                | "TIMESTAMP"
                | "DATETIME"
                | "DECIMAL"
                | "NUMERIC"
                | "FLOAT"
                | "DOUBLE"
                | "REAL"
                | "BLOB"
                | "JSON"
                | "JSONB"
                | "UUID"
                | "SERIAL"
        ),
        _ => false,
    }
}

fn is_keyword(lang: &str, word: &str) -> bool {
    match lang {
        "rust" => matches!(
            word,
            "as" | "async"
                | "await"
                | "break"
                | "const"
                | "continue"
                | "crate"
                | "dyn"
                | "else"
                | "enum"
                | "extern"
                | "fn"
                | "for"
                | "if"
                | "impl"
                | "in"
                | "let"
                | "loop"
                | "match"
                | "mod"
                | "move"
                | "mut"
                | "pub"
                | "ref"
                | "return"
                | "static"
                | "struct"
                | "super"
                | "trait"
                | "type"
                | "unsafe"
                | "use"
                | "where"
                | "while"
        ),
        "js" => matches!(
            word,
            "as" | "async"
                | "await"
                | "break"
                | "case"
                | "catch"
                | "class"
                | "const"
                | "continue"
                | "debugger"
                | "default"
                | "delete"
                | "do"
                | "else"
                | "enum"
                | "export"
                | "extends"
                | "finally"
                | "for"
                | "from"
                | "function"
                | "if"
                | "implements"
                | "import"
                | "in"
                | "instanceof"
                | "interface"
                | "keyof"
                | "let"
                | "new"
                | "of"
                | "private"
                | "protected"
                | "public"
                | "readonly"
                | "return"
                | "static"
                | "switch"
                | "throw"
                | "try"
                | "type"
                | "typeof"
                | "var"
                | "while"
                | "yield"
        ),
        "python" => matches!(
            word,
            "and"
                | "as"
                | "assert"
                | "async"
                | "await"
                | "break"
                | "case"
                | "class"
                | "continue"
                | "def"
                | "del"
                | "elif"
                | "else"
                | "except"
                | "finally"
                | "for"
                | "from"
                | "global"
                | "if"
                | "import"
                | "in"
                | "is"
                | "lambda"
                | "match"
                | "nonlocal"
                | "not"
                | "or"
                | "pass"
                | "raise"
                | "return"
                | "try"
                | "while"
                | "with"
                | "yield"
        ),
        "c" => matches!(
            word,
            "break"
                | "case"
                | "catch"
                | "class"
                | "const"
                | "constexpr"
                | "continue"
                | "default"
                | "delete"
                | "do"
                | "else"
                | "enum"
                | "explicit"
                | "extern"
                | "for"
                | "friend"
                | "goto"
                | "if"
                | "inline"
                | "mutable"
                | "namespace"
                | "new"
                | "noexcept"
                | "operator"
                | "override"
                | "private"
                | "protected"
                | "public"
                | "register"
                | "return"
                | "sizeof"
                | "static"
                | "struct"
                | "switch"
                | "template"
                | "throw"
                | "try"
                | "typedef"
                | "typename"
                | "union"
                | "using"
                | "virtual"
                | "volatile"
                | "while"
        ),
        "csharp" => matches!(
            word,
            "abstract"
                | "as"
                | "async"
                | "await"
                | "break"
                | "case"
                | "catch"
                | "checked"
                | "class"
                | "const"
                | "continue"
                | "default"
                | "delegate"
                | "do"
                | "else"
                | "enum"
                | "event"
                | "explicit"
                | "extern"
                | "finally"
                | "fixed"
                | "for"
                | "foreach"
                | "from"
                | "get"
                | "goto"
                | "if"
                | "implicit"
                | "in"
                | "init"
                | "interface"
                | "internal"
                | "is"
                | "lock"
                | "nameof"
                | "namespace"
                | "new"
                | "operator"
                | "out"
                | "override"
                | "params"
                | "partial"
                | "private"
                | "protected"
                | "public"
                | "readonly"
                | "record"
                | "ref"
                | "return"
                | "sealed"
                | "select"
                | "set"
                | "sizeof"
                | "stackalloc"
                | "static"
                | "struct"
                | "switch"
                | "throw"
                | "try"
                | "typeof"
                | "unchecked"
                | "unsafe"
                | "using"
                | "var"
                | "virtual"
                | "volatile"
                | "where"
                | "while"
                | "yield"
        ),
        "java" => matches!(
            word,
            "abstract"
                | "assert"
                | "break"
                | "case"
                | "catch"
                | "class"
                | "continue"
                | "default"
                | "do"
                | "else"
                | "enum"
                | "extends"
                | "final"
                | "finally"
                | "for"
                | "if"
                | "implements"
                | "import"
                | "instanceof"
                | "interface"
                | "native"
                | "new"
                | "package"
                | "permits"
                | "private"
                | "protected"
                | "public"
                | "record"
                | "return"
                | "sealed"
                | "static"
                | "strictfp"
                | "switch"
                | "synchronized"
                | "throw"
                | "throws"
                | "transient"
                | "try"
                | "var"
                | "volatile"
                | "while"
                | "yield"
        ),
        "go" => matches!(
            word,
            "break"
                | "case"
                | "chan"
                | "const"
                | "continue"
                | "default"
                | "defer"
                | "else"
                | "fallthrough"
                | "for"
                | "func"
                | "go"
                | "goto"
                | "if"
                | "import"
                | "interface"
                | "map"
                | "package"
                | "range"
                | "return"
                | "select"
                | "struct"
                | "switch"
                | "type"
                | "var"
        ),
        "kotlin" => matches!(
            word,
            "abstract"
                | "as"
                | "break"
                | "by"
                | "catch"
                | "class"
                | "companion"
                | "const"
                | "constructor"
                | "continue"
                | "data"
                | "do"
                | "else"
                | "enum"
                | "finally"
                | "for"
                | "fun"
                | "if"
                | "import"
                | "in"
                | "init"
                | "inline"
                | "interface"
                | "internal"
                | "is"
                | "lateinit"
                | "object"
                | "open"
                | "override"
                | "package"
                | "private"
                | "protected"
                | "public"
                | "return"
                | "sealed"
                | "suspend"
                | "throw"
                | "try"
                | "typealias"
                | "val"
                | "var"
                | "when"
                | "where"
                | "while"
        ),
        "swift" => matches!(
            word,
            "actor"
                | "any"
                | "as"
                | "associatedtype"
                | "async"
                | "await"
                | "break"
                | "case"
                | "catch"
                | "class"
                | "continue"
                | "default"
                | "defer"
                | "deinit"
                | "do"
                | "else"
                | "enum"
                | "extension"
                | "fileprivate"
                | "final"
                | "for"
                | "func"
                | "guard"
                | "if"
                | "import"
                | "in"
                | "init"
                | "internal"
                | "is"
                | "let"
                | "mutating"
                | "open"
                | "override"
                | "private"
                | "protocol"
                | "public"
                | "repeat"
                | "return"
                | "some"
                | "static"
                | "struct"
                | "switch"
                | "throw"
                | "throws"
                | "try"
                | "typealias"
                | "unowned"
                | "var"
                | "weak"
                | "where"
                | "while"
        ),
        "ruby" => matches!(
            word,
            "and"
                | "attr_accessor"
                | "attr_reader"
                | "attr_writer"
                | "begin"
                | "break"
                | "case"
                | "class"
                | "def"
                | "do"
                | "else"
                | "elsif"
                | "end"
                | "ensure"
                | "extend"
                | "for"
                | "if"
                | "in"
                | "include"
                | "lambda"
                | "module"
                | "next"
                | "not"
                | "or"
                | "private"
                | "proc"
                | "protected"
                | "public"
                | "raise"
                | "redo"
                | "require"
                | "require_relative"
                | "rescue"
                | "retry"
                | "return"
                | "then"
                | "unless"
                | "until"
                | "when"
                | "while"
                | "yield"
        ),
        "php" => matches!(
            word.to_ascii_lowercase().as_str(),
            "abstract"
                | "as"
                | "break"
                | "case"
                | "catch"
                | "class"
                | "const"
                | "continue"
                | "default"
                | "do"
                | "echo"
                | "else"
                | "elseif"
                | "enum"
                | "extends"
                | "final"
                | "finally"
                | "fn"
                | "for"
                | "foreach"
                | "function"
                | "if"
                | "implements"
                | "include"
                | "include_once"
                | "instanceof"
                | "interface"
                | "match"
                | "namespace"
                | "new"
                | "print"
                | "private"
                | "protected"
                | "public"
                | "readonly"
                | "require"
                | "require_once"
                | "return"
                | "static"
                | "switch"
                | "throw"
                | "trait"
                | "try"
                | "use"
                | "while"
                | "yield"
        ),
        "lua" => matches!(
            word,
            "and"
                | "break"
                | "do"
                | "else"
                | "elseif"
                | "end"
                | "for"
                | "function"
                | "goto"
                | "if"
                | "in"
                | "local"
                | "not"
                | "or"
                | "repeat"
                | "return"
                | "then"
                | "until"
                | "while"
        ),
        "powershell" => matches!(
            word.to_ascii_lowercase().as_str(),
            "begin"
                | "break"
                | "catch"
                | "class"
                | "continue"
                | "default"
                | "do"
                | "else"
                | "elseif"
                | "end"
                | "enum"
                | "exit"
                | "filter"
                | "finally"
                | "for"
                | "foreach"
                | "function"
                | "if"
                | "in"
                | "param"
                | "process"
                | "return"
                | "switch"
                | "throw"
                | "trap"
                | "try"
                | "until"
                | "using"
                | "while"
        ),
        "shell" => matches!(
            word,
            "case"
                | "do"
                | "done"
                | "elif"
                | "else"
                | "esac"
                | "exit"
                | "export"
                | "fi"
                | "for"
                | "function"
                | "if"
                | "in"
                | "local"
                | "return"
                | "set"
                | "then"
                | "until"
                | "while"
        ),
        "sql" => matches!(
            word.to_ascii_uppercase().as_str(),
            "ALL"
                | "ALTER"
                | "ANALYZE"
                | "AND"
                | "AS"
                | "ASC"
                | "BEGIN"
                | "BETWEEN"
                | "BY"
                | "CASE"
                | "CHECK"
                | "COMMIT"
                | "CONSTRAINT"
                | "CREATE"
                | "CROSS"
                | "DATABASE"
                | "DEFAULT"
                | "DELETE"
                | "DESC"
                | "DISTINCT"
                | "DROP"
                | "ELSE"
                | "END"
                | "EXISTS"
                | "EXPLAIN"
                | "FOREIGN"
                | "FROM"
                | "FULL"
                | "GRANT"
                | "GROUP"
                | "HAVING"
                | "IF"
                | "ILIKE"
                | "IN"
                | "INDEX"
                | "INNER"
                | "INSERT"
                | "INTO"
                | "IS"
                | "JOIN"
                | "KEY"
                | "LEFT"
                | "LIKE"
                | "LIMIT"
                | "NOT"
                | "OFFSET"
                | "ON"
                | "OR"
                | "ORDER"
                | "OUTER"
                | "PRIMARY"
                | "RECURSIVE"
                | "REFERENCES"
                | "RETURNING"
                | "REVOKE"
                | "RIGHT"
                | "ROLLBACK"
                | "SCHEMA"
                | "SELECT"
                | "SET"
                | "TABLE"
                | "THEN"
                | "TRANSACTION"
                | "TRIGGER"
                | "UNION"
                | "UNIQUE"
                | "UPDATE"
                | "VALUES"
                | "VIEW"
                | "WHEN"
                | "WHERE"
                | "WITH"
        ),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::appearance::{SyntaxPalette, Tints};

    struct TestResolver;
    impl HighlightStyleResolver for TestResolver {
        fn style(&self, name: &str) -> Option<HighlightStyle> {
            SyntaxPalette::from_tints(Tints::new(true)).style(name)
        }
    }

    #[test]
    fn language_detects_all_phase_11_types() {
        let cases = [
            ("main.rs", "rust"),
            ("index.tsx", "js"),
            ("data.json", "json"),
            ("app.py", "python"),
            ("main.cpp", "c"),
            ("Program.cs", "csharp"),
            ("App.java", "java"),
            ("page.html", "html"),
            ("layout.xml", "xml"),
            ("styles.scss", "css"),
            ("query.sql", "sql"),
            ("server.go", "go"),
            ("Main.kt", "kotlin"),
            ("App.swift", "swift"),
            ("Gemfile", "ruby"),
            ("index.php", "php"),
            ("init.lua", "lua"),
            ("build.ps1", "powershell"),
            ("Dockerfile", "dockerfile"),
            ("Dockerfile.prod", "dockerfile"),
            ("config.ini", "ini"),
            ("app.conf", "ini"),
            ("fix.patch", "diff"),
            (".env", "env"),
            (".env.local", "env"),
            ("prod.env", "env"),
            ("README.md", "markdown"),
        ];
        for (file, expected) in cases {
            assert_eq!(language(Path::new(file)), expected, "{file}");
        }
    }

    #[test]
    fn csharp_and_java_have_separate_keywords() {
        assert!(is_keyword("csharp", "foreach"));
        assert!(!is_keyword("java", "foreach"));
        assert!(is_keyword("java", "synchronized"));
        assert!(!is_keyword("csharp", "synchronized"));
    }

    #[test]
    fn highlights_all_eight_roles_and_languages() {
        let palette = SyntaxPalette::from_tints(Tints::new(true));
        let rust = "const MAX: usize = 42;\nfn greet(name: &str) -> bool { /* hi */ true }\n";
        let spans = highlight_text("rust", rust, &palette);
        let colors: Vec<_> = spans.iter().filter_map(|(_, s)| s.color).collect();
        for role_color in [
            palette.keyword,
            palette.constant,
            palette.type_,
            palette.number,
            palette.function,
            palette.comment,
            palette.punctuation,
        ] {
            assert!(
                colors.contains(&role_color),
                "missing colour {role_color:?} in rust highlights"
            );
        }

        // .env colours keys and values separately
        let env_spans = highlight_text("env", "# secret\nAPI_KEY=abc123\n", &palette);
        assert!(env_spans.iter().any(|(_, s)| s.color == Some(palette.comment)));
        assert!(env_spans.iter().any(|(_, s)| s.color == Some(palette.function)));
        assert!(env_spans.iter().any(|(_, s)| s.color == Some(palette.string)));

        // HTML tags, attributes and values
        let html_spans = highlight_text("html", "<div class=\"card\">ok</div>", &palette);
        assert!(html_spans.iter().any(|(_, s)| s.color == Some(palette.type_)));
        assert!(html_spans.iter().any(|(_, s)| s.color == Some(palette.function)));
        assert!(html_spans.iter().any(|(_, s)| s.color == Some(palette.string)));

        // SQL keywords, types and strings
        let sql_spans = highlight_text("sql", "SELECT id FROM users WHERE name = 'ada';", &palette);
        assert!(sql_spans.iter().any(|(_, s)| s.color == Some(palette.keyword)));
        assert!(sql_spans.iter().any(|(_, s)| s.color == Some(palette.string)));

        // Diff added/removed/hunk
        let diff_spans = highlight_text("diff", "@@ -1,2 +1,2 @@\n-old\n+new\n", &palette);
        assert!(diff_spans.iter().any(|(_, s)| s.color == Some(palette.number)));
        assert!(diff_spans.iter().any(|(_, s)| s.color == Some(palette.constant)));
        assert!(diff_spans.iter().any(|(_, s)| s.color == Some(palette.string)));
    }

    #[test]
    fn visible_range_highlighting_stays_fast_on_large_files() {
        let mut h = Highlighter::new("rust");
        let mut big = String::with_capacity(20_000 * 32);
        for i in 0..20_000 {
            big.push_str(&format!("let value_{i}: usize = {i};\n"));
        }
        h.text = Rope::from(big.as_str());
        let line_15000 = h.text.line_start_offset(15_000);
        let line_15040 = h.text.line_start_offset(15_040);
        let start = std::time::Instant::now();
        let runs = h.styles(&(line_15000..line_15040), &TestResolver);
        assert!(
            start.elapsed() < std::time::Duration::from_millis(50),
            "too slow: {:?}",
            start.elapsed()
        );
        assert_eq!(runs.first().unwrap().0.start, line_15000);
        assert_eq!(runs.last().unwrap().0.end, line_15040);
        assert!(runs.iter().any(|(_, s)| s.color.is_some()));
    }
}
