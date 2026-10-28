//! In-process Windows driver. Searcher and live overlay are vendored portable code.
use crate::{
    index::Index,
    live::Live,
    query::{Query, Searcher},
    walk::{self, FLAG_SYNTH, KIND_DIR, Listing, NONE, RawEnt},
    watch::{Change, Watch},
};
use std::{
    collections::{BTreeMap, HashMap},
    path::{Path, PathBuf},
    sync::{Arc, Condvar, Mutex, RwLock, mpsc},
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};
mod changes;
mod helper_sync;
mod startup;
#[derive(Clone, Copy, Debug, Default)]
pub enum SourceMode {
    #[default]
    Auto,
    Walker,
    Helper,
}

#[derive(Clone)]
pub struct Options {
    pub dir: PathBuf,
    pub roots: Vec<PathBuf>,
}
#[derive(Clone, Debug, serde::Serialize)]
pub struct Found {
    pub path: PathBuf,
    pub kind: u8,
    pub size: u64,
    pub mtime: u32,
    pub score: i32,
}
#[derive(Clone, Debug, Default, serde::Serialize)]
pub struct Status {
    pub ready: bool,
    pub catching_up: bool,
    pub entries: usize,
    pub dirs: usize,
    pub index_bytes: usize,
    pub skipped: usize,
    pub catch_up_ms: f64,
    pub error: Option<String>,
    pub source: String,
    pub helper_note: Option<String>,
    /// Per-session evidence: distinguish replay from a hidden full rescan.
    pub journal_batches: usize,
    pub journal_directories: usize,
    pub journal_resets: usize,
    pub journal_reconciliations: usize,
    pub ntfs_fallbacks: usize,
    pub scan_profiles: Vec<crate::ntfs::ScanProfile>,
    pub progress: startup::Progress,
}
type Notify = Arc<dyn Fn() + Send + Sync>;
struct Shared {
    closing: std::sync::atomic::AtomicBool,
    roots: Vec<Vec<u8>>,
    live: RwLock<Option<Live>>,
    status: Mutex<Status>,
    ready: Condvar,
    notify: Mutex<Option<Notify>>,
    opened: RwLock<HashMap<Vec<u8>, u32>>,
    journal: Mutex<Option<helper_sync::SavedState>>,
    helper_cancel: Mutex<Option<crate::kernel::Cancel>>,
}
struct Control {
    tx: mpsc::Sender<Change>,
    thread: Mutex<Option<JoinHandle<()>>>,
    shared: Arc<Shared>,
}
impl Drop for Control {
    fn drop(&mut self) {
        self.shared
            .closing
            .store(true, std::sync::atomic::Ordering::Release);
        if let Some(cancel) = self.shared.helper_cancel.lock().unwrap().as_ref() {
            cancel.cancel();
        }
        let _ = self.tx.send(Change::Stop);
        if let Some(thread) = self.thread.lock().unwrap().take() {
            let _ = thread.join();
        }
    }
}
#[derive(Clone)]
pub struct Engine {
    shared: Arc<Shared>,
    control: Arc<Control>,
}

impl Shared {
    fn install(&self, index: Index) {
        self.install_live(Live::new(index));
    }
    fn install_live(&self, next: Live) {
        let previous = self.live.write().unwrap().replace(next);
        // Releasing a large mmap/overlay can be slow on Windows. Readers can
        // use the new snapshot while the old allocation is destroyed.
        drop(previous);
    }
    fn announce(&self) {
        self.ready.notify_all();
        if let Some(notify) = self.notify.lock().unwrap().clone() {
            notify();
        }
    }
    fn update_status(&self) {
        let live = self.live.read().unwrap();
        let mut status = self.status.lock().unwrap();
        if let Some(live) = live.as_ref() {
            status.ready = true;
            status.entries = live.base.n - live.dead_count + live.over.len();
            status.dirs = live.base.d;
            status.index_bytes = live.base.bytes();
        }
        status.skipped = walk::DENIED
            .lock()
            .unwrap()
            .keys()
            .filter(|path| {
                self.roots.iter().any(|root| {
                    path.len() >= root.len()
                        && crate::paths::cmp_ci(&path[..root.len()], root).is_eq()
                        && (path.len() == root.len() || path.get(root.len()) == Some(&b'/'))
                })
            })
            .count();
    }
}

impl Engine {
    pub fn start(options: Options) -> Result<Self, String> {
        Self::start_with_source(options, SourceMode::Auto)
    }
    pub fn start_with_source(options: Options, source: SourceMode) -> Result<Self, String> {
        if options.roots.is_empty() {
            return Err("At least one indexed root is required".into());
        }
        let shared = Arc::new(Shared {
            closing: std::sync::atomic::AtomicBool::new(false),
            roots: options
                .roots
                .iter()
                .map(|p| crate::paths::internal(p))
                .collect(),
            live: RwLock::new(None),
            status: Mutex::new(Status {
                catching_up: true,
                source: "walker".into(),
                ..Default::default()
            }),
            ready: Condvar::new(),
            notify: Mutex::new(None),
            opened: RwLock::new(HashMap::new()),
            journal: Mutex::new(None),
            helper_cancel: Mutex::new(None),
        });
        let (tx, rx) = mpsc::channel();
        let thread_shared = shared.clone();
        let watch_tx = tx.clone();
        let thread = thread::Builder::new()
            .name("seek-index".into())
            .spawn(move || {
                crate::qos::utility();
                if let Err(error) = run(&thread_shared, &options, source, watch_tx, rx) {
                    thread_shared.status.lock().unwrap().error = Some(error);
                    thread_shared.announce();
                }
            })
            .map_err(|e| e.to_string())?;
        Ok(Self {
            shared: shared.clone(),
            control: Arc::new(Control {
                tx,
                thread: Mutex::new(Some(thread)),
                shared,
            }),
        })
    }
    pub fn set_notifier(&self, notify: impl Fn() + Send + Sync + 'static) {
        *self.shared.notify.lock().unwrap() = Some(Arc::new(notify));
        self.shared.announce();
    }
    pub fn status(&self) -> Status {
        self.shared.status.lock().unwrap().clone()
    }
    pub fn wait_ready(&self, timeout: Duration) -> Result<Status, String> {
        let status = self.shared.status.lock().unwrap();
        let (status, timed) = self
            .shared
            .ready
            .wait_timeout_while(status, timeout, |s| s.catching_up && s.error.is_none())
            .unwrap();
        if let Some(error) = &status.error {
            return Err(error.clone());
        }
        if timed.timed_out() || !status.ready {
            return Err("Index/catch-up timed out".into());
        }
        Ok(status.clone())
    }
    /// Original engine's Searcher invocation and materialization, adapted to WTF-8 paths.
    pub fn search(&self, q: &Query) -> Result<Vec<Found>, String> {
        let guard = self.shared.live.read().unwrap();
        let live = guard.as_ref().ok_or("Building index")?;
        let mut path = Vec::new();
        let mut hits: Vec<Found> = Searcher { live }
            .search(q)
            .into_iter()
            .map(|h| {
                let (kind, size, mtime) = match h.over {
                    Some(over) => {
                        let o = live.over[&over];
                        path = over;
                        (o.kind, o.size, o.mtime)
                    }
                    None => {
                        let i = h.idx as usize;
                        live.base.path(i, &mut path);
                        live.attributes(i)
                    }
                };
                Found {
                    path: crate::paths::native(&path),
                    kind,
                    size,
                    mtime,
                    score: h.score,
                }
            })
            .filter(|h| h.kind & FLAG_SYNTH == 0)
            .collect();
        drop(guard);
        if q.tokens.is_empty() {
            hits.sort_by(|a, b| b.mtime.cmp(&a.mtime).then(b.score.cmp(&a.score)));
        }
        let opened = self.shared.opened.read().unwrap();
        let tier = |h: &Found| {
            let name = h
                .path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_lowercase();
            let tokens: Vec<_> = q.tokens.iter().filter(|t| !t.negate).collect();
            if tokens.len() == 1 {
                let text = String::from_utf8_lossy(&tokens[0].text);
                if name == text {
                    return 2;
                }
                if name.starts_with(text.as_ref()) {
                    return 1;
                }
            }
            0
        };
        if !q.tokens.is_empty() {
            let now = crate::query::now_secs();
            hits.sort_by_cached_key(|h| {
                (
                    std::cmp::Reverse(tier(h)),
                    std::cmp::Reverse(
                        h.score
                            + match now.saturating_sub(h.mtime) {
                                0..=86_400 => 6,
                                86_401..=604_800 => 4,
                                604_801..=2_592_000 => 2,
                                _ => 0,
                            }
                            + opened
                                .get(&crate::paths::internal(&h.path))
                                .copied()
                                .unwrap_or(0)
                                .min(5) as i32,
                    ),
                )
            });
        }
        hits.truncate(q.limit);
        Ok(hits)
    }
    /// Complete newest-first namespace, materializing paths only for one page.
    /// The temporary ordinal list is 8 bytes per eligible base entry; it does
    /// not retain formatted rows or a duplicate full-path list for the drive.
    pub fn all_locations_page(
        &self,
        q: &Query,
        offset: usize,
        count: usize,
    ) -> Result<(usize, Vec<Found>), String> {
        if q.cancelled() {
            return Err("Cancelled".into());
        }
        let guard = self.shared.live.read().unwrap();
        let live = guard.as_ref().ok_or("Building index")?;
        let overlay: Vec<_> = live.over.iter().collect();
        let mut order = Vec::new();
        let mut path = Vec::new();
        for i in 1..live.base.n {
            if i % 4096 == 0 && q.cancelled() {
                return Err("Cancelled".into());
            }
            if live.is_dead(i as u32) {
                continue;
            }
            let (kind, size, mtime) = live.attributes(i);
            if kind & FLAG_SYNTH != 0 {
                continue;
            }
            // match_path handles hidden flags and user exclusions consistently
            // with filename search, never omitting a class of readable files.
            if q.hide_hidden_system && kind & (walk::FLAG_HIDDEN | walk::FLAG_SYSTEM) != 0 {
                continue;
            }
            if !q.excluded_paths.is_empty() {
                live.base.path(i, &mut path);
                if q.match_path(&path, kind, size, mtime).is_none() {
                    continue;
                }
            }
            order.push((std::cmp::Reverse(mtime), i as u32));
        }
        for (i, (path, entry)) in overlay.iter().enumerate() {
            if q.cancelled() {
                return Err("Cancelled".into());
            }
            if entry.kind & FLAG_SYNTH == 0
                && q.match_path(path, entry.kind, entry.size, entry.mtime)
                    .is_some()
            {
                order.push((std::cmp::Reverse(entry.mtime), (live.base.n + i) as u32));
            }
        }
        let total = order.len();
        let start = offset.min(total);
        let end = start.saturating_add(count).min(total);
        if start == end {
            return Ok((total, vec![]));
        }
        if start > 0 {
            order.select_nth_unstable(start);
        }
        let rest = &mut order[start..];
        let take = end - start;
        if take < rest.len() {
            rest.select_nth_unstable(take);
        }
        rest[..take].sort_unstable();
        let rows = rest[..take]
            .iter()
            .map(|(_, ordinal)| {
                let i = *ordinal as usize;
                let (kind, size, mtime) = if i < live.base.n {
                    live.base.path(i, &mut path);
                    live.attributes(i)
                } else {
                    let (p, e) = overlay[i - live.base.n];
                    path.clone_from(p);
                    (e.kind, e.size, e.mtime)
                };
                Found {
                    path: crate::paths::native(&path),
                    kind,
                    size,
                    mtime,
                    score: 0,
                }
            })
            .collect();
        Ok((total, rows))
    }
    /// Refresh a bounded set of already displayed hits from the live index.
    /// This allows recent-file views to retain their order without retaining
    /// deleted files or doing a directory walk.
    pub fn refresh_results(&self, previous: &[Found], q: &Query) -> Vec<Found> {
        let guard = self.shared.live.read().unwrap();
        let Some(live) = guard.as_ref() else {
            return vec![];
        };
        previous
            .iter()
            .filter_map(|hit| {
                let mut path = crate::paths::internal(&hit.path);
                let (kind, size, mtime) = if let Some(over) = live.over.get(&path) {
                    (over.kind, over.size, over.mtime)
                } else {
                    let entry = live.base.lookup(&path).filter(|e| !live.is_dead(*e))?;
                    live.base.path(entry as usize, &mut path);
                    live.attributes(entry as usize)
                };
                if kind & FLAG_SYNTH != 0 {
                    return None;
                }
                let score = q.match_path(&path, kind, size, mtime)?;
                Some(Found {
                    path: crate::paths::native(&path),
                    kind,
                    size,
                    mtime,
                    score,
                })
            })
            .collect()
    }
    pub fn checkpoint(&self) -> Result<(), String> {
        let (tx, rx) = mpsc::channel();
        self.control
            .tx
            .send(Change::Checkpoint(tx))
            .map_err(|e| e.to_string())?;
        rx.recv().map_err(|e| e.to_string())?
    }
    pub fn record_open(&self, path: &Path) {
        let _ = self
            .control
            .tx
            .send(Change::Opened(crate::paths::internal(path)));
    }
    /// Explicit user-requested reconciliation; never a polling timer.
    pub fn rebuild(&self) {
        for root in &self.shared.roots {
            let _ = self.control.tx.send(Change::Overflow(root.clone()));
        }
    }
    pub fn refresh_parent(&self, path: &Path) {
        if let Some(parent) = path.parent() {
            let _ = self
                .control
                .tx
                .send(Change::Directories(vec![crate::paths::internal(parent)]));
        }
    }
    /// Direct indexed children; never scans disk or truncates to query top-k.
    /// Is a directory present in the published namespace? Missing is not empty.
    pub fn has_directory(&self, folder: &Path) -> bool {
        let root = crate::paths::internal(folder);
        let guard = self.shared.live.read().unwrap();
        guard.as_ref().is_some_and(|live| {
            live.base
                .lookup(&root)
                .is_some_and(|e| !live.is_dead(e) && live.base.kind()[e as usize] & 3 == KIND_DIR)
                || live.over.get(&root).is_some_and(|e| e.kind & 3 == KIND_DIR)
        })
    }
    pub fn children(&self, folder: &Path, show_hidden: bool, excluded: &[PathBuf]) -> Vec<Found> {
        self.profiled_children(folder, show_hidden, excluded).0
    }
    pub fn largest(&self, query: &Query, folders: bool) -> Vec<Found> {
        self.shared
            .live
            .read()
            .unwrap()
            .as_ref()
            .map_or_else(Vec::new, |live| live.largest(query, folders))
    }
    /// Diagnostic stages: waiting for the index lock, base children, overlay children.
    pub fn profiled_children(
        &self,
        folder: &Path,
        show_hidden: bool,
        excluded: &[PathBuf],
    ) -> (Vec<Found>, [f64; 3]) {
        let started = Instant::now();
        let guard = self.shared.live.read().unwrap();
        let locked = Instant::now();
        let Some(live) = guard.as_ref() else {
            return (vec![], [0.; 3]);
        };
        let mut root = crate::paths::internal(folder);
        while root.last() == Some(&b'/') {
            root.pop();
        }
        let filter = Query {
            hide_hidden_system: !show_hidden,
            excluded_paths: excluded
                .iter()
                .map(|p| {
                    let mut path = crate::paths::internal(p);
                    while path.last() == Some(&b'/') {
                        path.pop();
                    }
                    path
                })
                .collect(),
            ..Default::default()
        };
        let mut rows = vec![];
        let mut path = vec![];
        if let Some(entry) = live.base.lookup(&root).filter(|e| !live.is_dead(*e))
            && let Some(dir) = live.base.dir_of(entry)
        {
            for i in live.base.children(dir) {
                let (kind, size, mtime) = live.attributes(i);
                if live.is_dead(i as u32) || kind & FLAG_SYNTH != 0 || !filter.kind_ok(kind) {
                    continue;
                }
                live.base.path(i, &mut path);
                if !filter.path_allowed(&path) {
                    continue;
                }
                rows.push(Found {
                    path: crate::paths::native(&path),
                    kind,
                    size,
                    mtime,
                    score: 0,
                });
            }
        }
        let base_done = Instant::now();
        for (path, e) in live.overlay_children(&root) {
            if e.kind & FLAG_SYNTH != 0 || !filter.kind_ok(e.kind) || !filter.path_allowed(path) {
                continue;
            }
            rows.push(Found {
                path: crate::paths::native(path),
                kind: e.kind,
                size: e.size,
                mtime: e.mtime,
                score: 0,
            });
        }
        (
            rows,
            [
                (locked - started).as_secs_f64() * 1000.,
                (base_done - locked).as_secs_f64() * 1000.,
                base_done.elapsed().as_secs_f64() * 1000.,
            ],
        )
    }
    /// Complete snapshot used by verify, without applying search limits or ranking.
    pub fn snapshot(&self) -> Vec<Found> {
        let guard = self.shared.live.read().unwrap();
        let Some(live) = guard.as_ref() else {
            return Vec::new();
        };
        let mut path = Vec::new();
        let mut entries = Vec::with_capacity(live.base.n + live.over.len());
        for i in 1..live.base.n {
            if live.is_dead(i as u32) || live.base.kind()[i] & FLAG_SYNTH != 0 {
                continue;
            }
            live.base.path(i, &mut path);
            let (kind, size, mtime) = live.attributes(i);
            entries.push(Found {
                path: crate::paths::native(&path),
                kind,
                size,
                mtime,
                score: 0,
            });
        }
        for (path, e) in &live.over {
            if e.kind & FLAG_SYNTH == 0 {
                entries.push(Found {
                    path: crate::paths::native(path),
                    kind: e.kind,
                    size: e.size,
                    mtime: e.mtime,
                    score: 0,
                });
            }
        }
        entries
    }
}

fn run(
    shared: &Shared,
    options: &Options,
    source: SourceMode,
    tx: mpsc::Sender<Change>,
    rx: mpsc::Receiver<Change>,
) -> Result<(), String> {
    std::fs::create_dir_all(&options.dir).map_err(|e| e.to_string())?;
    let root_bytes: Vec<_> = options
        .roots
        .iter()
        .map(|p| crate::paths::internal(p))
        .collect();
    let mut watchers = Vec::new();
    for root in &root_bytes {
        match Watch::start(root.clone(), tx.clone()) {
            Ok(watch) => watchers.push(watch),
            Err(e) => {
                walk::DENIED
                    .lock()
                    .unwrap()
                    .insert(root.clone(), e.raw_os_error().unwrap_or(0) as u32);
            }
        }
    }
    let root_file = options.dir.join("roots.json");
    let roots_match = std::fs::read(&root_file)
        .ok()
        .and_then(|b| serde_json::from_slice::<Vec<Vec<u8>>>(&b).ok())
        .is_some_and(|saved| saved == root_bytes);
    let load_started = Instant::now();
    let saved = roots_match
        .then(|| Index::load(&options.dir.join("index.bin")))
        .flatten();
    shared.status.lock().unwrap().progress.load_ms = load_started.elapsed().as_secs_f64() * 1000.;
    let started = Instant::now();
    let cached = saved.is_some();
    if let Some(index) = saved {
        *shared.journal.lock().unwrap() = index
            .saved_state()
            .and_then(|b| serde_json::from_slice::<helper_sync::SavedState>(b).ok())
            .filter(|s| s.roots == root_bytes);
        if let Some(state) = shared.journal.lock().unwrap().as_ref() {
            walk::DENIED
                .lock()
                .unwrap()
                .extend(state.skipped.iter().cloned());
        }
        shared.install(index);
        shared.update_status();
        shared.announce();
    }
    if let Ok(bytes) = std::fs::read(options.dir.join("opened.json"))
        && let Ok(items) = serde_json::from_slice::<Vec<(Vec<u8>, u32)>>(&bytes)
    {
        shared.opened.write().unwrap().extend(items);
    }
    let connect_started = Instant::now();
    let mut helper = match helper_sync::Session::start(shared, &root_bytes, source, tx.clone()) {
        Ok(session) => session,
        Err(error) if matches!(source, SourceMode::Auto) => {
            shared.status.lock().unwrap().helper_note =
                Some(format!("Helper unavailable; using walker: {error}"));
            None
        }
        Err(error) => return Err(error),
    };
    shared.status.lock().unwrap().progress.connect_ms =
        connect_started.elapsed().as_secs_f64() * 1000.;
    if helper.is_some() {
        shared.status.lock().unwrap().progress.phase = if cached {
            "Replaying journal"
        } else {
            "Reading drive metadata"
        }
        .into();
        shared.announce();
    }
    let mut helper_initial = false;
    // The optional journal connection starts first; replay can queue while the
    // ordinary-user worker refreshes the folders used for daily work.
    if cached {
        startup::refresh(shared, helper.is_none(), started);
        startup::retry_failures(shared);
    }
    if let Some(session) = helper.as_mut() {
        match helper_sync::initial(shared, &root_bytes, session, Some((&rx, &options.dir))) {
            Ok(()) => helper_initial = true,
            Err(_)
                if shared
                    .helper_cancel
                    .lock()
                    .unwrap()
                    .as_ref()
                    .is_some_and(|cancel| cancel.cancelled()) =>
            {
                return Ok(());
            }
            Err(error) if matches!(source, SourceMode::Helper) => return Err(error),
            Err(error) => {
                shared.status.lock().unwrap().helper_note = Some(error);
                helper = None;
                *shared.journal.lock().unwrap() = None;
            }
        }
    }
    if !helper_initial && shared.live.read().unwrap().is_none() {
        shared.install(Index::build(
            graft_roots(&root_bytes, |root| {
                walk::scan_observed(root, 4, &|count| {
                    {
                        let mut s = shared.status.lock().unwrap();
                        s.progress.phase = "Building index".into();
                        s.progress.completed = count;
                        s.progress.total = None;
                    }
                    shared.announce();
                })
            }),
            0,
            crate::query::now_secs(),
            &[],
        ));
        shared.update_status();
        shared.announce();
    }
    if cached && !helper_initial && !shared.status.lock().unwrap().progress.full_relist_done {
        // NTFS folder mtimes alone do not reliably capture file-content/metadata changes.
        // One launch-only relist per indexed directory keeps the helper-absent path correct.
        let dirs = {
            let live = shared.live.read().unwrap();
            let live = live.as_ref().unwrap();
            let mut path = Vec::new();
            let mut dirs = Vec::with_capacity(live.base.d);
            for &entry in live.base.dir_entry().iter().skip(1) {
                if live.base.kind()[entry as usize] & FLAG_SYNTH == 0 {
                    live.base.path(entry as usize, &mut path);
                    dirs.push(path.clone());
                }
            }
            dirs
        };
        for path in dirs {
            relist(shared, &path, false);
        }
    } else if !cached || helper_initial {
        shared.status.lock().unwrap().progress.phase = "Saving index".into();
        shared.announce();
        save(shared, &options.dir)?;
        std::fs::write(
            &root_file,
            serde_json::to_vec(&root_bytes).map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;
    }
    let mut catch_up_started = Some(started);
    let mut reconciliation = 0u64;
    let _ = tx.send(Change::Reconciled(reconciliation));
    let mut pending = None;
    loop {
        if shared.closing.load(std::sync::atomic::Ordering::Acquire) {
            break;
        }
        // A crawl/relist can race filesystem changes. Keep the catch-up warning
        // until notifications already queued during that work have been applied.
        // Once drained, block on the channel; there is no idle polling.
        let change = match pending.take().map(Ok).unwrap_or_else(|| rx.try_recv()) {
            Ok(change) => change,
            Err(mpsc::TryRecvError::Empty) => {
                if let Some(started) = catch_up_started.take() {
                    shared.update_status();
                    {
                        let mut status = shared.status.lock().unwrap();
                        status.catching_up = false;
                        status.catch_up_ms = started.elapsed().as_secs_f64() * 1000.;
                    }
                    shared.announce();
                }
                match rx.recv() {
                    Ok(change) => change,
                    Err(_) => break,
                }
            }
            Err(mpsc::TryRecvError::Disconnected) => break,
        };
        if std::env::var_os("SEEK_TRACE").is_some() {
            eprintln!("seek event: {change:?}");
        }
        match change {
            Change::Stop => break,
            Change::Reconciled(completed) => {
                // A busy drive need not become completely silent to finish
                // catch-up. This FIFO boundary includes all earlier changes.
                if completed == reconciliation
                    && let Some(started) = catch_up_started.take()
                {
                    let mut status = shared.status.lock().unwrap();
                    status.catching_up = false;
                    status.catch_up_ms = started.elapsed().as_secs_f64() * 1000.;
                }
            }
            Change::HelperAvailable => {
                let mut failed = None;
                let mut reconciled = false;
                if let Some(session) = helper.as_mut() {
                    for turn in 0..64 {
                        let Some(response) = session.try_recv() else {
                            break;
                        };
                        if matches!(response, crate::helper::Response::Overflow { .. }) {
                            reconciliation = reconciliation.wrapping_add(1);
                            reconciled = true;
                            catch_up_started.get_or_insert_with(Instant::now);
                        }
                        if let Err(error) = helper_sync::apply(shared, &root_bytes, response) {
                            failed = Some(error);
                            break;
                        }
                        if turn == 63 {
                            // Continuous journal traffic must not starve the
                            // queued completion boundary or control messages.
                            let _ = tx.send(Change::HelperAvailable);
                        }
                    }
                }
                if let Some(error) = failed {
                    // Cancelling the pipe during intentional shutdown is not
                    // a lost-helper event. Never start a drive crawl on quit.
                    if shared.closing.load(std::sync::atomic::Ordering::Acquire) {
                        break;
                    }
                    helper = None;
                    *shared.journal.lock().unwrap() = None;
                    catch_up_started.get_or_insert_with(Instant::now);
                    {
                        let mut status = shared.status.lock().unwrap();
                        status.catching_up = true;
                        status.source = "walker".into();
                        status.helper_note = Some(format!(
                            "Helper disconnected; reconciling with walker: {error}"
                        ));
                    }
                    shared.announce();
                    for root in &root_bytes {
                        relist(shared, root, true);
                    }
                    reconciliation = reconciliation.wrapping_add(1);
                    reconciled = true;
                }
                if reconciled {
                    let _ = tx.send(Change::Reconciled(reconciliation));
                }
            }
            Change::Checkpoint(reply) => {
                let _ = reply.send(save(shared, &options.dir));
            }
            Change::Opened(path) => {
                record_open(shared, &options.dir, path)?;
            }
            change @ (Change::Directories(_) | Change::Overflow(_)) => {
                let batch = changes::collect(change, &rx);
                pending = batch.pending;
                if batch.paths.iter().any(|(_, recursive)| *recursive) {
                    reconciliation = reconciliation.wrapping_add(1);
                    catch_up_started.get_or_insert_with(Instant::now);
                    shared.status.lock().unwrap().catching_up = true;
                    shared.announce();
                }
                for (path, recursive) in batch.paths {
                    relist(shared, &path, recursive);
                }
                if catch_up_started.is_some() {
                    let _ = tx.send(Change::Reconciled(reconciliation));
                }
            }
            Change::Error(root, error) => {
                walk::DENIED.lock().unwrap().insert(root, error);
                shared.status.lock().unwrap().error =
                    Some(format!("Filesystem watcher failed: Windows error {error}"));
            }
        }
        shared.update_status();
        shared.announce();
    }
    drop(watchers);
    Ok(())
}

fn record_open(shared: &Shared, dir: &Path, path: Vec<u8>) -> Result<(), String> {
    let mut opened = shared.opened.write().unwrap();
    let count = opened.entry(path).or_default();
    *count = count.saturating_add(1);
    let mut items: Vec<_> = opened.iter().map(|(p, &n)| (p.clone(), n)).collect();
    items.sort_by_key(|(_, n)| std::cmp::Reverse(*n));
    items.truncate(4096);
    *opened = items.iter().cloned().collect();
    std::fs::write(
        dir.join("opened.json"),
        serde_json::to_vec(&items).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())
}

fn relist(shared: &Shared, path: &[u8], recursive: bool) {
    let began = Instant::now();
    let (fetched, snapshot) = {
        let live = shared.live.read().unwrap();
        let live = live.as_ref().unwrap();
        let fetched = live.fetch(path, recursive);
        let snapshot = live.bulk_update(&fetched).then(|| live.fork_for_update());
        (fetched, snapshot)
    };
    shared.status.lock().unwrap().progress.read_ms += began.elapsed().as_secs_f64() * 1000.;
    let began = Instant::now();
    if let Some(mut snapshot) = snapshot {
        snapshot.apply(fetched);
        shared.install_live(snapshot);
        shared.status.lock().unwrap().progress.apply_ms += began.elapsed().as_secs_f64() * 1000.;
        return;
    }
    shared
        .live
        .write()
        .unwrap()
        .as_mut()
        .unwrap()
        .apply(fetched);
    shared.status.lock().unwrap().progress.apply_ms += began.elapsed().as_secs_f64() * 1000.;
}
fn save(shared: &Shared, dir: &Path) -> Result<(), String> {
    let saving = Instant::now();
    let listings = shared.live.read().unwrap().as_ref().unwrap().to_listings();
    let mut state = shared.journal.lock().unwrap().clone();
    if let Some(state) = state.as_mut() {
        state.skipped = walk::DENIED
            .lock()
            .unwrap()
            .iter()
            .filter(|(path, _)| {
                state.roots.iter().any(|root| {
                    path.len() >= root.len()
                        && crate::paths::cmp_ci(&path[..root.len()], root).is_eq()
                        && (path.len() == root.len() || path.get(root.len()) == Some(&b'/'))
                })
            })
            .map(|(path, error)| (path.clone(), *error))
            .collect();
    }
    let event_id = state
        .as_ref()
        .and_then(|s| s.cursors.first())
        .map_or(0, |c| c.next_usn as u64);
    let index = Index::build(listings, event_id, crate::query::now_secs(), &[]);
    // File-backed mappings are immutable snapshots; replacement is done after dropping the old map.
    shared.install(index);
    let path = dir.join("index.bin");
    shared
        .live
        .read()
        .unwrap()
        .as_ref()
        .unwrap()
        .base
        .save_with_state(
            &path,
            state
                .as_ref()
                .map(serde_json::to_vec)
                .transpose()
                .map_err(|e| e.to_string())?
                .as_deref(),
        )
        .map_err(|e| e.to_string())?;
    let mapped = Index::load(&path).ok_or("Saved index failed validation")?;
    shared.install(mapped);
    shared.status.lock().unwrap().progress.save_ms += saving.elapsed().as_secs_f64() * 1000.;
    Ok(())
}

fn graft_roots(roots: &[Vec<u8>], mut scan: impl FnMut(&[u8]) -> Vec<Listing>) -> Vec<Listing> {
    let mut all: Vec<Listing> = vec![Listing {
        id: 0,
        names: Vec::new(),
        ents: Vec::new(),
    }];
    let mut grafts = BTreeMap::new();
    grafts.insert(b"/".to_vec(), 0u32);
    for root in roots {
        let mut parent = 0;
        let mut path = Vec::new();
        for component in root.split(|&b| b == b'/').filter(|c| !c.is_empty()) {
            path.push(b'/');
            path.extend_from_slice(component);
            let child = if let Some(&id) = grafts.get(&path) {
                id
            } else {
                let id = all.len() as u32;
                let listing = &mut all[parent as usize];
                listing.ents.push(RawEnt {
                    name_off: listing.names.len() as u32,
                    name_len: component.len() as u16,
                    kind: KIND_DIR | FLAG_SYNTH,
                    size: 0,
                    mtime: 0,
                    child: id,
                });
                listing.names.extend_from_slice(component);
                all.push(Listing {
                    id,
                    names: Vec::new(),
                    ents: Vec::new(),
                });
                grafts.insert(path.clone(), id);
                id
            };
            parent = child;
        }
        // Mark the selected root as a real directory, retain synthetic ancestors only.
        for listing in &mut all {
            for entry in &mut listing.ents {
                if entry.child == parent
                    && let Some(attrs) = crate::live::lstat(root)
                {
                    entry.kind = attrs.kind;
                    entry.mtime = attrs.mtime;
                }
            }
        }
        let mut listings = scan(root);
        let offset = all.len() as u32;
        for listing in &mut listings {
            listing.id = if listing.id == 0 {
                parent
            } else {
                offset + listing.id - 1
            };
            for entry in &mut listing.ents {
                if entry.child != NONE {
                    entry.child = offset + entry.child - 1;
                }
            }
        }
        if let Some(pos) = listings.iter().position(|l| l.id == parent) {
            all[parent as usize] = listings.swap_remove(pos);
        }
        listings.sort_by_key(|l| l.id);
        all.extend(listings);
    }
    all
}
