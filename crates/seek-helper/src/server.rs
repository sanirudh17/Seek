use seek_engine::{
    helper::{self, Impersonation, Request, Response},
    kernel::Pipe,
    ntfs::{Cursor, JournalEntry, NtfsSource},
    walk::Listing,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    io::{self, BufWriter, Write},
    sync::{Arc, Mutex},
    thread,
};
struct Monitor {
    cancel: seek_engine::kernel::Cancel,
    thread: Option<thread::JoinHandle<()>>,
}
impl Drop for Monitor {
    fn drop(&mut self) {
        self.cancel.cancel();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn send_chunks(pipe: &mut Pipe, root: &[u8], listings: Vec<Listing>) -> io::Result<()> {
    // One small directory must not cost two overlapped writes and event handles.
    // Flush before journal replay/Ready, so buffering never delays idle updates.
    let mut pipe = BufWriter::with_capacity(1024 * 1024, pipe);
    for listing in listings {
        let count = listing.ents.len().div_ceil(256).max(1);
        for n in 0..count {
            let mut chunk = Listing {
                id: listing.id,
                names: vec![],
                ents: vec![],
            };
            for e in listing.ents.iter().skip(n * 256).take(256) {
                let mut entry = *e;
                entry.name_off = chunk.names.len() as u32;
                chunk
                    .names
                    .extend(&listing.names[e.name_off as usize..][..e.name_len as usize]);
                chunk.ents.push(entry);
            }
            helper::write_response(
                &mut pipe,
                &Response::Chunk {
                    root: root.to_vec(),
                    listing: chunk,
                    last: n + 1 == count,
                },
            )?;
        }
    }
    pipe.flush()
}
fn write_changes(
    pipe: &mut impl Write,
    roots: &[Vec<u8>],
    cursor: Cursor,
    paths: io::Result<BTreeSet<Vec<u8>>>,
) -> io::Result<()> {
    match paths {
        Ok(paths) => {
            let paths: Vec<_> = paths
                .into_iter()
                .filter(|p| {
                    roots.iter().any(|r| {
                        p.len() >= r.len()
                            && seek_engine::paths::cmp_ci(&p[..r.len()], r).is_eq()
                            && (p.len() == r.len() || p.get(r.len()) == Some(&b'/'))
                    })
                })
                .collect();
            let count = paths.len().div_ceil(256).max(1);
            for n in 0..count {
                helper::write_response(
                    pipe,
                    &Response::Changes {
                        paths: paths.iter().skip(n * 256).take(256).cloned().collect(),
                        cursor: cursor.clone(),
                        commit: n + 1 == count,
                    },
                )?;
            }
            Ok(())
        }
        Err(e) => helper::write_response(
            pipe,
            &Response::Overflow {
                roots: roots.to_vec(),
                cursor,
                reason: format!("Conservative USN reconciliation: {e}"),
            },
        ),
    }
}

// Historical batches can repeatedly mention the same deleted parent. Accumulate
// resolved directories in a bounded set, or request one reconciliation covering
// the final cursor. Sending an overflow per batch can repeatedly walk a drive.
#[derive(Default)]
struct Replay {
    paths: BTreeSet<Vec<u8>>,
    bytes: usize,
    overflow: Option<String>,
}

// Resolve parents after collecting the bounded launch delta: a child's record
// can precede its deleted parent's record by several native read batches.
#[derive(Default)]
struct ReplayEvents {
    events: Vec<JournalEntry>,
    bytes: usize,
    overflow: bool,
}
impl ReplayEvents {
    fn add(&mut self, events: Vec<JournalEntry>) {
        if self.overflow {
            return;
        }
        for event in events {
            let bytes = std::mem::size_of::<JournalEntry>() + event.name.len();
            if bytes > Replay::MAX_BYTES - self.bytes {
                self.events.clear();
                self.bytes = 0;
                self.overflow = true;
                return;
            }
            self.bytes += bytes;
            self.events.push(event);
        }
    }

    fn finish(self) -> io::Result<Vec<JournalEntry>> {
        if self.overflow {
            Err(io::Error::other("launch replay exceeded its event budget"))
        } else {
            Ok(self.events)
        }
    }
}
impl Replay {
    const MAX_PATHS: usize = 4096;
    const MAX_BYTES: usize = 8 * 1024 * 1024;

    fn add(&mut self, paths: io::Result<BTreeSet<Vec<u8>>>) {
        if self.overflow.is_some() {
            return;
        }
        match paths {
            Ok(paths) => {
                for path in paths {
                    if self.paths.contains(&path) {
                        continue;
                    }
                    if self.paths.len() == Self::MAX_PATHS
                        || path.len() > Self::MAX_BYTES - self.bytes
                    {
                        self.overflow = Some("launch replay exceeded its directory budget".into());
                        break;
                    }
                    self.bytes += path.len();
                    self.paths.insert(path);
                }
                if self.overflow.is_none() {
                    return;
                }
            }
            Err(e) => self.overflow = Some(e.to_string()),
        }
        self.paths.clear();
        self.bytes = 0;
    }

    fn finish(self) -> io::Result<BTreeSet<Vec<u8>>> {
        match self.overflow {
            Some(reason) => Err(io::Error::other(reason)),
            None => Ok(self.paths),
        }
    }
}

fn changes(
    pipe: &mut Pipe,
    source: &NtfsSource,
    roots: &[Vec<u8>],
    cursor: Cursor,
    events: &[seek_engine::ntfs::JournalEntry],
) -> io::Result<()> {
    write_changes(
        pipe,
        roots,
        cursor,
        source
            .changed_directories(events, roots)
            .map(|paths| paths.into_iter().collect()),
    )
}
pub fn session(mut pipe: Pipe, sid: &str) -> io::Result<()> {
    let request: Request = helper::read(&mut pipe)?;
    if let Request::Ping { version } = request {
        let _user = Impersonation::client(&pipe, sid)?;
        if version != helper::VERSION {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "protocol version mismatch",
            ));
        }
        return helper::write_response(&mut pipe, &Response::Pong { version });
    }
    let Request::Sync {
        version,
        roots,
        cursors,
        scan,
    } = request
    else {
        unreachable!()
    };
    if version != helper::VERSION || roots.is_empty() || roots.len() > 64 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "unsupported helper request",
        ));
    }
    seek_engine::walk::DENIED.lock().unwrap().clear();
    let mut volumes: BTreeMap<u8, Vec<Vec<u8>>> = BTreeMap::new();
    for root in &roots {
        if root.len() < 3
            || root[0] != b'/'
            || !root[1].is_ascii_alphabetic()
            || root[2] != b':'
            || root.len() > 100_000
            || root.contains(&0)
            || root.split(|b| *b == b'/').any(|p| p == b".." || p == b".")
            || seek_engine::paths::encode(&seek_engine::paths::decode(root)) != *root
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "invalid helper root",
            ));
        }
        volumes
            .entry(root[1].to_ascii_uppercase())
            .or_default()
            .push(root.clone());
    }
    // Raw handles are opened under the service identity. Every ordinary path
    // operation below (including directory access checks) uses the client token.
    let sources: Vec<_> = volumes
        .into_iter()
        .map(|(drive, roots)| (roots, NtfsSource::open(drive, pipe.cancel.clone())))
        .collect();
    let _user = Impersonation::client(&pipe, sid)?;
    let token = Arc::new(helper::duplicate_client_token()?);
    let mut control = Pipe {
        handle: pipe.handle.clone(),
        cancel: pipe.cancel.clone(),
    };
    let monitor_cancel = pipe.cancel.clone();
    let mut monitor = Monitor {
        cancel: pipe.cancel.clone(),
        thread: Some(thread::spawn(move || {
            let _ = helper::read::<Request>(&mut control);
            monitor_cancel.cancel();
        })),
    };
    let mut tails = vec![];
    let mut scan_profiles = vec![];
    for (roots, result) in sources {
        let source = match result {
            Ok(source) => source,
            Err(e) => {
                for root in roots {
                    helper::write_response(
                        &mut pipe,
                        &Response::Fallback {
                            root,
                            reason: format!("NTFS unavailable; walker used: {e}"),
                        },
                    )?;
                }
                continue;
            }
        };
        let (current, first) = match source.cursor() {
            Ok(v) => v,
            Err(e) => {
                for root in roots {
                    helper::write_response(
                        &mut pipe,
                        &Response::Fallback {
                            root,
                            reason: format!("USN unavailable; walker used: {e}"),
                        },
                    )?;
                }
                continue;
            }
        };
        let saved = cursors.iter().find(|c| c.drive == source.drive);
        let mut cursor = if !scan && saved.is_some_and(|c| c.valid_for(&current, first)) {
            saved.unwrap().clone()
        } else {
            current.clone()
        };
        if scan {
            for root in &roots {
                match source.scan(root) {
                    Ok(scan) => {
                        scan_profiles.push(scan.profile);
                        send_chunks(&mut pipe, root, scan.listings)?;
                    }
                    Err(e) => helper::write_response(
                        &mut pipe,
                        &Response::Fallback {
                            root: root.clone(),
                            reason: format!(
                                "MFT scan could not prove completeness; walker used: {e}"
                            ),
                        },
                    )?,
                }
            }
        } else if saved.is_none_or(|c| !c.valid_for(&current, first)) {
            for root in &roots {
                helper::write_response(
                    &mut pipe,
                    &Response::Reset {
                        root: root.clone(),
                        reason: "USN history unavailable/replaced; full folder reconciliation"
                            .into(),
                    },
                )?;
            }
        }
        let (target, _) = source.cursor()?;
        let mut replay = Replay::default();
        let mut history = ReplayEvents::default();
        let mut replayed = false;
        while cursor.next_usn < target.next_usn {
            let (next, events) = source.read_journal(&cursor, false)?;
            if next.next_usn <= cursor.next_usn {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "USN replay did not advance",
                ));
            }
            history.add(events);
            replayed = true;
            cursor = next;
        }
        if replayed {
            replay.add(history.finish().and_then(|events| {
                source
                    .changed_directories(&events, &roots)
                    .map(|paths| paths.into_iter().collect())
            }));
            write_changes(&mut pipe, &roots, cursor.clone(), replay.finish())?;
        }
        tails.push((roots, source, cursor));
    }
    let skipped: Vec<_> = seek_engine::walk::DENIED
        .lock()
        .unwrap()
        .iter()
        .map(|(p, &e)| (p.clone(), e))
        .collect();
    for chunk in skipped.chunks(256) {
        helper::write_response(
            &mut pipe,
            &Response::Skipped {
                paths: chunk.to_vec(),
            },
        )?;
    }
    helper::write_response(
        &mut pipe,
        &Response::Ready {
            cursors: tails.iter().map(|(_, _, c)| c.clone()).collect(),
            scan_profiles,
        },
    )?;
    let cancel = pipe.cancel.clone();
    let writer = Arc::new(Mutex::new(Pipe {
        handle: pipe.handle.clone(),
        cancel: cancel.clone(),
    }));
    let mut workers = vec![];
    for (roots, source, mut cursor) in tails {
        let writer = writer.clone();
        let token = token.clone();
        let cancel = cancel.clone();
        workers.push(thread::spawn(move || {
            seek_engine::qos::utility();
            let result = (|| -> io::Result<()> {
                let _user = Impersonation::token(&token)?;
                while !cancel.cancelled() {
                    match source.read_journal(&cursor, true) {
                        Ok((next, events)) => {
                            if next.next_usn == cursor.next_usn && events.is_empty() {
                                return Err(io::Error::other(
                                    "Unexpected empty blocking journal read",
                                ));
                            }
                            changes(
                                &mut writer.lock().unwrap(),
                                &source,
                                &roots,
                                next.clone(),
                                &events,
                            )?;
                            cursor = next;
                        }
                        Err(e) if !cancel.cancelled() => {
                            let (next, first) = source.cursor()?;
                            if cursor.valid_for(&next, first) {
                                return Err(e);
                            }
                            helper::write_response(
                                &mut *writer.lock().unwrap(),
                                &Response::Overflow {
                                    roots: roots.clone(),
                                    cursor: next.clone(),
                                    reason: format!("USN read failed; full reconciliation: {e}"),
                                },
                            )?;
                            cursor = next;
                        }
                        Err(e) => return Err(e),
                    }
                }
                Ok(())
            })();
            if let Err(e) = result
                && !cancel.cancelled()
            {
                let _ = helper::write_response(
                    &mut *writer.lock().unwrap(),
                    &Response::Error {
                        message: e.to_string(),
                    },
                );
                cancel.cancel();
            }
        }));
    }
    // A pending read detects client death without a heartbeat or polling.
    // The protocol has one initial request; any additional message closes it.
    if let Some(thread) = monitor.thread.take() {
        let _ = thread.join();
    }
    cancel.cancel();
    for worker in workers {
        let _ = worker.join();
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn launch_history_keeps_cross_batch_parent_records_and_bounds_memory() {
        let event = |id, name: Vec<u8>| JournalEntry {
            id,
            parent: 5,
            usn: id as i64,
            reason: 0,
            attributes: 16,
            name,
        };
        let mut history = ReplayEvents::default();
        history.add(vec![event(20, b"child".to_vec())]);
        history.add(vec![event(21, b"deleted-parent".to_vec())]);
        let events = history.finish().unwrap();
        assert_eq!(events.iter().map(|e| e.id).collect::<Vec<_>>(), [20, 21]);
        let mut history = ReplayEvents::default();
        history.add(vec![event(20, vec![b'x'; Replay::MAX_BYTES + 1])]);
        assert!(history.events.is_empty());
        history.add(vec![event(21, b"later".to_vec())]);
        assert!(
            history
                .finish()
                .unwrap_err()
                .to_string()
                .contains("event budget")
        );
    }

    fn cursor() -> Cursor {
        Cursor {
            drive: b'C',
            serial: 7,
            journal_id: 11,
            next_usn: 999,
        }
    }

    #[test]
    fn replay_deduplicates_batches_and_commits_only_after_every_path() {
        let roots = vec![b"/C:/users/test".to_vec()];
        let paths: BTreeSet<_> = (0..300)
            .map(|n| format!("/C:/users/test/folder{n:03}").into_bytes())
            .collect();
        let mut replay = Replay::default();
        replay.add(Ok(paths.clone()));
        replay.add(Ok(paths.clone()));
        replay.add(Ok(BTreeSet::from([b"/C:/users/testing/outside".to_vec()])));
        let mut wire = vec![];
        write_changes(&mut wire, &roots, cursor(), replay.finish()).unwrap();
        let mut input = wire.as_slice();
        let mut received = BTreeSet::new();
        for n in 0..2 {
            let Response::Changes {
                paths,
                cursor: checkpoint,
                commit,
            } = helper::read_response(&mut input).unwrap()
            else {
                panic!("expected a change chunk")
            };
            assert_eq!(checkpoint, cursor());
            assert_eq!(commit, n == 1);
            assert_eq!(paths.len(), if n == 0 { 256 } else { 44 });
            received.extend(paths);
        }
        assert_eq!(received, paths);
        assert!(input.is_empty());
    }

    #[test]
    fn replay_budget_requests_one_reconciliation_without_dropping_changes() {
        let roots = vec![b"/C:".to_vec()];
        for byte_limit in [false, true] {
            let mut replay = Replay::default();
            let paths = if byte_limit {
                BTreeSet::from([vec![b'x'; Replay::MAX_BYTES + 1]])
            } else {
                (0..=Replay::MAX_PATHS)
                    .map(|n| n.to_string().into_bytes())
                    .collect()
            };
            replay.add(Ok(paths));
            assert!(replay.paths.is_empty());
            assert_eq!(replay.bytes, 0);
            replay.add(Ok(BTreeSet::from([b"/C:/later".to_vec()])));
            let mut wire = vec![];
            write_changes(&mut wire, &roots, cursor(), replay.finish()).unwrap();
            let mut input = wire.as_slice();
            let Response::Overflow {
                roots: actual,
                cursor: checkpoint,
                reason,
            } = helper::read_response(&mut input).unwrap()
            else {
                panic!("expected explicit reconciliation")
            };
            assert_eq!(actual, roots);
            assert_eq!(checkpoint, cursor());
            assert!(reason.contains("budget"));
            assert!(input.is_empty());
        }
    }

    #[test]
    fn unresolved_historical_parent_keeps_the_final_reconciliation() {
        let mut replay = Replay::default();
        replay.add(Ok(BTreeSet::from([b"/C:/first".to_vec()])));
        replay.add(Err(io::Error::new(
            io::ErrorKind::NotFound,
            "parent was deleted",
        )));
        replay.add(Ok(BTreeSet::from([b"/C:/last".to_vec()])));
        assert!(replay.paths.is_empty());
        assert!(
            replay
                .finish()
                .unwrap_err()
                .to_string()
                .contains("parent was deleted")
        );
    }
}
