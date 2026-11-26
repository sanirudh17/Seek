//! Secret protection for `.env` files (Phase 11.3).
//!
//! Detects `.env`, `.env.*` and `*.env` files and replaces every non-empty
//! value with deterministic made-up letters whose length never reveals the
//! real secret's length. Keys, `export` prefixes, `=`, quotes and comments
//! stay visible.
//!
//! Hidden values are excluded from rendered preview buffers, clipboard copies,
//! content-search snippets, MCP `read_text` output (unless explicitly opted
//! in) and debug logs.
use std::{collections::HashSet, fmt, path::Path};

/// Whether `path` is an environment file (`.env`, `.env.*`, `*.env`).
pub fn is_env_file(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
        return false;
    };
    crate::code_highlight::is_env_name(&name.to_ascii_lowercase())
}

/// Deterministic made-up letters whose length (8..=13) is derived purely from
/// a BLAKE3 hash of `(key, value)` — never from `value.len()` — so it reveals
/// neither the content nor the length of the secret while staying distinct per
/// value and stable across redraws.
pub fn mask_value(key: &str, value: &str) -> String {
    const LETTERS: &[u8] = b"abcdefghjkmnpqrstuvwxyz";
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"seek-env-mask-v1\0");
    hasher.update(key.as_bytes());
    hasher.update(b"\0");
    hasher.update(value.as_bytes());
    let hash = hasher.finalize();
    let bytes = hash.as_bytes();
    let len = 8 + (bytes[0] as usize % 6); // 8..=13 characters, unrelated to value.len()
    let mut out = String::with_capacity(len);
    for i in 0..len {
        let idx = bytes[1 + i] as usize % LETTERS.len();
        out.push(LETTERS[idx] as char);
    }
    out
}

#[derive(Clone, PartialEq, Eq)]
pub enum EnvLine {
    /// Blank line, comment, or non-assignment line (safe to display as-is).
    Plain { line_index: usize, text: String },
    /// `prefix` (leading whitespace + optional `export `), `key`, `eq` (`=`),
    /// `open_quote`, secret value (hidden by default), `close_quote`, `suffix`
    /// (trailing whitespace / inline `# comment`).
    Entry {
        line_index: usize,
        prefix: String,
        key: String,
        eq: String,
        open_quote: String,
        masked_value: String,
        real_value: String,
        close_quote: String,
        suffix: String,
    },
}

/// Custom `Debug` that never logs `real_value`.
impl fmt::Debug for EnvLine {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Plain { line_index, text } => f
                .debug_struct("Plain")
                .field("line_index", line_index)
                .field("text", text)
                .finish(),
            Self::Entry {
                line_index,
                key,
                masked_value,
                ..
            } => f
                .debug_struct("Entry")
                .field("line_index", line_index)
                .field("key", key)
                .field("value", &format!("<masked:{masked_value}>"))
                .finish(),
        }
    }
}

impl EnvLine {
    pub fn line_index(&self) -> usize {
        match self {
            Self::Plain { line_index, .. } | Self::Entry { line_index, .. } => *line_index,
        }
    }

    pub fn has_secret(&self) -> bool {
        matches!(self, Self::Entry { real_value, .. } if !real_value.is_empty())
    }

    /// Formats one line, substituting `masked_value` whenever the secret is
    /// not revealed. Never exposes `real_value` unless `revealed == true`.
    pub fn render(&self, revealed: bool) -> String {
        match self {
            Self::Plain { text, .. } => text.clone(),
            Self::Entry {
                prefix,
                key,
                eq,
                open_quote,
                masked_value,
                real_value,
                close_quote,
                suffix,
                ..
            } => {
                let shown = if revealed || real_value.is_empty() {
                    real_value.as_str()
                } else {
                    masked_value.as_str()
                };
                format!("{prefix}{key}{eq}{open_quote}{shown}{close_quote}{suffix}")
            }
        }
    }
}

#[derive(Clone, Debug)]
pub struct MaskedEnv {
    pub lines: Vec<EnvLine>,
}

impl MaskedEnv {
    pub fn parse(text: &str) -> Self {
        let lines = text
            .split('\n')
            .enumerate()
            .map(|(idx, raw)| {
                let clean = raw.trim_end_matches('\r');
                parse_line(idx, clean)
            })
            .collect();
        Self { lines }
    }

    pub fn has_any_secret(&self) -> bool {
        self.lines.iter().any(EnvLine::has_secret)
    }

    /// Renders the full document with only `reveal_all` or specifically
    /// `revealed_lines` showing real values. All other values use their
    /// deterministic masked placeholders so clipboard copies cannot leak them.
    pub fn render(&self, reveal_all: bool, revealed_lines: &HashSet<usize>) -> String {
        self.lines
            .iter()
            .map(|line| {
                let revealed = reveal_all || revealed_lines.contains(&line.line_index());
                line.render(revealed)
            })
            .collect::<Vec<_>>()
            .join("\n")
    }
}

fn parse_line(line_index: usize, line: &str) -> EnvLine {
    let bytes = line.as_bytes();
    let mut i = 0;
    while i < bytes.len() && matches!(bytes[i], b' ' | b'\t') {
        i += 1;
    }
    if i >= bytes.len() || bytes[i] == b'#' {
        return EnvLine::Plain {
            line_index,
            text: line.to_owned(),
        };
    }
    let mut key_start = i;
    if line[i..].starts_with("export ") || line[i..].starts_with("export\t") {
        let mut after = i + 6;
        while after < bytes.len() && matches!(bytes[after], b' ' | b'\t') {
            after += 1;
        }
        key_start = after;
    }
    let Some(eq_rel) = line[key_start..].find('=') else {
        return EnvLine::Plain {
            line_index,
            text: line.to_owned(),
        };
    };
    let eq_pos = key_start + eq_rel;
    let raw_key = &line[key_start..eq_pos];
    let key_trimmed = raw_key.trim_end();
    if key_trimmed.is_empty()
        || !key_trimmed
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'.' | b'-'))
    {
        return EnvLine::Plain {
            line_index,
            text: line.to_owned(),
        };
    }

    let prefix = line[..key_start].to_owned();
    let key = key_trimmed.to_owned();
    let eq = line[key_start + key_trimmed.len()..=eq_pos].to_owned();
    let rest = &line[eq_pos + 1..];
    let leading_ws = rest.len() - rest.trim_start().len();
    let val_slice = &rest[leading_ws..];

    let (open_quote, real_value, close_quote, suffix) = if val_slice.is_empty() {
        (String::new(), String::new(), String::new(), rest.to_owned())
    } else if val_slice.starts_with('"') || val_slice.starts_with('\'') {
        let q = val_slice.as_bytes()[0];
        let q_char = q as char;
        let vb = val_slice.as_bytes();
        let mut j = 1;
        let mut closed = false;
        while j < vb.len() {
            if vb[j] == b'\\' && j + 1 < vb.len() {
                j += 2;
                continue;
            }
            if vb[j] == q {
                closed = true;
                break;
            }
            j += 1;
        }
        let open = format!("{}{q_char}", &rest[..leading_ws]);
        if closed {
            (
                open,
                val_slice[1..j].to_owned(),
                q_char.to_string(),
                val_slice[j + 1..].to_owned(),
            )
        } else {
            (open, val_slice[1..].to_owned(), String::new(), String::new())
        }
    } else {
        // Unquoted value: inline comment starts at whitespace + `#`.
        let (val_part, comment_part) = match val_slice.find(" #").or_else(|| val_slice.find("\t#")) {
            Some(pos) => (&val_slice[..pos], &val_slice[pos..]),
            None => (val_slice, ""),
        };
        let trimmed = val_part.trim_end();
        let trailing_ws = &val_part[trimmed.len()..];
        (
            rest[..leading_ws].to_owned(),
            trimmed.to_owned(),
            String::new(),
            format!("{trailing_ws}{comment_part}"),
        )
    };

    let masked_value = if real_value.is_empty() {
        String::new()
    } else {
        mask_value(&key, &real_value)
    };

    EnvLine::Entry {
        line_index,
        prefix,
        key,
        eq,
        open_quote,
        masked_value,
        real_value,
        close_quote,
        suffix,
    }
}

/// Mask every value in an `.env` text buffer (used for MCP `read_text`,
/// default read views, and logs).
pub fn mask_all(text: &str) -> String {
    MaskedEnv::parse(text).render(false, &HashSet::new())
}

/// Mask matching snippet lines from content search (`"12: API_KEY=secret"` ->
/// `"12: API_KEY=<masked>"`), ensuring secret values never appear in snippets.
pub fn mask_snippet(snippet: &str) -> String {
    snippet
        .lines()
        .enumerate()
        .map(|(idx, line)| {
            if let Some((num, body)) = line.split_once(": ")
                && num.chars().all(|c| c.is_ascii_digit())
            {
                let masked = parse_line(idx, body).render(false);
                format!("{num}: {masked}")
            } else {
                parse_line(idx, line).render(false)
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_all_env_filename_patterns() {
        assert!(is_env_file(Path::new(r"C:\app\.env")));
        assert!(is_env_file(Path::new(r"C:\app\.env.local")));
        assert!(is_env_file(Path::new(r"C:\app\.env.production")));
        assert!(is_env_file(Path::new(r"C:\app\staging.env")));
        assert!(is_env_file(Path::new(r"C:\app\.ENV.Test")));
        assert!(!is_env_file(Path::new(r"C:\app\environment.txt")));
        assert!(!is_env_file(Path::new(r"C:\app\env.rs")));
    }

    #[test]
    fn hidden_values_never_leak_content_or_length_and_stay_deterministic() {
        let short = "a";
        let long = "sk_live_0123456789abcdefghijklmnopqrstuvwxyz_super_secret_token_9999";
        let m1 = mask_value("API_KEY", short);
        let m2 = mask_value("API_KEY", long);
        assert_eq!(m1, mask_value("API_KEY", short), "deterministic");
        assert_ne!(m1, m2, "distinct per value");
        assert!((8..=13).contains(&m1.len()));
        assert!((8..=13).contains(&m2.len()));
        assert_ne!(m1.len(), short.len());
        assert_ne!(m2.len(), long.len());
        assert!(m1.chars().all(|c| c.is_ascii_lowercase()));
        assert!(m2.chars().all(|c| c.is_ascii_lowercase()));
    }

    #[test]
    fn parsed_env_keeps_keys_and_comments_and_never_leaks_to_clipboard_snippet_or_logs() {
        let raw = concat!(
            "# Database credentials\n",
            "DB_PASS=top_secret_pw_99\n",
            "export STRIPE_KEY=\"sk_live_verysecretvalue\" # prod\n",
            "EMPTY_KEY=\n"
        );
        let parsed = MaskedEnv::parse(raw);
        let rendered = parsed.render(false, &HashSet::new());
        assert!(rendered.contains("# Database credentials"));
        assert!(rendered.contains("DB_PASS="));
        assert!(rendered.contains("export STRIPE_KEY=\""));
        assert!(rendered.contains("# prod"));
        assert!(rendered.contains("EMPTY_KEY="));
        assert!(!rendered.contains("top_secret_pw_99"));
        assert!(!rendered.contains("sk_live_verysecretvalue"));

        // Revealing a single line reveals ONLY that value.
        let mut one = HashSet::new();
        one.insert(1);
        let partial = parsed.render(false, &one);
        assert!(partial.contains("top_secret_pw_99"));
        assert!(!partial.contains("sk_live_verysecretvalue"));

        // Revealing all shows both; hiding again removes both.
        let all = parsed.render(true, &HashSet::new());
        assert!(all.contains("top_secret_pw_99"));
        assert!(all.contains("sk_live_verysecretvalue"));

        // Debug output never contains secrets (logs stay clean).
        let debug_str = format!("{parsed:?}");
        assert!(!debug_str.contains("top_secret_pw_99"));
        assert!(!debug_str.contains("sk_live_verysecretvalue"));

        // Content search snippet masking never leaks secrets.
        let snippet = "2: DB_PASS=top_secret_pw_99\n3: STRIPE_KEY=\"sk_live_verysecretvalue\"";
        let masked_snippet = mask_snippet(snippet);
        assert!(masked_snippet.contains("2: DB_PASS="));
        assert!(!masked_snippet.contains("top_secret_pw_99"));
        assert!(!masked_snippet.contains("sk_live_verysecretvalue"));
    }
}
