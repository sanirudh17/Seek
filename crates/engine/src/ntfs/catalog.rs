//! Temporary metadata namespace from one bounded kernel layout pass.
//! Access checks happen before any subtree is returned to the client.
use super::{graft_subtree, insert, record::Record};
use crate::{
    live::OEnt,
    walk::{self, Listing, NONE},
};
use std::{
    collections::{HashMap, HashSet},
    io,
};

#[derive(Default)]
pub(super) struct Catalog {
    directories: Vec<(u64, bool)>,
    directory_ids: HashMap<u64, u32>,
    children: HashMap<u64, Listing>,
}
pub(super) struct Scoped {
    pub listings: Vec<Listing>,
    pub denied: Vec<(Vec<u8>, u32)>,
}
fn empty(id: u32) -> Listing {
    Listing {
        id,
        names: vec![],
        ents: vec![],
    }
}
struct Pending {
    id: u64,
    path: Vec<u8>,
    target: u32,
    provider: bool,
}
fn authorize_pending(
    pending: &mut Vec<Pending>,
    out: &mut Vec<Listing>,
    queue: &mut Vec<(u64, Vec<u8>, u32)>,
    denied: &mut Vec<(Vec<u8>, u32)>,
    authorize: &mut impl FnMut(&[(u64, &[u8])]) -> Vec<io::Result<u64>>,
    provider: &mut impl FnMut(&[u8]) -> Vec<Listing>,
) -> io::Result<()> {
    let requests: Vec<_> = pending.iter().map(|p| (p.id, p.path.as_slice())).collect();
    let results = authorize(&requests);
    if results.len() != pending.len() {
        return Err(super::bad("incomplete directory access batch"));
    }
    for (job, result) in pending.drain(..).zip(results) {
        match result {
            Ok(actual) if actual == job.id => {
                if job.provider {
                    graft_subtree(out, job.target, provider(&job.path));
                } else {
                    queue.push((job.id, job.path, job.target));
                }
            }
            Ok(_) => return Err(super::bad("NTFS directory changed during scan")),
            Err(error) => denied.push((job.path, error.raw_os_error().unwrap_or(5) as u32)),
        }
    }
    Ok(())
}
impl Catalog {
    pub fn ensure_volume_root(&mut self, record: Record) -> io::Result<()> {
        // Some layout providers omit reserved record 5. Its identity/metadata
        // has already been validated through one kernel record read. Other
        // missing directories must still fail the completeness check.
        if record.id & 0x0000_ffff_ffff_ffff == 5 && !self.directory_ids.contains_key(&record.id) {
            self.add(record)?;
        }
        Ok(())
    }
    pub fn add(&mut self, record: Record) -> io::Result<()> {
        let ordinal = record.id & 0x0000_ffff_ffff_ffff;
        if ordinal < 16 && ordinal != 5 {
            return Ok(());
        }
        let child = if record.directory {
            let child = u32::try_from(self.directories.len()).map_err(io::Error::other)?;
            if self.directory_ids.insert(record.id, child).is_some() {
                return Err(super::bad("duplicate NTFS directory layout"));
            }
            self.directories
                .push((record.id, record.attributes.unwrap_or(0) & 0x400 != 0));
            child
        } else {
            NONE
        };
        // The volume root's self name is outside the ordinary directory namespace.
        if ordinal == 5 {
            return Ok(());
        }
        for name in &record.names {
            insert(
                self.children.entry(name.parent).or_insert_with(|| empty(0)),
                &name.text,
                record.kind(),
                if record.directory {
                    0
                } else {
                    record.size.unwrap_or(0)
                },
                record
                    .mtime
                    .ok_or_else(|| super::bad("missing NTFS layout date"))?,
                child,
            )?;
        }
        Ok(())
    }
    pub fn scope(
        mut self,
        root_id: u64,
        root: &[u8],
        cancelled: impl Fn() -> bool,
        mut authorize: impl FnMut(&[(u64, &[u8])]) -> Vec<io::Result<u64>>,
        mut metadata: impl FnMut(&[u8]) -> Option<OEnt>,
        mut provider: impl FnMut(&[u8]) -> Vec<Listing>,
    ) -> io::Result<Scoped> {
        if !self.directory_ids.contains_key(&root_id) {
            return Err(super::bad("NTFS layout omitted selected root"));
        }
        let mut out = vec![empty(0)];
        let mut denied = vec![];
        let mut queue = vec![(root_id, root.to_vec(), 0u32)];
        let mut visited = HashSet::new();
        let mut pending = Vec::with_capacity(256);
        while !queue.is_empty() || !pending.is_empty() {
            if cancelled() {
                return Err(io::Error::new(
                    io::ErrorKind::Interrupted,
                    "NTFS scan cancelled",
                ));
            }
            let Some((id, path, target)) = queue.pop() else {
                authorize_pending(
                    &mut pending,
                    &mut out,
                    &mut queue,
                    &mut denied,
                    &mut authorize,
                    &mut provider,
                )?;
                continue;
            };
            if !visited.insert(id) {
                return Err(super::bad("NTFS directory cycle"));
            }
            let mut listing = self.children.remove(&id).unwrap_or_else(|| empty(target));
            listing.id = target;
            for entry in &mut listing.ents {
                let directory = if entry.child == NONE {
                    None
                } else {
                    Some(
                        *self
                            .directories
                            .get(entry.child as usize)
                            .ok_or_else(|| super::bad("NTFS directory reference"))?,
                    )
                };
                let reparse =
                    entry.kind & 3 == walk::KIND_LINK || directory.is_some_and(|(_, r)| r);
                let child_path = if directory.is_some() || reparse {
                    Some(crate::live::join(
                        &path,
                        &listing.names[entry.name_off as usize..][..entry.name_len as usize],
                    ))
                } else {
                    None
                };
                if reparse {
                    let native = metadata(child_path.as_ref().unwrap()).ok_or_else(|| {
                        super::bad(
                            "NTFS reparse metadata unavailable; namespace reconciliation required",
                        )
                    })?;
                    entry.kind = native.kind;
                    entry.mtime = native.mtime;
                    if directory.is_none() {
                        entry.size = native.size;
                    }
                }
                entry.child = NONE;
                if let Some((child, reparse)) = directory
                    && entry.kind & 3 == walk::KIND_DIR
                {
                    let child_path = child_path.unwrap();
                    let next = u32::try_from(out.len()).map_err(io::Error::other)?;
                    out.push(empty(next));
                    entry.child = next;
                    pending.push(Pending {
                        id: child,
                        path: child_path,
                        target: next,
                        provider: reparse,
                    });
                    if pending.len() == 256 {
                        authorize_pending(
                            &mut pending,
                            &mut out,
                            &mut queue,
                            &mut denied,
                            &mut authorize,
                            &mut provider,
                        )?;
                    }
                }
            }
            out[target as usize] = listing;
        }
        Ok(Scoped {
            listings: out,
            denied,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::super::record::Name;
    use super::*;
    fn record(id: u64, parent: u64, name: &str, dir: bool, attributes: u32) -> Record {
        Record {
            id,
            directory: dir,
            attributes: Some(attributes),
            mtime: Some(123),
            size: Some(789),
            names: vec![Name {
                parent,
                text: name.as_bytes().to_vec(),
                namespace: 1,
            }],
            ..Default::default()
        }
    }
    fn names(listing: &Listing) -> Vec<String> {
        listing
            .ents
            .iter()
            .map(|e| {
                String::from_utf8_lossy(
                    &listing.names[e.name_off as usize..][..e.name_len as usize],
                )
                .into_owned()
            })
            .collect()
    }
    #[test]
    fn out_of_order_metadata_keeps_scoped_hard_links_and_denies_descendants() {
        let mut c = Catalog::default();
        let mut linked = record(40, 30, "outside.pdf", false, 0);
        linked.names.push(Name {
            parent: 20,
            text: b"inside.pdf".to_vec(),
            namespace: 1,
        });
        for r in [
            linked,
            record(41, 21, "secret.txt", false, 0),
            record(21, 20, "denied", true, 16),
            record(30, 5, "outside", true, 16),
            record(20, 5, "selected", true, 16),
            record(5, 5, ".", true, 16),
        ] {
            c.add(r).unwrap();
        }
        let scoped = c
            .scope(
                20,
                b"/C:/selected",
                || false,
                |paths| {
                    assert_eq!(paths, [(21, b"/C:/selected/denied".as_slice())]);
                    vec![Err(io::Error::from_raw_os_error(5))]
                },
                |_| panic!("ordinary metadata needs no per-file lookup"),
                |_| panic!("ordinary folder needs no walk"),
            )
            .unwrap();
        assert_eq!(names(&scoped.listings[0]), ["inside.pdf", "denied"]);
        assert_eq!(scoped.listings[0].ents[0].size, 789);
        assert!(scoped.listings[1].ents.is_empty());
        assert_eq!(scoped.denied, [(b"/C:/selected/denied".to_vec(), 5)]);
    }
    #[test]
    fn provider_children_are_grafted_and_redirecting_links_are_not_followed() {
        let mut c = Catalog::default();
        for r in [
            record(20, 5, "cloud", true, 16 | 0x400),
            record(21, 5, "loop", true, 16 | 0x400),
            record(5, 5, ".", true, 16),
        ] {
            c.add(r).unwrap();
        }
        let scoped = c
            .scope(
                5,
                b"/C:",
                || false,
                |paths| {
                    assert_eq!(paths, [(20, b"/C:/cloud".as_slice())]);
                    vec![Ok(20)]
                },
                |p| {
                    Some(OEnt::new(
                        p,
                        if p.ends_with(b"cloud") {
                            walk::KIND_DIR
                        } else {
                            walk::KIND_LINK
                        },
                        0,
                        321,
                    ))
                },
                |p| {
                    assert_eq!(p, b"/C:/cloud");
                    let mut listing = empty(0);
                    insert(
                        &mut listing,
                        b"provider-only.txt",
                        walk::KIND_FILE,
                        11,
                        321,
                        NONE,
                    )
                    .unwrap();
                    vec![listing]
                },
            )
            .unwrap();
        assert_eq!(names(&scoped.listings[1]), ["provider-only.txt"]);
        assert_eq!(scoped.listings[0].ents[1].child, NONE);
    }
    #[test]
    fn missing_root_duplicate_directories_and_cancellation_fail_closed() {
        assert!(
            Catalog::default()
                .scope(5, b"/C:", || false, |_| vec![], |_| None, |_| vec![])
                .is_err()
        );
        let mut c = Catalog::default();
        c.add(record(5, 5, ".", true, 16)).unwrap();
        assert!(c.add(record(5, 5, ".", true, 16)).is_err());
        assert!(
            c.scope(5, b"/C:", || true, |_| vec![], |_| None, |_| vec![])
                .is_err()
        );
        let mut c = Catalog::default();
        c.ensure_volume_root(record(5, 5, ".", true, 16)).unwrap();
        c.ensure_volume_root(record(5, 5, ".", true, 16)).unwrap();
        assert_eq!(
            c.scope(5, b"/C:", || false, |_| vec![], |_| None, |_| vec![])
                .unwrap()
                .listings
                .len(),
            1
        );
        let mut c = Catalog::default();
        c.ensure_volume_root(record(20, 5, "selected", true, 16))
            .unwrap();
        assert!(
            c.scope(
                20,
                b"/C:/selected",
                || false,
                |_| vec![],
                |_| None,
                |_| vec![]
            )
            .is_err()
        );
    }
    #[test]
    fn unreadable_reparse_metadata_cannot_silently_hide_provider_descendants() {
        let mut c = Catalog::default();
        c.add(record(5, 5, ".", true, 16)).unwrap();
        c.add(record(20, 5, "cloud", true, 16 | 0x400)).unwrap();
        assert!(
            c.scope(5, b"/C:", || false, |_| vec![Ok(20)], |_| None, |_| vec![])
                .is_err()
        );
    }
    #[test]
    fn bounded_access_batches_keep_all_siblings_and_do_not_visit_denied_children() {
        let mut c = Catalog::default();
        c.add(record(5, 5, ".", true, 16)).unwrap();
        for id in 20..620 {
            c.add(record(id, 5, &format!("folder-{id}"), true, 16))
                .unwrap();
            c.add(record(id + 1000, id, "found.txt", false, 0)).unwrap();
        }
        c.add(record(2000, 20, "private-child", true, 16)).unwrap();
        c.add(record(2001, 2000, "secret.txt", false, 0)).unwrap();
        let mut checked = 0;
        let mut batches = vec![];
        let scoped = c
            .scope(
                5,
                b"/C:",
                || false,
                |paths| {
                    assert!(paths.len() <= 256);
                    checked += paths.len();
                    batches.push(paths.len());
                    paths
                        .iter()
                        .map(|&(id, _)| {
                            assert_ne!(id, 2000, "a denied parent must stop traversal");
                            if id == 20 {
                                Err(io::Error::from_raw_os_error(5))
                            } else {
                                Ok(id)
                            }
                        })
                        .collect()
                },
                |_| panic!("ordinary metadata requires no lookup"),
                |_| panic!("ordinary metadata requires no walk"),
            )
            .unwrap();
        assert_eq!(batches, [256, 256, 88]);
        assert_eq!(checked, 600);
        assert_eq!(scoped.listings.len(), 601);
        assert_eq!(scoped.listings[0].ents.len(), 600);
        assert_eq!(
            scoped
                .listings
                .iter()
                .skip(1)
                .map(|l| l.ents.len())
                .sum::<usize>(),
            599
        );
        assert_eq!(scoped.denied, [(b"/C:/folder-20".to_vec(), 5)]);
    }
    #[test]
    fn incomplete_access_batch_fails_instead_of_publishing_missing_directories() {
        let mut c = Catalog::default();
        c.add(record(5, 5, ".", true, 16)).unwrap();
        c.add(record(20, 5, "child", true, 16)).unwrap();
        let error = c
            .scope(5, b"/C:", || false, |_| vec![], |_| None, |_| vec![])
            .err()
            .unwrap();
        assert!(
            error
                .to_string()
                .contains("incomplete directory access batch")
        );
    }
}
