use super::*;
use std::ops::Bound::{Excluded, Included};

impl Live {
    /// Direct overlay children, skipping whole descendant ranges and unrelated
    /// folders. Resolve the caller's Windows casing before using byte ranges.
    pub(crate) fn overlay_children(
        &self,
        folder: &[u8],
    ) -> impl Iterator<Item = (&Vec<u8>, &OEnt)> {
        let mut root = normalize(folder);
        if let Some((path, _)) = self.over.get_key_value(&root) {
            root = path.clone();
        } else if let Some(entry) = self.base_alive(&root) {
            self.base.path(entry as usize, &mut root);
        } else if let Some((path, _)) = self.over.iter().find(|(path, entry)| {
            entry.kind & 3 == KIND_DIR && crate::paths::cmp_ci(path, &root).is_eq()
        }) {
            root = path.clone();
        }
        let (mut lower, upper) = subtree_bounds(&root);
        let prefix = lower.len();
        let mut exclude = false;
        std::iter::from_fn(move || {
            loop {
                let bound = if exclude {
                    Excluded(lower.clone())
                } else {
                    Included(lower.clone())
                };
                let (path, entry) = self.over.range((bound, Excluded(upper.clone()))).next()?;
                if let Some(slash) = path[prefix..].iter().position(|b| *b == b'/') {
                    // The parent entry was already yielded. Jump past its complete
                    // subtree instead of visiting every nested file.
                    lower = path[..prefix + slash].to_vec();
                    lower.push(b'0'); // Upper bound of the parent's '/' prefix.
                    exclude = false;
                    continue;
                }
                lower = path.clone();
                exclude = true;
                return Some((path, entry));
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bulk_update_fork_shares_the_mmap_but_keeps_readers_and_flat_overlay_unchanged() {
        let base = Index::build(
            vec![Listing {
                id: 0,
                names: b"a.txt".to_vec(),
                ents: vec![RawEnt {
                    name_off: 0,
                    name_len: 5,
                    kind: 0,
                    size: 1,
                    mtime: 1,
                    child: NONE,
                }],
            }],
            0,
            0,
            &[],
        );
        let mut old = Live::new(base);
        old.put(b"/b.txt".to_vec(), OEnt::new(b"/b.txt", 0, 1, 1));
        let mut next = old.fork_for_update();
        assert!(std::sync::Arc::ptr_eq(&old.base, &next.base));
        next.kill_subtree(1);
        next.put(b"/b.txt".to_vec(), OEnt::new(b"/b.txt", 0, 2, 2));
        next.put(b"/new.txt".to_vec(), OEnt::new(b"/new.txt", 0, 3, 3));
        assert!(!old.is_dead(1));
        assert!(next.is_dead(1));
        assert_eq!(old.over[b"/b.txt".as_slice()].size, 1);
        assert_eq!(next.over[b"/b.txt".as_slice()].size, 2);
        assert_eq!(old.flat.items[0].1.size, 1);
        assert_eq!(next.flat.items[0].1.size, 2);
        assert!(!old.over.contains_key(b"/new.txt".as_slice()));
    }

    #[test]
    fn direct_overlay_children_skip_other_folders_and_nested_subtrees() {
        let base = Index::build(
            vec![Listing {
                id: 0,
                names: vec![],
                ents: vec![],
            }],
            0,
            0,
            &[],
        );
        let mut live = Live::new(base);
        let folder = b"/C:/Downloads";
        live.put(folder.to_vec(), OEnt::new(folder, KIND_DIR, 0, 0));
        for path in [b"/C:/Downloads/a.txt".as_slice(), b"/C:/Downloads/z.txt"] {
            live.put(path.to_vec(), OEnt::new(path, 0, 1, 1));
        }
        let nested = b"/C:/Downloads/Album";
        live.put(nested.to_vec(), OEnt::new(nested, KIND_DIR, 0, 0));
        for i in 0..10_000 {
            for path in [format!("/C:/Other/{i}"), format!("/C:/Downloads/Album/{i}")] {
                live.put(
                    path.as_bytes().to_vec(),
                    OEnt::new(path.as_bytes(), 0, 1, 1),
                );
            }
        }
        let names = |live: &Live, root: &[u8]| {
            live.overlay_children(root)
                .map(|(p, _)| p.clone())
                .collect::<Vec<_>>()
        };
        let expected = vec![
            nested.to_vec(),
            b"/C:/Downloads/a.txt".to_vec(),
            b"/C:/Downloads/z.txt".to_vec(),
        ];
        assert_eq!(names(&live, folder), expected);
        assert_eq!(names(&live, b"/c:/DOWNLOADS"), expected);
        live.drop_over_subtree(nested);
        assert_eq!(names(&live, folder).len(), 2);
    }
}
