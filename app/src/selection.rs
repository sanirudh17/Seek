//! Explorer-style range anchor, independent of the moving keyboard caret.
use crate::backend::Row;
use std::{collections::HashSet, path::PathBuf};
pub struct Input {
    pub shift: bool,
    pub control: bool,
    pub keyboard: bool,
}
pub fn change(
    rows: &[Row],
    selected: &mut HashSet<PathBuf>,
    anchor: &mut Option<PathBuf>,
    current: Option<usize>,
    target: usize,
    input: Input,
) {
    let Input {
        shift,
        control,
        keyboard,
    } = input;
    let Some(row) = rows.get(target) else {
        return;
    };
    if shift {
        let from = anchor
            .as_ref()
            .and_then(|p| rows.iter().position(|r| &r.file.path == p))
            .or(current.filter(|i| *i < rows.len()))
            .unwrap_or(target);
        if anchor.is_none() {
            *anchor = Some(rows[from].file.path.clone());
        }
        if !control {
            selected.clear();
        }
        selected.extend(
            rows[from.min(target)..=from.max(target)]
                .iter()
                .map(|r| r.file.path.clone()),
        );
    } else if control && keyboard {
        // Move the caret without changing the selected set or its anchor.
    } else {
        if !control {
            selected.clear();
        }
        if !control || !selected.remove(&row.file.path) {
            selected.insert(row.file.path.clone());
        }
        *anchor = Some(row.file.path.clone());
    }
}
pub fn next(current: Option<usize>, len: usize, key: &str, columns: usize) -> Option<usize> {
    if len == 0 {
        return None;
    }
    let i = current.unwrap_or(0).min(len - 1);
    let step = if matches!(key, "up" | "down") {
        columns.max(1)
    } else {
        1
    };
    Some(if current.is_none() {
        0
    } else if matches!(key, "down" | "right") {
        i.saturating_add(step).min(len - 1)
    } else {
        i.saturating_sub(step)
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn range_anchor_survives_reversal_and_control_movement() {
        let rows: Vec<_> = (0..7)
            .map(|i| {
                Row::new(seek_engine::Found {
                    path: format!("owned-{i}.txt").into(),
                    kind: 0,
                    mtime: 0,
                    size: 1,
                    score: 0,
                })
            })
            .collect();
        let (mut selected, mut anchor) = (HashSet::new(), None);
        change(
            &rows,
            &mut selected,
            &mut anchor,
            None,
            2,
            Input {
                shift: false,
                control: false,
                keyboard: false,
            },
        );
        change(
            &rows,
            &mut selected,
            &mut anchor,
            Some(2),
            5,
            Input {
                shift: true,
                control: false,
                keyboard: true,
            },
        );
        assert_eq!(selected.len(), 4);
        change(
            &rows,
            &mut selected,
            &mut anchor,
            Some(5),
            3,
            Input {
                shift: true,
                control: false,
                keyboard: true,
            },
        );
        assert_eq!(selected.len(), 2);
        let before = selected.clone();
        change(
            &rows,
            &mut selected,
            &mut anchor,
            Some(3),
            6,
            Input {
                shift: false,
                control: true,
                keyboard: true,
            },
        );
        assert_eq!(selected, before);
        change(
            &rows,
            &mut selected,
            &mut anchor,
            Some(6),
            6,
            Input {
                shift: false,
                control: true,
                keyboard: false,
            },
        );
        assert_eq!(selected.len(), 3);
        change(
            &rows,
            &mut selected,
            &mut anchor,
            Some(6),
            3,
            Input {
                shift: true,
                control: false,
                keyboard: false,
            },
        );
        assert_eq!(selected.len(), 4);
        assert_eq!(next(Some(2), 7, "down", 3), Some(5));
        assert_eq!(next(Some(6), 7, "right", 3), Some(6));
        selected.clear();
        anchor = None;
        change(
            &rows,
            &mut selected,
            &mut anchor,
            Some(99),
            1,
            Input {
                shift: true,
                control: false,
                keyboard: true,
            },
        );
        assert_eq!(selected.len(), 1);
        change(
            &rows,
            &mut selected,
            &mut anchor,
            Some(1),
            1,
            Input {
                shift: false,
                control: true,
                keyboard: false,
            },
        );
        assert!(
            selected.is_empty(),
            "Ctrl+click can deselect the final item"
        );
    }
}
