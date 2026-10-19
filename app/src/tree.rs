//! Visible list order, independent of rendering. Children are supplied by the index.
use crate::backend::Row;
use gpui_component::table::ColumnSort;
use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
};

pub fn flatten(
    base: &[Row],
    children: &mut HashMap<PathBuf, Vec<Row>>,
    expanded: &HashSet<PathBuf>,
    sort: Option<(usize, ColumnSort)>,
) -> (Vec<Row>, Vec<usize>) {
    let compare = |a: &Row, b: &Row| {
        let (column, direction) = sort.unwrap_or((0, ColumnSort::Ascending));
        let order = match column {
            0 => seek_engine::paths::cmp_ci(&a.sort_name, &b.sort_name),
            1 => a.file.mtime.cmp(&b.file.mtime),
            2 => a.file.size.cmp(&b.file.size),
            3 => a.kind.cmp(&b.kind),
            _ => seek_engine::paths::cmp_ci(
                &seek_engine::paths::internal(
                    a.file
                        .path
                        .parent()
                        .unwrap_or_else(|| std::path::Path::new("")),
                ),
                &seek_engine::paths::internal(
                    b.file
                        .path
                        .parent()
                        .unwrap_or_else(|| std::path::Path::new("")),
                ),
            ),
        }
        .then_with(|| seek_engine::paths::cmp_ci(&a.sort_name, &b.sort_name));
        if direction == ColumnSort::Descending {
            order.reverse()
        } else {
            order
        }
    };
    let mut roots: Vec<_> = base.iter().collect();
    if sort.is_some() {
        roots.sort_by(|a, b| compare(a, b));
    }
    for items in children.values_mut() {
        items.sort_by(compare);
    }
    let expanded_roots: Vec<_> = expanded
        .iter()
        .filter(|p| base.iter().any(|r| r.file.path == **p))
        .collect();
    let mut pending: Vec<_> = roots
        .into_iter()
        .rev()
        .filter(|row| {
            !expanded_roots
                .iter()
                .any(|p| row.file.path != ***p && row.file.path.starts_with(p))
        })
        .map(|row| (row, 0))
        .collect();
    let mut seen = HashSet::new();
    let (mut rows, mut depths) = (Vec::new(), Vec::new());
    while let Some((row, depth)) = pending.pop() {
        if !seen.insert(row.file.path.as_path()) {
            continue;
        }
        rows.push(row.clone());
        depths.push(depth);
        if expanded.contains(&row.file.path)
            && let Some(items) = children.get(&row.file.path)
        {
            pending.extend(items.iter().rev().map(|child| (child, depth + 1)));
        }
    }
    (rows, depths)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn row(path: &str, kind: u8, size: u64) -> Row {
        Row::new(seek_engine::Found {
            path: path.into(),
            kind,
            size,
            mtime: 1,
            score: 0,
        })
    }
    #[test]
    fn nested_expansion_sorts_each_level_and_deduplicates_search_hits() {
        let folder = row(r"C:\Test\folder", seek_engine::walk::KIND_DIR, 0);
        let nested = row(r"C:\Test\folder\Nested", seek_engine::walk::KIND_DIR, 0);
        let a = row(r"C:\Test\folder\a.txt", 0, 20);
        let z = row(r"C:\Test\folder\z.txt", 0, 10);
        let deep = row(r"C:\Test\folder\Nested\deep.txt", 0, 1);
        let mut children = HashMap::from([
            (
                folder.file.path.clone(),
                vec![z.clone(), nested.clone(), a.clone()],
            ),
            (nested.file.path.clone(), vec![deep.clone()]),
        ]);
        let mut expanded = HashSet::from([folder.file.path.clone(), nested.file.path.clone()]);
        let base = vec![z.clone(), folder.clone(), nested.clone(), deep.clone()];
        let (rows, depths) = flatten(
            &base,
            &mut children,
            &expanded,
            Some((0, ColumnSort::Ascending)),
        );
        assert_eq!(
            rows.iter().map(|r| r.name.as_str()).collect::<Vec<_>>(),
            ["folder", "a.txt", "Nested", "deep.txt", "z.txt"]
        );
        assert_eq!(depths, [0, 1, 1, 2, 1]);
        let (rows, _) = flatten(
            &base,
            &mut children,
            &expanded,
            Some((0, ColumnSort::Descending)),
        );
        assert_eq!(
            rows.iter().map(|r| r.name.as_str()).collect::<Vec<_>>(),
            ["folder", "z.txt", "Nested", "deep.txt", "a.txt"]
        );
        expanded.clear();
        let (rows, depths) = flatten(&[folder], &mut children, &expanded, None);
        assert_eq!(rows.len(), 1);
        assert_eq!(depths, [0]);
    }
}
