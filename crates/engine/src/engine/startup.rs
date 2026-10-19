use super::*;

#[derive(Clone, Debug, Default, serde::Serialize)]
pub struct Progress {
    pub phase: String,
    pub completed: usize,
    pub total: Option<usize>,
    pub priority_ready: bool,
    pub priority_ms: f64,
    pub full_relist_done: bool,
    pub read_ms: f64,
    pub apply_ms: f64,
    pub load_ms: f64,
    pub plan_ms: f64,
    pub connect_ms: f64,
    pub save_ms: f64,
}

fn contains(root: &[u8], path: &[u8]) -> bool {
    path.len() >= root.len()
        && crate::paths::cmp_ci(&path[..root.len()], root).is_eq()
        && (path.len() == root.len() || path.get(root.len()) == Some(&b'/'))
}

/// Saved errors describe a previous observation, not permanent exclusions.
/// Retry after priority browsing is ready, using the existing index worker.
pub(super) fn retry_failures(shared: &Shared) {
    let paths = walk::DENIED
        .lock()
        .unwrap()
        .keys()
        .filter(|p| shared.roots.iter().any(|r| contains(r, p)))
        .cloned()
        .collect::<Vec<_>>();
    for path in paths {
        if shared.closing.load(std::sync::atomic::Ordering::Acquire) {
            return;
        }
        // A failed directory may have been replaced by a redirecting link.
        // Preserve the indexer's rule against following name-surrogate links.
        if let Some(entry) = crate::live::lstat(&path)
            && entry.kind & 3 == walk::KIND_LINK
        {
            walk::DENIED.lock().unwrap().remove(&path);
            continue;
        }
        if walk::list_one(&path).is_some() {
            // A formerly denied folder may have descendants absent from the
            // cached index. Recover its whole subtree, not just its counter.
            relist(shared, &path, true);
        }
    }
    shared.update_status();
    shared.announce();
}

#[cfg(test)]
mod retry_tests {
    use super::*;
    #[test]
    fn recovered_cached_folder_reindexes_descendants_before_clearing_its_warning() {
        let temp = tempfile::tempdir().unwrap();
        let root = crate::paths::internal(temp.path());
        let folder = temp.path().join("recovered");
        std::fs::create_dir(&folder).unwrap();
        std::fs::write(folder.join("visible.txt"), b"fixture").unwrap();
        let mut listings = walk::scan(&root, 1);
        // Model a cached listing whose directory previously denied access.
        for listing in &mut listings {
            if listing.id != 0 {
                listing.names.clear();
                listing.ents.clear();
            }
        }
        let shared = Arc::new(Shared {
            closing: std::sync::atomic::AtomicBool::new(false),
            roots: vec![root],
            live: RwLock::new(None),
            status: Mutex::new(Status::default()),
            ready: Condvar::new(),
            notify: Mutex::new(None),
            opened: RwLock::new(HashMap::new()),
            journal: Mutex::new(None),
            helper_cancel: Mutex::new(None),
        });
        shared.install(Index::build(listings, 0, crate::query::now_secs(), &[]));
        let (tx, _rx) = mpsc::channel();
        let engine = Engine {
            shared: shared.clone(),
            control: Arc::new(Control {
                tx,
                thread: Mutex::new(None),
                shared: shared.clone(),
            }),
        };
        let failed = crate::paths::internal(&folder);
        walk::DENIED.lock().unwrap().insert(failed.clone(), 5);
        assert!(engine.children(&folder, true, &[]).is_empty());
        retry_failures(&shared);
        assert_eq!(engine.children(&folder, true, &[]).len(), 1);
        assert!(!walk::DENIED.lock().unwrap().contains_key(&failed));
    }
}

pub(super) fn refresh(shared: &Shared, full: bool, started: Instant) {
    let planning = Instant::now();
    let priorities: Vec<_> = crate::places::favorites()
        .into_iter()
        .map(|(_, p)| crate::paths::internal(&p))
        .filter(|p| shared.roots.iter().any(|r| contains(r, p)))
        .collect();
    let mut dirs = {
        let guard = shared.live.read().unwrap();
        let live = guard.as_ref().unwrap();
        let mut dirs = Vec::with_capacity(if full { live.base.d } else { 256 });
        let mut path = Vec::new();
        let entries = if full {
            live.base
                .dir_entry()
                .iter()
                .skip(1)
                .copied()
                .collect::<Vec<_>>()
        } else {
            // The mapped index already stores a folder's subtree as a block.
            // Resolve six roots instead of materializing every drive path.
            let mut entries = std::collections::BTreeSet::new();
            for p in &priorities {
                if let Some(d) = live.base.lookup(p).and_then(|e| live.base.dir_of(e)) {
                    entries.insert(live.base.dir_entry()[d as usize]);
                    for d in live.base.descendants(d) {
                        entries.insert(live.base.dir_entry()[d as usize]);
                    }
                }
            }
            entries.into_iter().collect()
        };
        for entry in entries {
            if live.base.kind()[entry as usize] & FLAG_SYNTH != 0 {
                continue;
            }
            live.base.path(entry as usize, &mut path);
            let rank = priorities
                .iter()
                .position(|p| contains(p, &path))
                .unwrap_or(priorities.len());
            if full || rank < priorities.len() {
                dirs.push((
                    rank,
                    std::cmp::Reverse(live.attributes(entry as usize).2),
                    path.clone(),
                ));
            }
        }
        dirs
    };
    for (rank, p) in priorities.iter().enumerate() {
        if !dirs
            .iter()
            .any(|(_, _, d)| crate::paths::cmp_ci(d, p).is_eq())
        {
            dirs.push((rank, std::cmp::Reverse(u32::MAX), p.clone()));
        }
    }
    // Every cached directory remains in the plan. Ordering never filters data.
    dirs.sort_unstable_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)).then(a.2.cmp(&b.2)));
    let priority_count = dirs.iter().take_while(|d| d.0 < priorities.len()).count();
    {
        let mut s = shared.status.lock().unwrap();
        s.progress.phase = "Checking priority folders".into();
        s.progress.plan_ms = planning.elapsed().as_secs_f64() * 1000.;
        s.progress.completed = 0;
        s.progress.total = Some(dirs.len());
    }
    let mut last = Instant::now();
    for (i, (_, _, path)) in dirs.iter().enumerate() {
        if shared.closing.load(std::sync::atomic::Ordering::Acquire) {
            return;
        }
        relist(shared, path, false);
        let publish = last.elapsed() >= Duration::from_millis(100)
            || i + 1 == priority_count
            || i + 1 == dirs.len();
        {
            let mut s = shared.status.lock().unwrap();
            s.progress.completed = i + 1;
            if i + 1 >= priority_count && !s.progress.priority_ready {
                s.progress.priority_ready = true;
                s.progress.priority_ms = started.elapsed().as_secs_f64() * 1000.;
                s.progress.phase = if full {
                    "Checking remaining folders"
                } else {
                    "Replaying journal"
                }
                .into();
            }
        }
        if publish {
            shared.update_status();
            shared.announce();
            last = Instant::now();
        }
    }
    let mut s = shared.status.lock().unwrap();
    s.progress.full_relist_done = full;
    if !full {
        s.progress.total = None;
        s.progress.phase = "Replaying journal".into();
    }
    if priority_count == 0 {
        s.progress.priority_ready = true;
        s.progress.priority_ms = started.elapsed().as_secs_f64() * 1000.;
    }
}
