use super::*;
use crate::{
    helper::{self, Request, Response},
    kernel::Cancel,
    ntfs::Cursor,
};
#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub(super) struct SavedState {
    pub roots: Vec<Vec<u8>>,
    pub cursors: Vec<Cursor>,
    pub ntfs_baseline: bool,
    #[serde(default)]
    pub skipped: Vec<(Vec<u8>, u32)>,
}
pub(super) struct Session {
    rx: Option<mpsc::Receiver<Vec<Response>>>,
    pending: std::collections::VecDeque<Response>,
    cancel: Cancel,
    thread: Option<JoinHandle<()>>,
}
impl Drop for Session {
    fn drop(&mut self) {
        self.cancel.cancel();
        self.rx.take();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}
impl Session {
    pub fn start(
        shared: &Shared,
        roots: &[Vec<u8>],
        source: SourceMode,
        tx: mpsc::Sender<Change>,
    ) -> Result<Option<Self>, String> {
        if matches!(source, SourceMode::Walker) {
            return Ok(None);
        }
        let cancel = Cancel::new().map_err(|e| e.to_string())?;
        let mut pipe = match helper::installed_connection(cancel.clone()) {
            Ok(Some(pipe)) => pipe,
            Ok(None) if matches!(source, SourceMode::Helper) => {
                return Err("Seek helper is not installed/running; opt in from settings".into());
            }
            Ok(None) => return Ok(None),
            Err(e) if matches!(source, SourceMode::Helper) => return Err(e.to_string()),
            Err(e) => {
                shared.status.lock().unwrap().helper_note =
                    Some(format!("Helper unavailable; using walker: {e}"));
                return Ok(None);
            }
        };
        *shared.helper_cancel.lock().unwrap() = Some(cancel.clone());
        let state = shared.journal.lock().unwrap().clone();
        let request = Request::Sync {
            version: helper::VERSION,
            roots: roots.to_vec(),
            cursors: state.as_ref().map_or_else(Vec::new, |s| s.cursors.clone()),
            scan: state.is_none(),
        };
        helper::write(&mut pipe, &request).map_err(|e| e.to_string())?;
        let (out, rx) = mpsc::sync_channel(4);
        let thread = thread::Builder::new()
            .name("seek-helper-pipe".into())
            .spawn(move || {
                crate::qos::utility();
                let mut pipe = std::io::BufReader::with_capacity(1024 * 1024, pipe);
                loop {
                    let batch = helper::read_response_batch(&mut pipe);
                    let error = batch.iter().any(|r| matches!(r, Response::Error { .. }));
                    if out.send(batch).is_err() {
                        break;
                    }
                    let _ = tx.send(Change::HelperAvailable);
                    if error {
                        break;
                    }
                }
            })
            .map_err(|e| e.to_string())?;
        Ok(Some(Self {
            rx: Some(rx),
            pending: Default::default(),
            cancel,
            thread: Some(thread),
        }))
    }
    fn recv(&mut self) -> Result<Response, String> {
        if self.pending.is_empty() {
            self.pending.extend(
                self.rx
                    .as_ref()
                    .unwrap()
                    .recv()
                    .map_err(|e| e.to_string())?,
            );
        }
        self.pending
            .pop_front()
            .ok_or("Empty helper response batch".into())
    }
    pub fn try_recv(&mut self) -> Option<Response> {
        if self.pending.is_empty() {
            self.pending
                .extend(self.rx.as_ref().unwrap().try_recv().ok()?);
        }
        self.pending.pop_front()
    }
}
fn scoped(path: &[u8], roots: &[Vec<u8>]) -> bool {
    roots.iter().any(|root| {
        path.len() >= root.len()
            && crate::paths::cmp_ci(&path[..root.len()], root).is_eq()
            && (path.len() == root.len() || path.get(root.len()) == Some(&b'/'))
    })
}
fn cursor(shared: &Shared, cursor: Cursor) {
    if let Some(state) = shared.journal.lock().unwrap().as_mut() {
        if let Some(old) = state.cursors.iter_mut().find(|c| c.drive == cursor.drive) {
            *old = cursor;
        } else {
            state.cursors.push(cursor);
        }
    }
}
pub(super) fn apply(shared: &Shared, roots: &[Vec<u8>], response: Response) -> Result<(), String> {
    match response {
        Response::Changes {
            paths,
            cursor: next,
            commit,
        } => {
            {
                let mut status = shared.status.lock().unwrap();
                status.journal_directories += paths.len();
                status.journal_batches += usize::from(commit);
            }
            for path in paths {
                if scoped(&path, roots) {
                    relist(shared, &path, false);
                }
            }
            if commit {
                cursor(shared, next);
            }
        }
        Response::Overflow {
            roots: changed,
            cursor: next,
            reason,
        } => {
            {
                let mut s = shared.status.lock().unwrap();
                s.catching_up = true;
                s.helper_note = Some(reason);
                s.journal_reconciliations += 1;
            }
            shared.announce();
            for root in changed {
                if roots.contains(&root) {
                    relist(shared, &root, true);
                }
            }
            cursor(shared, next);
        }
        Response::Skipped { paths } => {
            for (path, error) in paths {
                walk::record_failure(path, error);
            }
        }
        Response::Ready { cursors, .. } => {
            for c in cursors {
                cursor(shared, c);
            }
        }
        Response::Error { message } => return Err(message),
        _ => return Err("Unexpected helper response after synchronization".into()),
    }
    Ok(())
}
pub(super) fn initial(
    shared: &Shared,
    roots: &[Vec<u8>],
    session: &mut Session,
    controls: Option<(&mpsc::Receiver<Change>, &Path)>,
) -> Result<(), String> {
    let mut scans: HashMap<Vec<u8>, BTreeMap<u32, Listing>> = HashMap::new();
    let mut completed: HashMap<Vec<u8>, std::collections::HashSet<u32>> = HashMap::new();
    let mut deferred = Vec::new();
    let mut reconcile_all = false;
    let mut native_changes = std::collections::BTreeSet::new();
    let mut ntfs = shared
        .journal
        .lock()
        .unwrap()
        .as_ref()
        .is_none_or(|s| s.ntfs_baseline);
    loop {
        // Cached results stay live while an optional first MFT snapshot is
        // streamed. Consume both sources on this worker, without timers.
        let response = if !session.pending.is_empty() {
            session.recv()?
        } else if let Some((rx, dir)) = controls {
            match rx.recv().map_err(|e| e.to_string())? {
                Change::HelperAvailable => session.recv()?,
                Change::Stop => return Err("Synchronization cancelled".into()),
                Change::Reconciled(_) => continue,
                Change::Checkpoint(reply) => {
                    let result = if shared.live.read().unwrap().is_some() {
                        save(shared, dir)
                    } else {
                        Err("Index is still building".into())
                    };
                    let _ = reply.send(result);
                    continue;
                }
                Change::Opened(path) => {
                    record_open(shared, dir, path)?;
                    continue;
                }
                Change::Directories(paths) => {
                    for path in paths {
                        if shared.live.read().unwrap().is_some() {
                            relist(shared, &path, false);
                        }
                        if native_changes.len() < 4096 {
                            native_changes.insert(path);
                        } else {
                            reconcile_all = true;
                        }
                    }
                    shared.update_status();
                    shared.announce();
                    continue;
                }
                Change::Overflow(root) => {
                    if shared.live.read().unwrap().is_some() {
                        relist(shared, &root, true);
                    }
                    reconcile_all = true;
                    shared.update_status();
                    shared.announce();
                    continue;
                }
                Change::Error(root, error) => {
                    walk::DENIED.lock().unwrap().insert(root, error);
                    shared.status.lock().unwrap().error =
                        Some(format!("Filesystem watcher failed: Windows error {error}"));
                    continue;
                }
            }
        } else {
            session.recv()?
        };
        match response {
            Response::Chunk {
                root,
                listing,
                last,
            } => {
                if !roots.contains(&root) || listing.id > 1_000_000 {
                    return Err("Helper returned an unexpected root/directory ID".into());
                }
                if completed
                    .get(&root)
                    .is_some_and(|set| set.contains(&listing.id))
                {
                    return Err("Helper repeated a completed directory".into());
                }
                if last {
                    completed
                        .entry(root.clone())
                        .or_default()
                        .insert(listing.id);
                }
                let target = scans
                    .entry(root)
                    .or_default()
                    .entry(listing.id)
                    .or_insert_with(|| Listing {
                        id: listing.id,
                        names: vec![],
                        ents: vec![],
                    });
                let base = u32::try_from(target.names.len()).map_err(|e| e.to_string())?;
                for mut ent in listing.ents {
                    if ent.name_off as usize + ent.name_len as usize > listing.names.len() {
                        return Err("Helper name bounds".into());
                    }
                    ent.name_off = ent
                        .name_off
                        .checked_add(base)
                        .ok_or("Helper name offset overflow")?;
                    target.ents.push(ent);
                }
                target.names.extend(listing.names);
                if last {
                    let count = completed.values().map(|s| s.len()).sum::<usize>();
                    {
                        let mut s = shared.status.lock().unwrap();
                        s.progress.phase = "Building index".into();
                        s.progress.completed = count;
                        s.progress.total = None;
                    }
                    if count.is_multiple_of(128) {
                        shared.announce();
                    }
                }
            }
            Response::Fallback { root, reason } => {
                if !roots.contains(&root) {
                    return Err("Helper fallback root mismatch".into());
                }
                ntfs = false;
                {
                    let mut status = shared.status.lock().unwrap();
                    status.helper_note = Some(reason);
                    status.ntfs_fallbacks += 1;
                }
                let listings = walk::scan(&root, 4);
                completed.insert(root.clone(), listings.iter().map(|l| l.id).collect());
                scans.insert(
                    root.clone(),
                    listings.into_iter().map(|l| (l.id, l)).collect(),
                );
            }
            Response::Reset { root, reason } => {
                if !roots.contains(&root) {
                    return Err("Helper reset root mismatch".into());
                }
                {
                    let mut status = shared.status.lock().unwrap();
                    status.helper_note = Some(reason);
                    status.journal_resets += 1;
                }
                if shared.live.read().unwrap().is_some() {
                    relist(shared, &root, true);
                } else {
                    return Err("Helper reset without a baseline".into());
                }
            }
            Response::Skipped { paths } => {
                for (path, error) in paths {
                    walk::record_failure(path, error);
                }
            }
            Response::Ready {
                cursors,
                scan_profiles,
            } => {
                shared.status.lock().unwrap().scan_profiles = scan_profiles;
                if !scans.is_empty() {
                    if roots.iter().any(|root| !scans.contains_key(root)) {
                        return Err("Incomplete helper snapshot".into());
                    }
                    for (root, listings) in &scans {
                        let done = completed
                            .get(root)
                            .ok_or("Helper has incomplete directory chunks")?;
                        if done.len() != listings.len()
                            || listings.keys().copied().ne(0..listings.len() as u32)
                            || listings.values().any(|l| {
                                l.ents
                                    .iter()
                                    .any(|e| e.child != NONE && !listings.contains_key(&e.child))
                            })
                        {
                            return Err("Invalid/incomplete helper directory tree".into());
                        }
                    }
                    let listings = graft_roots(roots, |root| {
                        scans.remove(root).unwrap().into_values().collect()
                    });
                    shared.install(Index::build(listings, 0, crate::query::now_secs(), &[]));
                }
                if shared.live.read().unwrap().is_none() {
                    return Err("Helper sent no initial index".into());
                }
                *shared.journal.lock().unwrap() = Some(SavedState {
                    roots: roots.to_vec(),
                    cursors: Vec::new(),
                    ntfs_baseline: ntfs,
                    skipped: Vec::new(),
                });
                for response in deferred {
                    apply(shared, roots, response)?;
                }
                if reconcile_all {
                    for root in roots {
                        relist(shared, root, true);
                    }
                } else {
                    for path in native_changes {
                        relist(shared, &path, false);
                    }
                }
                let usn = !cursors.is_empty();
                shared.journal.lock().unwrap().as_mut().unwrap().cursors = cursors;
                shared.status.lock().unwrap().source = if !usn {
                    "walker"
                } else if ntfs {
                    "ntfs + usn"
                } else {
                    "walker + usn"
                }
                .into();
                shared.update_status();
                shared.announce();
                return Ok(());
            }
            Response::Error { message } => return Err(message),
            response @ (Response::Changes { .. } | Response::Overflow { .. }) => {
                if shared.live.read().unwrap().is_some() && scans.is_empty() {
                    apply(shared, roots, response)?;
                } else {
                    if deferred.len() == 64 {
                        deferred.clear();
                        reconcile_all = true;
                    }
                    if !reconcile_all {
                        deferred.push(response);
                    }
                }
            }
            _ => return Err("Unexpected helper handshake response".into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn session(messages: Vec<Response>) -> Session {
        let (tx, rx) = mpsc::sync_channel(4);
        let thread = thread::spawn(move || {
            for message in messages {
                if tx.send(vec![message]).is_err() {
                    break;
                }
            }
        });
        Session {
            rx: Some(rx),
            pending: Default::default(),
            cancel: Cancel::new().unwrap(),
            thread: Some(thread),
        }
    }
    fn test_engine() -> Engine {
        let shared = Arc::new(Shared {
            closing: std::sync::atomic::AtomicBool::new(false),
            roots: Vec::new(),
            live: RwLock::new(None),
            status: Mutex::new(Status::default()),
            ready: Condvar::new(),
            notify: Mutex::new(None),
            opened: RwLock::new(HashMap::new()),
            journal: Mutex::new(None),
            helper_cancel: Mutex::new(None),
        });
        let (tx, _rx) = mpsc::channel();
        Engine {
            shared: shared.clone(),
            control: Arc::new(Control {
                tx,
                thread: Mutex::new(None),
                shared,
            }),
        }
    }
    fn cursor(usn: i64) -> Cursor {
        Cursor {
            drive: b'C',
            serial: 123,
            journal_id: 456,
            next_usn: usn,
        }
    }
    fn saved_usn(dir: &Path) -> i64 {
        let index = Index::load(&dir.join("index.bin")).unwrap();
        let state: SavedState = serde_json::from_slice(index.saved_state().unwrap()).unwrap();
        assert_eq!(index.event_id, state.cursors[0].next_usn as u64);
        state.cursors[0].next_usn
    }
    #[test]
    fn helper_checkpoint_retains_scoped_denials_and_reads_previous_footer() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("files");
        let data = temp.path().join("index");
        std::fs::create_dir(&root).unwrap();
        std::fs::create_dir(&data).unwrap();
        let roots = vec![crate::paths::internal(&root)];
        let engine = test_engine();
        let mut messages: Vec<_> = walk::scan(&roots[0], 1)
            .into_iter()
            .map(|listing| Response::Chunk {
                root: roots[0].clone(),
                listing,
                last: true,
            })
            .collect();
        messages.push(Response::Ready {
            cursors: vec![cursor(77)],
            scan_profiles: vec![],
        });
        initial(&engine.shared, &roots, &mut session(messages), None).unwrap();
        let denied = crate::paths::internal(&root.join("denied"));
        walk::DENIED.lock().unwrap().insert(denied.clone(), 5);
        save(&engine.shared, &data).unwrap();
        walk::DENIED.lock().unwrap().remove(&denied);
        let index = Index::load(&data.join("index.bin")).unwrap();
        let state: SavedState = serde_json::from_slice(index.saved_state().unwrap()).unwrap();
        assert_eq!(state.skipped, vec![(denied, 5)]);
        let old = serde_json::json!({"roots":roots,"cursors":[cursor(77)],"ntfs_baseline":true});
        assert!(
            serde_json::from_value::<SavedState>(old)
                .unwrap()
                .skipped
                .is_empty()
        );
    }
    #[test]
    fn streamed_baseline_live_replay_reset_and_atomic_cursor_checkpoints() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("files");
        let data = temp.path().join("index");
        std::fs::create_dir_all(root.join("deep")).unwrap();
        std::fs::create_dir(&data).unwrap();
        std::fs::write(root.join("alpha.txt"), b"alpha").unwrap();
        std::fs::write(root.join("deep/beta.txt"), b"beta").unwrap();
        let roots = vec![crate::paths::internal(&root)];
        let engine = test_engine();
        let mut messages: Vec<_> = walk::scan(&roots[0], 2)
            .into_iter()
            .map(|listing| Response::Chunk {
                root: roots[0].clone(),
                listing,
                last: true,
            })
            .collect();
        messages.push(Response::Ready {
            cursors: vec![cursor(77)],
            scan_profiles: vec![],
        });
        initial(&engine.shared, &roots, &mut session(messages), None).unwrap();
        assert_eq!(
            engine
                .search(&Query::parse("alpha", "").unwrap())
                .unwrap()
                .len(),
            1
        );
        save(&engine.shared, &data).unwrap();
        assert_eq!(saved_usn(&data), 77);
        // Changes happened with no watcher/engine worker present. Only the
        // delivered journal directory delta reconciles this saved baseline.
        std::fs::rename(root.join("alpha.txt"), root.join("renamed-🚀.txt")).unwrap();
        std::fs::remove_file(root.join("deep/beta.txt")).unwrap();
        std::fs::create_dir(root.join("new-folder")).unwrap();
        std::fs::write(root.join("new-folder/created.txt"), b"created").unwrap();
        apply(
            &engine.shared,
            &roots,
            Response::Changes {
                paths: vec![roots[0].clone(), crate::paths::internal(&root.join("deep"))],
                cursor: cursor(90),
                commit: false,
            },
        )
        .unwrap();
        assert!(
            engine
                .search(&Query::parse("alpha", "").unwrap())
                .unwrap()
                .is_empty()
        );
        assert!(
            engine
                .search(&Query::parse("beta", "").unwrap())
                .unwrap()
                .is_empty()
        );
        assert_eq!(
            engine
                .search(&Query::parse("created", "").unwrap())
                .unwrap()
                .len(),
            1
        );
        let expected: std::collections::HashSet<_> = [
            root.clone(),
            root.join("deep"),
            root.join("renamed-🚀.txt"),
            root.join("new-folder"),
            root.join("new-folder/created.txt"),
        ]
        .into_iter()
        .collect();
        assert_eq!(
            engine
                .snapshot()
                .into_iter()
                .map(|h| h.path)
                .collect::<std::collections::HashSet<_>>(),
            expected
        );
        // A checkpoint between chunks must retain the OLD USN, so a crash
        // replays the entire batch rather than losing its remaining changes.
        save(&engine.shared, &data).unwrap();
        assert_eq!(saved_usn(&data), 77);
        apply(
            &engine.shared,
            &roots,
            Response::Changes {
                paths: vec![],
                cursor: cursor(90),
                commit: true,
            },
        )
        .unwrap();
        save(&engine.shared, &data).unwrap();
        assert_eq!(saved_usn(&data), 90);
        assert_eq!(engine.status().journal_batches, 1);
        assert_eq!(engine.status().journal_directories, 2);
        std::fs::write(root.join("after-wrap.txt"), b"new").unwrap();
        initial(
            &engine.shared,
            &roots,
            &mut session(vec![
                Response::Reset {
                    root: roots[0].clone(),
                    reason: "history lost".into(),
                },
                Response::Ready {
                    cursors: vec![cursor(100)],
                    scan_profiles: vec![],
                },
            ]),
            None,
        )
        .unwrap();
        assert_eq!(engine.status().journal_resets, 1);
        assert_eq!(
            engine
                .search(&Query::parse("after-wrap", "").unwrap())
                .unwrap()
                .len(),
            1
        );
        save(&engine.shared, &data).unwrap();
        assert_eq!(saved_usn(&data), 100);
        // Saving without state strips the trailer; byte offsets stay portable.
        let index = Index::load(&data.join("index.bin")).unwrap();
        index.save(&data.join("plain.bin")).unwrap();
        assert!(
            Index::load(&data.join("plain.bin"))
                .unwrap()
                .saved_state()
                .is_none()
        );
    }
    #[test]
    fn cached_results_follow_native_changes_during_streamed_baseline() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("files");
        let data = temp.path().join("index");
        std::fs::create_dir(&root).unwrap();
        std::fs::create_dir(&data).unwrap();
        std::fs::write(root.join("before.txt"), b"before").unwrap();
        let roots = vec![crate::paths::internal(&root)];
        let baseline = walk::scan(&roots[0], 2);
        let engine = test_engine();
        *engine.shared.live.write().unwrap() = Some(Live::new(Index::build(
            graft_roots(&roots, |_| baseline.clone()),
            0,
            crate::query::now_secs(),
            &[],
        )));
        let shared = engine.shared.clone();
        let roots_worker = roots.clone();
        let (frames, frame_rx) = mpsc::sync_channel(4);
        let (controls, control_rx) = mpsc::channel();
        let worker = thread::spawn(move || {
            let mut session = Session {
                rx: Some(frame_rx),
                pending: Default::default(),
                cancel: Cancel::new().unwrap(),
                thread: None,
            };
            initial(
                &shared,
                &roots_worker,
                &mut session,
                Some((&control_rx, &data)),
            )
        });
        std::fs::write(root.join("during-scan.txt"), b"new").unwrap();
        controls.send(Change::Directories(roots.clone())).unwrap();
        let (ack, ack_rx) = mpsc::channel();
        controls.send(Change::Checkpoint(ack)).unwrap();
        ack_rx
            .recv_timeout(Duration::from_secs(5))
            .unwrap()
            .unwrap();
        assert_eq!(
            engine
                .search(&Query::parse("during-scan", "").unwrap())
                .unwrap()
                .len(),
            1
        );
        // The snapshot started before this file was created. The native delta
        // must be applied again when it replaces the already updated cache.
        let mut batch: Vec<_> = baseline
            .into_iter()
            .map(|listing| Response::Chunk {
                root: roots[0].clone(),
                listing,
                last: true,
            })
            .collect();
        batch.push(Response::Ready {
            cursors: vec![cursor(90)],
            scan_profiles: vec![],
        });
        frames.send(batch).unwrap();
        // One notification for the entire baseline including Ready must drain
        // the buffered messages without waiting for a second wake-up.
        controls.send(Change::HelperAvailable).unwrap();
        worker.join().unwrap().unwrap();
        assert_eq!(
            engine
                .search(&Query::parse("during-scan", "").unwrap())
                .unwrap()
                .len(),
            1
        );
    }
    #[test]
    fn incomplete_snapshot_never_publishes_a_ready_index() {
        let engine = test_engine();
        let roots = vec![b"/C:/test".to_vec()];
        let messages = vec![
            Response::Chunk {
                root: roots[0].clone(),
                listing: Listing {
                    id: 0,
                    names: vec![],
                    ents: vec![],
                },
                last: false,
            },
            Response::Ready {
                cursors: vec![cursor(77)],
                scan_profiles: vec![],
            },
        ];
        assert!(initial(&engine.shared, &roots, &mut session(messages), None).is_err());
        assert!(engine.shared.live.read().unwrap().is_none());
    }
}
