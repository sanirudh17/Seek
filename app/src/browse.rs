//! Directory metadata can be ready before the full search index. Finite worker
//! reads keep sidebar navigation independent of first-index completion.
use crate::backend::Command;
use seek_engine::{Found, Query};
use std::{
    collections::{HashSet, VecDeque},
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
fn fetch(folder: PathBuf, tx: &Sender<Command>) {
    let began = Instant::now();
    let result = read(&folder);
    #[cfg(feature = "benchmarks")]
    crate::metrics::interaction(
        "folder_metadata",
        serde_json::json!({"folder":folder,"rows":result.as_ref().map(|r|r.len()).ok(),"error":result.as_ref().err(),"elapsed_ms":began.elapsed().as_secs_f64()*1000.}),
    );
    let _ = began;
    let _ = tx.send(Command::BrowseFinished(folder, result));
}
type Entry = (Vec<u8>, Result<Vec<Found>, String>);
pub struct Cache {
    entries: VecDeque<Entry>,
    pending: HashSet<Vec<u8>>,
    active: Arc<AtomicUsize>,
    tx: Sender<Command>,
}
impl Cache {
    pub fn new(tx: Sender<Command>) -> Self {
        Self {
            entries: VecDeque::new(),
            pending: HashSet::new(),
            active: Arc::new(AtomicUsize::new(0)),
            tx,
        }
    }
    pub fn prime(&self, folders: &[(String, PathBuf)]) {
        let folders: Vec<_> = folders.iter().map(|(_, p)| p.clone()).collect();
        let tx = self.tx.clone();
        std::thread::Builder::new()
            .name("seek-priority-folders".into())
            .spawn(move || {
                for folder in folders {
                    fetch(folder, &tx);
                }
            })
            .expect("priority directory metadata worker");
    }
    pub fn invalidate(&mut self, folder: &Path) {
        let k = key(folder);
        self.entries.retain(|(p, _)| *p != k);
    }
    pub fn finish(&mut self, folder: PathBuf, result: Result<Vec<Found>, String>) {
        let k = key(&folder);
        self.pending.remove(&k);
        self.entries.retain(|(p, _)| *p != k);
        self.entries.push_back((k, result));
        while self.entries.len() > 1
            && (self.entries.len() > 8
                || self
                    .entries
                    .iter()
                    .map(|(_, r)| r.as_ref().map_or(0, Vec::len))
                    .sum::<usize>()
                    > 32_000)
        {
            self.entries.pop_front();
        }
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
        if !self.pending.contains(&k) && self.active.load(Ordering::Acquire) < 2 {
            self.pending.insert(k);
            self.active.fetch_add(1, Ordering::AcqRel);
            let folder = folder.to_owned();
            let tx = self.tx.clone();
            let active = self.active.clone();
            std::thread::Builder::new()
                .name("seek-folder-metadata".into())
                .spawn(move || {
                    let result = read(&folder);
                    active.fetch_sub(1, Ordering::AcqRel);
                    let _ = tx.send(Command::BrowseFinished(folder, result));
                })
                .expect("folder metadata worker");
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
        if let Command::BrowseFinished(path, result) =
            rx.recv_timeout(std::time::Duration::from_secs(5)).unwrap()
        {
            cache.finish(path, result)
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
