//! Task toggles operate on the source buffer, never directly on disk.
#[derive(Debug, PartialEq, Eq)]
pub struct Task {
    pub offset: usize,
    pub checked: bool,
    pub label: String,
}
pub fn tasks(text: &str) -> Vec<Task> {
    let mut out = Vec::new();
    let mut offset = 0;
    let mut fence: Option<(u8, usize)> = None;
    for line in text.split_inclusive('\n') {
        let trimmed = line.trim_start_matches([' ', '\t']);
        let indent = line.len() - trimmed.len();
        let marker = trimmed.as_bytes().first().copied();
        if matches!(marker, Some(b'`' | b'~')) {
            let m = marker.unwrap();
            let n = trimmed.bytes().take_while(|b| *b == m).count();
            if n >= 3 {
                match fence {
                    None => fence = Some((m, n)),
                    Some((old, count)) if old == m && n >= count => fence = None,
                    _ => {}
                }
                offset += line.len();
                continue;
            }
        }
        if fence.is_none() && indent < 4 {
            let bytes = trimmed.as_bytes();
            if bytes.len() >= 6
                && matches!(bytes[0], b'-' | b'*' | b'+')
                && bytes[1..3] == *b" ["
                && matches!(bytes[3], b' ' | b'x' | b'X')
                && bytes[4] == b']'
                && bytes[5].is_ascii_whitespace()
            {
                out.push(Task {
                    offset: offset + indent + 3,
                    checked: bytes[3] != b' ',
                    label: trimmed[6..].trim_end().to_owned(),
                });
            }
        }
        offset += line.len();
    }
    out
}
pub fn toggle(text: &str, offset: usize, expected: bool) -> Option<String> {
    let task = tasks(text)
        .into_iter()
        .find(|task| task.offset == offset && task.checked == expected)?;
    let mut output = text.to_owned();
    output.replace_range(
        task.offset..task.offset + 1,
        if task.checked { " " } else { "x" },
    );
    Some(output)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn toggles_unicode_task_markers_without_touching_code_blocks_or_line_endings() {
        let source =
            "- [ ] \u{4e2d}\u{6587}\r\n```md\n- [ ] example\n```\n    - [ ] code\n* [X] Done\n";
        let found = tasks(source);
        assert_eq!(found.len(), 2);
        assert!(!found[0].checked);
        assert!(found[1].checked);
        let draft = toggle(source, found[0].offset, false).unwrap();
        assert!(draft.starts_with("- [x] \u{4e2d}\u{6587}\r\n"));
        assert!(draft.contains("- [ ] example"));
        assert!(toggle(&draft, found[0].offset, false).is_none());
        assert_eq!(toggle(&draft, found[0].offset, true).unwrap(), source);
    }
    #[test]
    fn empty_and_non_task_bullets_are_not_editable_task_markers() {
        assert!(tasks("").is_empty());
        assert!(tasks("- ordinary\n- [z] invalid\n").is_empty());
        assert!(toggle("plain", 1, false).is_none());
    }
}
