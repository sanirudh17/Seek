//! Validate persisted dimensions before constructing typed mmap views, then
//! reject broken references instead of panicking during the first query.
use super::*;

pub(super) fn checked_sections(
    n: usize,
    d: usize,
    u: usize,
    names_len: usize,
    bytes: usize,
) -> Option<([usize; NSEC], usize)> {
    if n == 0 || d == 0 || u == 0 || d > n || u > n {
        return None;
    }
    for count in [n, d, u, names_len] {
        u32::try_from(count).ok()?;
    }
    // Every index contains several four-byte entry arrays. This bound also
    // keeps the original section arithmetic safe on smaller address spaces.
    if n > bytes.checked_sub(HDR)? / 4 {
        return None;
    }
    u.checked_add(1)?.checked_mul(4)?;
    u.div_ceil(64).checked_mul(NBITMAPS)?.checked_mul(8)?;
    n.div_ceil(ZONE).checked_mul(std::mem::size_of::<Zone>())?;
    let lens = section_lens(n, d, u, names_len);
    let mut offsets = [0; NSEC];
    let mut at = HDR;
    for (offset, len) in offsets.iter_mut().zip(lens) {
        *offset = at;
        at = at.checked_add(len)?.checked_add(63)? & !63;
    }
    (at <= bytes).then_some((offsets, at))
}

impl Index {
    pub(super) fn valid_structure(&self) -> bool {
        let monotonic = |values: &[u32], last: usize| {
            values.first() == Some(&0)
                && values.last().copied().map(|v| v as usize) == Some(last)
                && values.windows(2).all(|w| w[0] <= w[1])
        };
        if !monotonic(self.name_off(), self.names_len)
            || !monotonic(self.name_ents_off(), self.n)
            || self.ent_name().iter().any(|&v| v as usize >= self.u)
            || self.parent().iter().any(|&v| v as usize >= self.d)
            || self.dir_entry().first() != Some(&0)
            || self
                .dir_entry()
                .last()
                .is_none_or(|&v| v as usize >= self.n)
            || !self.dir_entry().windows(2).all(|w| w[0] < w[1])
            || self.dir_parent()[0] != 0
            || self.parent()[0] != 0
            || self.ext_table().iter().any(|ext| ext[0] > 15)
        {
            return false;
        }
        let mut children = 0usize;
        for d in 0..self.d {
            let e = self.dir_entry()[d] as usize;
            let start = self.dir_start()[d] as usize;
            let len = self.dir_len()[d] as usize;
            let end = self.dir_end()[d] as usize;
            if self.kind()[e] & 3 != KIND_DIR
                || self.parent()[e] != self.dir_parent()[d]
                || (d != 0 && self.dir_parent()[d] as usize >= d)
                || start <= e
                || start > self.n
                || len > self.n - start
                || end < start + len
                || end > self.n
            {
                return false;
            }
            if len > self.n - 1 - children {
                return false;
            }
            children += len;
        }
        if children != self.n - 1 || self.dir_end()[0] as usize != self.n {
            return false;
        }
        for (entry, &parent) in self.parent().iter().enumerate().skip(1) {
            let d = parent as usize;
            let start = self.dir_start()[d] as usize;
            if entry < start || entry - start >= self.dir_len()[d] as usize {
                return false;
            }
        }
        for (name, range) in self.name_ents_off().windows(2).enumerate() {
            let entries = &self.name_ents()[range[0] as usize..range[1] as usize];
            if entries
                .iter()
                .any(|&e| e as usize >= self.n || self.ent_name()[e as usize] as usize != name)
                || !entries.windows(2).all(|w| {
                    // The portable core orders a name's entries by descending
                    // directory prior, then entry ordinal within that prior.
                    let key = |e: u32| {
                        (
                            std::cmp::Reverse(self.dir_prior()[self.parent()[e as usize] as usize]),
                            e,
                        )
                    };
                    key(w[0]) < key(w[1])
                })
            {
                return false;
            }
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> Index {
        Index::build(
            vec![Listing {
                id: 0,
                names: vec![],
                ents: vec![],
            }],
            7,
            123,
            &[],
        )
    }

    #[test]
    fn duplicate_names_in_different_ranking_folders_remain_loadable() {
        let file = || Listing {
            id: 0,
            names: b"report.txt".to_vec(),
            ents: vec![crate::walk::RawEnt {
                name_off: 0,
                name_len: 10,
                kind: 0,
                size: 1,
                mtime: 42,
                child: crate::walk::NONE,
            }],
        };
        let mut a = file();
        a.id = 1;
        let mut b = file();
        b.id = 2;
        let root = Listing {
            id: 0,
            names: b"AAADownloads".to_vec(),
            ents: vec![
                crate::walk::RawEnt {
                    name_off: 0,
                    name_len: 3,
                    kind: KIND_DIR,
                    size: 0,
                    mtime: 0,
                    child: 1,
                },
                crate::walk::RawEnt {
                    name_off: 3,
                    name_len: 9,
                    kind: KIND_DIR,
                    size: 0,
                    mtime: 0,
                    child: 2,
                },
            ],
        };
        let index = Index::build(vec![root, a, b], 0, 42, &[]);
        let name = index.ent_name()[3] as usize;
        let off = index.name_ents_off();
        assert_eq!(
            &index.name_ents()[off[name] as usize..off[name + 1] as usize],
            &[4, 3]
        );
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("index.bin");
        index.save(&path).unwrap();
        assert_eq!(Index::load(&path).unwrap().n, 5);
    }

    #[test]
    fn malformed_saved_dimensions_are_rejected_without_panics() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("index.bin");
        fixture().save(&path).unwrap();
        let valid = std::fs::read(&path).unwrap();
        for (field, value) in [
            (0, u64::MAX),
            (1, u64::MAX),
            (2, u64::MAX),
            (3, u64::MAX),
            (5, u64::MAX),
            (0, 0),
        ] {
            let mut bytes = valid.clone();
            bytes[8 + field * 8..16 + field * 8].copy_from_slice(&value.to_le_bytes());
            std::fs::write(&path, bytes).unwrap();
            assert!(Index::load(&path).is_none(), "field {field}");
        }
        for length in [0, 55, HDR, valid.len() - 1] {
            std::fs::write(&path, &valid[..length]).unwrap();
            assert!(Index::load(&path).is_none());
        }
    }

    #[test]
    fn broken_mapped_references_are_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("index.bin");
        let index = fixture();
        index.save(&path).unwrap();
        let valid = std::fs::read(&path).unwrap();
        for section in [
            Sec::NameOff,
            Sec::EntName,
            Sec::Parent,
            Sec::DirEntry,
            Sec::DirParent,
            Sec::NameEnts,
        ] {
            let mut bytes = valid.clone();
            let at = index.off[section as usize];
            bytes[at..at + 4].copy_from_slice(&u32::MAX.to_le_bytes());
            std::fs::write(&path, bytes).unwrap();
            assert!(Index::load(&path).is_none());
        }
    }

    #[test]
    fn checkpoint_replacement_preserves_old_reader_and_matching_cursor() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("index.bin");
        let mut index = fixture();
        index.save_with_state(&path, Some(b"cursor-seven")).unwrap();
        let old = Index::load(&path).unwrap();
        index.event_id = 8;
        index.save_with_state(&path, Some(b"cursor-eight")).unwrap();
        let new = Index::load(&path).unwrap();
        assert_eq!(
            (old.event_id, old.saved_state()),
            (7, Some(b"cursor-seven".as_slice()))
        );
        assert_eq!(
            (new.event_id, new.saved_state()),
            (8, Some(b"cursor-eight".as_slice()))
        );
        // An unfinished temporary write cannot replace the last complete file.
        std::fs::write(path.with_extension("tmp"), b"interrupted").unwrap();
        assert_eq!(Index::load(&path).unwrap().event_id, 8);
    }
}
