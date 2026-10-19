//! Display-only ranges; the vendored matcher remains the authority for results.
use seek_engine::query::{Mode, Query, fuzzy_score};
use std::ops::Range;

pub fn ranges(name: &str, query: &Query) -> Vec<Range<usize>> {
    // Map case-folded characters back to the original UTF-8 boundaries, including
    // characters whose lowercase representation expands to multiple characters.
    let mut lower = String::new();
    let mut positions = Vec::new();
    for (start, ch) in name.char_indices() {
        for folded in ch.to_lowercase() {
            positions.push((lower.len(), start..start + ch.len_utf8()));
            lower.push(folded);
        }
    }
    let mut marked = vec![false; positions.len()];
    for token in query.tokens.iter().filter(|t| !t.negate) {
        let Ok(text) = std::str::from_utf8(&token.text) else {
            continue;
        };
        let literal = match token.mode {
            Mode::Prefix => lower.starts_with(text).then_some(0),
            Mode::Suffix => lower.ends_with(text).then(|| lower.len() - text.len()),
            _ => lower.find(text),
        };
        if let Some(start) = literal {
            for (i, (offset, _)) in positions.iter().enumerate() {
                if (start..start + text.len()).contains(offset) {
                    marked[i] = true;
                }
            }
        } else if token.mode == Mode::Fuzzy && fuzzy_score(lower.as_bytes(), &token.text).is_some()
        {
            // LCS highlights the surviving letters for subsequences and one-typo
            // matches. Bound this display work even for extreme NTFS names.
            let chars: Vec<_> = lower.chars().collect();
            let wanted: Vec<_> = text.chars().collect();
            if chars.len() > 512 || wanted.len() > 128 {
                continue;
            }
            let width = wanted.len() + 1;
            let mut lengths = vec![0u16; (chars.len() + 1) * width];
            for i in 1..=chars.len() {
                for j in 1..=wanted.len() {
                    lengths[i * width + j] = if chars[i - 1] == wanted[j - 1] {
                        lengths[(i - 1) * width + j - 1] + 1
                    } else {
                        lengths[(i - 1) * width + j].max(lengths[i * width + j - 1])
                    };
                }
            }
            let (mut i, mut j) = (chars.len(), wanted.len());
            while i > 0 && j > 0 {
                if chars[i - 1] == wanted[j - 1] {
                    marked[i - 1] = true;
                    i -= 1;
                    j -= 1;
                } else if lengths[(i - 1) * width + j] >= lengths[i * width + j - 1] {
                    i -= 1;
                } else {
                    j -= 1;
                }
            }
        }
    }
    let mut ranges: Vec<Range<usize>> = Vec::new();
    for ((_, range), marked) in positions.into_iter().zip(marked) {
        if marked {
            if let Some(last) = ranges.last_mut()
                && range.start <= last.end
            {
                last.end = last.end.max(range.end);
            } else {
                ranges.push(range);
            }
        }
    }
    ranges
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn highlights_preserve_unicode_boundaries_and_ignore_filters_and_exclusions() {
        let name = "Été-📷-Invoice.txt";
        let query = Query::parse("été invoice ext:txt !budget", "").unwrap();
        let parts: Vec<_> = ranges(name, &query).into_iter().map(|r| &name[r]).collect();
        assert_eq!(parts, ["Été", "Invoice"]);
        assert!(ranges(name, &Query::parse("ext:txt !invoice", "").unwrap()).is_empty());
    }
    #[test]
    fn highlights_typo_survivors_and_literal_suffix() {
        let name = "Invoice.txt";
        let ranges = ranges(name, &Query::parse("invoce txt$", "").unwrap());
        let parts: Vec<_> = ranges.into_iter().map(|r| &name[r]).collect();
        assert_eq!(parts, ["Invo", "ce", "txt"]);
    }
}
