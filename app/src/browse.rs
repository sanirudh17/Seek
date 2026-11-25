//! Directory metadata can be ready before the full search index. Finite worker
//! reads keep sidebar navigation independent of first-index completion.
use crate::backend::Command;
use seek_engine::{Found, Query};
use std::{
    collections::{HashMap, VecDeque},
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
        mpsc::Sender,
    },
    time::Instant,
};

fn key(path: &Path) -> Vec<u8> {
    let mut k = seek_engine::paths::internal(path);
    k.make_ascii_lowercase();
    k
}
pub fn read(folder: &Path) -> Result<Vec<Found>, String> {
    // A ZIP file, or a folder inside one, lists its entries read-only.
    if let Some((zip, inner)) = crate::archive::split(folder) {
        return crate::archive::list(&zip, &inner);
    }
    if !std::fs::metadata(folder)
        .map_err(|e| e.to_string())?
        .is_dir()
    {
        return Err("This location is not a folder".into());
    }
    let root = seek_engine::paths::internal(folder);
    let listing = seek_engine::walk::list_one(&root).ok_or_else(|| {
        let code = seek_engine::walk::DENIED
            .lock()
            .unwrap()
            .get(&root)
            .copied()
            .unwrap_or(0);
        std::io::Error::from_raw_os_error(code as i32).to_string()
    })?;
    Ok(listing
        .ents
        .iter()
        .map(|entry| {
            let name = &listing.names
                [entry.name_off as usize..entry.name_off as usize + entry.name_len as usize];
            let mut path = root.clone();
            path.push(b'/');
            path.extend(name);
            Found {
                path: seek_engine::paths::native(&path),
                kind: entry.kind,
                size: entry.size,
                mtime: entry.mtime,
                score: 0,
            }
        })
        .collect())
}
/// One finite folder read with the same visibility, exclusion and newest-first
/// order the search worker applies. Used for panes that are not focused, so
/// their listings never share or cancel the focused pane's search generation.
pub fn listing(folder: &Path, hidden: bool, excluded: &[PathBuf]) -> Result<Vec<Found>, String> {
    let q = Query {
        hide_hidden_system: !hidden,
        excluded_paths: excluded
            .iter()
            .map(|p| seek_engine::paths::internal(p))
            .collect(),
        ..Default::default()
    };
    let mut files: Vec<Found> = read(folder)?
        .into_iter()
        .filter(|r| q.kind_ok(r.kind) && q.path_allowed(&seek_engine::paths::internal(&r.path)))
        .collect();
    files.sort_unstable_by(|a, b| b.mtime.cmp(&a.mtime).then(a.path.cmp(&b.path)));
    Ok(files)
}
fn fetch(folder: PathBuf, revision: u64, tx: &Sender<Command>, active: Option<&AtomicUsize>) {
    let began = Instant::now();
    let result = read(&folder);
    #[cfg(feature = "benchmarks")]
    crate::metrics::interaction(
        "folder_metadata",
        serde_json::json!({"folder":folder,"rows":result.as_ref().map(|r|r.len()).ok(),"error":result.as_ref().err(),"elapsed_ms":began.elapsed().as_secs_f64()*1000.}),
    );
    let _ = began;
    if let Some(active) = active {
        active.fetch_sub(1, Ordering::AcqRel);
    }
    let _ = tx.send(Command::BrowseFinished(folder, revision, result));
}
type Entry = (Vec<u8>, Result<Vec<Found>, String>);
pub struct Cache {
    entries: VecDeque<Entry>,
    pending: HashMap<Vec<u8>, u64>,
    revisions: HashMap<Vec<u8>, u64>,
    next_revision: u64,
    queued: VecDeque<PathBuf>,
    active: Arc<AtomicUsize>,
    tx: Sender<Command>,
    /// Folders that changed while being read: read again once that read ends,
    /// since it may have listed them before the change.
    again: std::collections::HashSet<Vec<u8>>,
}
impl Cache {
    pub fn new(tx: Sender<Command>) -> Self {
        Self {
            entries: VecDeque::new(),
            pending: HashMap::new(),
            revisions: HashMap::new(),
            next_revision: 0,
            queued: VecDeque::new(),
            active: Arc::new(AtomicUsize::new(0)),
            tx,
            again: Default::default(),
        }
    }
    pub fn prime(&mut self, folders: &[(String, PathBuf)]) {
        for (_, folder) in folders {
            self.queue(folder, false);
        }
    }
    /// Provisional recent files while the first whole-drive index is building.
    /// These finite direct listings never stand in for a complete drive search.
    pub fn recent(
        &mut self,
        folders: &[(String, PathBuf)],
        hidden: bool,
        excluded: &[PathBuf],
    ) -> Vec<Found> {
        let mut rows = vec![];
        let mut seen = std::collections::HashSet::new();
        for (_, folder) in folders {
            if !self.entries.iter().any(|(path, _)| *path == key(folder)) {
                // Keep the startup priority order; only explicit folder clicks
                // should promote a queued read ahead of the other locations.
                self.queue(folder, false);
                continue;
            }
            if let Some(Ok(files)) = self.get(folder, hidden, excluded) {
                rows.extend(files.into_iter().filter(|file| {
                    file.kind & 3 == seek_engine::walk::KIND_FILE && seen.insert(key(&file.path))
                }));
            }
        }
        rows.sort_unstable_by(|a, b| b.mtime.cmp(&a.mtime).then(a.path.cmp(&b.path)));
        rows.truncate(200);
        rows
    }
    pub fn invalidate(&mut self, folder: &Path) {
        let k = key(folder);
        self.entries.retain(|(p, _)| *p != k);
        self.next_revision += 1;
        self.revisions.insert(k.clone(), self.next_revision);
        self.pending.remove(&k);
    }
    pub fn refresh(&mut self, folder: &Path) {
        let k = key(folder);
        if self.pending.contains_key(&k) {
            self.again.insert(k);
            return;
        }
        self.queue(folder, true);
    }
    fn queue(&mut self, folder: &Path, priority: bool) {
        let k = key(folder);
        if self.pending.contains_key(&k) {
            return;
        }
        self.queued.retain(|p| key(p) != k);
        if self.active.load(Ordering::Acquire) >= 2 {
            if priority {
                self.queued.push_front(folder.to_owned());
            } else {
                self.queued.push_back(folder.to_owned());
            }
            return;
        }
        self.next_revision += 1;
        let revision = self.next_revision;
        self.pending.insert(k.clone(), revision);
        self.revisions.insert(k, revision);
        self.active.fetch_add(1, Ordering::AcqRel);
        let folder = folder.to_owned();
        let tx = self.tx.clone();
        let active = self.active.clone();
        std::thread::Builder::new()
            .name("seek-folder-metadata".into())
            .spawn(move || {
                fetch(folder, revision, &tx, Some(&active));
            })
            .expect("folder metadata worker");
    }
    fn drain(&mut self) {
        while self.active.load(Ordering::Acquire) < 2 {
            let Some(folder) = self.queued.pop_front() else {
                break;
            };
            self.queue(&folder, false);
        }
    }
    pub fn finish(&mut self, folder: PathBuf, revision: u64, result: Result<Vec<Found>, String>) {
        let k = key(&folder);
        if self.revisions.get(&k) != Some(&revision) {
            self.drain();
            return;
        }
        self.pending.remove(&k);
        let again = self.again.remove(&k);
        self.entries.retain(|(p, _)| *p != k);
        self.entries.push_back((k, result));
        if again {
            self.queue(&folder, true);
        }
        while self.entries.len() > 1
            && (self.entries.len() > 8
                || self
                    .entries
                    .iter()
                    .map(|(_, r)| r.as_ref().map_or(0, Vec::len))
                    .sum::<usize>()
                    > 32_000)
        {
            if let Some((old, _)) = self.entries.pop_front()
                && !self.pending.contains_key(&old)
            {
                self.revisions.remove(&old);
            }
        }
        self.drain();
    }
    pub fn get(
        &mut self,
        folder: &Path,
        hidden: bool,
        excluded: &[PathBuf],
    ) -> Option<Result<Vec<Found>, String>> {
        let k = key(folder);
        if let Some((_, entry)) = self.entries.iter().find(|(p, _)| *p == k) {
            let q = Query {
                hide_hidden_system: !hidden,
                excluded_paths: excluded
                    .iter()
                    .map(|p| seek_engine::paths::internal(p))
                    .collect(),
                ..Default::default()
            };
            return Some(
                entry
                    .as_ref()
                    .map(|rows| {
                        rows.iter()
                            .filter(|r| {
                                q.kind_ok(r.kind)
                                    && q.path_allowed(&seek_engine::paths::internal(&r.path))
                            })
                            .cloned()
                            .collect()
                    })
                    .map_err(Clone::clone),
            );
        }
        self.refresh(folder);
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn receive(cache: &mut Cache, rx: &std::sync::mpsc::Receiver<Command>) {
        match rx.recv_timeout(std::time::Duration::from_secs(5)).unwrap() {
            Command::BrowseFinished(path, revision, result) => cache.finish(path, revision, result),
            _ => panic!("unexpected command"),
        }
    }
    #[test]
    fn stale_reads_cannot_restore_deleted_rows_and_busy_cache_prioritizes_navigation() {
        let tmp = tempfile::tempdir().unwrap();
        let folder = tmp.path().join("Requested");
        std::fs::create_dir(&folder).unwrap();
        let file = folder.join("Owned.txt");
        std::fs::write(&file, "owned").unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        let mut cache = Cache::new(tx);
        cache.refresh(&folder);
        let Command::BrowseFinished(path, revision, result) =
            rx.recv_timeout(std::time::Duration::from_secs(5)).unwrap()
        else {
            panic!()
        };
        std::fs::remove_file(&file).unwrap();
        cache.invalidate(&folder);
        cache.finish(path, revision, result);
        assert!(cache.get(&folder, true, &[]).is_none());
        receive(&mut cache, &rx);
        assert!(cache.get(&folder, true, &[]).unwrap().unwrap().is_empty());
        // Simulate both finite workers busy, then request another folder.
        cache.active.store(2, Ordering::Release);
        cache.queue(&tmp.path().join("Background"), false);
        cache.refresh(&folder);
        assert_eq!(cache.queued.front(), Some(&folder));
        assert!(cache.get(&folder, true, &[]).unwrap().unwrap().is_empty());
        cache.active.store(0, Ordering::Release);
        cache.drain();
        receive(&mut cache, &rx);
        receive(&mut cache, &rx);
        assert!(cache.queued.is_empty());
    }
    #[test]
    fn warm_folder_navigation_returns_cached_contents_without_waiting_for_refresh() {
        let tmp = tempfile::tempdir().unwrap();
        for i in 0..400 {
            std::fs::write(tmp.path().join(format!("Owned-{i:04}.txt")), "owned").unwrap();
        }
        let expected: std::collections::BTreeSet<_> = std::fs::read_dir(tmp.path())
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        let (tx, rx) = std::sync::mpsc::channel();
        let mut cache = Cache::new(tx);
        let cold = Instant::now();
        cache.refresh(tmp.path());
        receive(&mut cache, &rx);
        let cold_ms = cold.elapsed().as_secs_f64() * 1000.;
        let mut timings = vec![];
        for _ in 0..100 {
            let began = Instant::now();
            let rows = cache.get(tmp.path(), true, &[]).unwrap().unwrap();
            timings.push(began.elapsed().as_secs_f64() * 1000.);
            assert_eq!(
                rows.iter()
                    .map(|r| r.path.file_name().unwrap().to_owned())
                    .collect::<std::collections::BTreeSet<_>>(),
                expected
            );
        }
        timings.sort_by(f64::total_cmp);
        println!(
            "directory cache: 400 correct rows; first metadata {:.3} ms; warm p95 {:.3} ms (100 accesses, test profile)",
            cold_ms, timings[94]
        );
    }
    #[test]
    fn fresh_metadata_cache_distinguishes_complete_empty_missing_and_unindexed() {
        let tmp = tempfile::tempdir().unwrap();
        let folder = tmp.path().join("OneDrive").join("Documents");
        std::fs::create_dir_all(&folder).unwrap();
        for name in ["report.docx", "note.txt", "other.pdf"] {
            std::fs::write(folder.join(name), "owned").unwrap();
        }
        let expected: std::collections::BTreeSet<_> = std::fs::read_dir(&folder)
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        let (tx, rx) = std::sync::mpsc::channel();
        let mut cache = Cache::new(tx);
        assert!(cache.get(&folder, true, &[]).is_none());
        if let Command::BrowseFinished(path, revision, result) =
            rx.recv_timeout(std::time::Duration::from_secs(5)).unwrap()
        {
            cache.finish(path, revision, result)
        } else {
            panic!("wrong reply")
        }
        let extended = PathBuf::from(format!(r"\\?\{}\", folder.to_string_lossy().to_uppercase()));
        let rows = cache.get(&extended, true, &[]).unwrap().unwrap();
        assert_eq!(
            rows.iter()
                .map(|r| r.path.file_name().unwrap().to_owned())
                .collect::<std::collections::BTreeSet<_>>(),
            expected
        );
        let empty = tmp.path().join("Empty");
        std::fs::create_dir(&empty).unwrap();
        assert!(read(&empty).unwrap().is_empty());
        assert!(read(&tmp.path().join("Missing")).is_err());
        let engine = seek_engine::Engine::start_with_source(
            seek_engine::Options {
                dir: tmp.path().join("index"),
                roots: vec![empty],
            },
            seek_engine::SourceMode::Walker,
        )
        .unwrap();
        engine
            .wait_ready(std::time::Duration::from_secs(5))
            .unwrap();
        assert!(!engine.has_directory(&folder));
        assert_eq!(read(&folder).unwrap().len(), 3);
    }
    #[test]
    fn redirected_junction_and_non_c_drive_letter_keep_all_direct_children() {
        use std::os::windows::process::CommandExt;
        let tmp = tempfile::tempdir().unwrap();
        let target = tmp.path().join("OneDrive").join("Documents");
        std::fs::create_dir_all(&target).unwrap();
        std::fs::write(target.join("redirected.txt"), "owned").unwrap();
        let link = tmp.path().join("Alias");
        let status = std::process::Command::new("cmd.exe")
            .args(["/C", "mklink", "/J"])
            .arg(&link)
            .arg(&target)
            .creation_flags(0x08000000)
            .output()
            .unwrap();
        assert!(
            status.status.success(),
            "{}",
            String::from_utf8_lossy(&status.stderr)
        );
        assert_eq!(read(&link).unwrap().len(), 1);
        // Remove the owned junction itself; preserve its target.
        std::fs::remove_dir(&link).unwrap();
        assert!(target.join("redirected.txt").exists());
        let mask = unsafe { windows_sys::Win32::Storage::FileSystem::GetLogicalDrives() };
        let letter = (b'T'..=b'Z')
            .rev()
            .find(|c| mask & (1 << (*c - b'A')) == 0)
            .expect("a free temporary drive letter");
        let drive = format!("{}:", letter as char);
        let status = std::process::Command::new("subst.exe")
            .arg(&drive)
            .arg(&target)
            .creation_flags(0x08000000)
            .status()
            .unwrap();
        assert!(status.success());
        let rows = read(Path::new(&format!(r"{}\", drive)));
        let undo = std::process::Command::new("subst.exe")
            .arg(&drive)
            .arg("/D")
            .creation_flags(0x08000000)
            .status()
            .unwrap();
        assert!(undo.success());
        let rows = rows.unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].path.file_name().unwrap(), "redirected.txt");
        assert!(target.join("redirected.txt").exists());
    }
}

/// Most entries a live search looks at, and how long it may run.
const LIVE_ENTRIES: usize = 300_000;
const LIVE_DEADLINE: std::time::Duration = std::time::Duration::from_secs(4);

/// Searches folders by walking them, with the engine's own matching
/// (`Query::match_path`), for when the index can't answer: it is still being
/// built, or the folder is outside every indexed location. Bounded by entry
/// count and time, cancelled with the query; says whether it stopped early.
pub fn live_search(folders: &[PathBuf], query: &Query) -> (Vec<Found>, bool) {
    let deadline = Instant::now() + LIVE_DEADLINE;
    let mut found = Vec::new();
    let mut seen = 0usize;
    let mut stopped = false;
    let mut queue: VecDeque<PathBuf> = folders.iter().cloned().collect();
    // The query's scope is the folder itself; the walk already stays inside.
    let mut query = query.clone();
    query.scope = None;
    while let Some(folder) = queue.pop_front() {
        if query.cancelled() {
            stopped = true;
            break;
        }
        if seen >= LIVE_ENTRIES || Instant::now() >= deadline {
            stopped = true;
            break;
        }
        let Ok(entries) = read(&folder) else { continue };
        for mut entry in entries {
            seen += 1;
            let path = seek_engine::paths::internal(&entry.path);
            if entry.kind & 3 == seek_engine::walk::KIND_DIR && query.path_allowed(&path) {
                queue.push_back(entry.path.clone());
            }
            if let Some(score) = query.match_path(&path, entry.kind, entry.size, entry.mtime) {
                entry.score = score;
                found.push(entry);
            }
        }
    }
    found.sort_unstable_by(|a, b| b.score.cmp(&a.score).then_with(|| a.path.cmp(&b.path)));
    found.truncate(query.limit.max(1));
    (found, stopped)
}

#[cfg(test)]
mod live_tests {
    use super::*;

    #[test]
    fn live_search_matches_like_the_index() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("deep/er")).unwrap();
        std::fs::write(dir.path().join("notes.txt"), "x").unwrap();
        std::fs::write(dir.path().join("deep/er/plan.txt"), "x").unwrap();
        std::fs::write(dir.path().join("trip.md"), "x").unwrap();
        let mut query = Query::parse(".txt", "").unwrap();
        query.limit = 200;
        let (found, stopped) = live_search(&[dir.path().to_path_buf()], &query);
        assert!(!stopped);
        let names: Vec<_> = found
            .iter()
            .map(|f| f.path.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names.len(), 2, "{names:?}");
        assert!(names.contains(&"plan.txt".to_string()));
    }
}
