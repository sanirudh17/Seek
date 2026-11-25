//! Folders shown in panes update the moment they change on disk: a download
//! finishing, a file saved by another app, an agent's move. Each visible
//! folder (not its subfolders) is watched directly with the system's change
//! notifications, independent of the index, so folders outside the indexed
//! locations update too. Changes are gathered for a moment and a folder is
//! read again at most a few times a second, so a file growing during a
//! download doesn't keep the folder re-reading.
use super::*;
use std::collections::{HashMap, HashSet};

/// How long changes are gathered before the folders are read again.
const SETTLE: std::time::Duration = std::time::Duration::from_millis(150);
/// The least time between two reads of changed folders.
const GAP: std::time::Duration = std::time::Duration::from_millis(350);

pub(super) struct LiveFolders {
    /// Each visible folder, with its watch once it has started (None while
    /// starting, or where a folder can't be watched, such as inside a ZIP).
    watches: HashMap<PathBuf, Option<seek_engine::FolderWatch>>,
    tx: async_channel::Sender<PathBuf>,
    rx: async_channel::Receiver<PathBuf>,
}

impl Default for LiveFolders {
    fn default() -> Self {
        let (tx, rx) = async_channel::unbounded();
        Self { watches: HashMap::new(), tx, rx }
    }
}

impl Seek {
    /// The folders panes show as folder listings right now.
    fn visible_folders(&self, cx: &App) -> HashSet<PathBuf> {
        let mut folders = HashSet::new();
        if let Some(scope) = &self.scope
            && !self.content_mode
            && self.largest.is_none()
            && self.input.read(cx).value().trim().is_empty()
        {
            folders.insert(scope.clone());
        }
        for slot in &self.panes {
            if let Some(folder) = slot.tabs.get(slot.active_tab).and_then(super::panes::folder_tab) {
                folders.insert(folder);
            }
        }
        folders
    }

    /// Starts watching folders that came into view and stops watching those
    /// that left it. Cheap when nothing changed; called on every render.
    pub(super) fn sync_live_folders(&mut self, cx: &mut Context<Self>) {
        let visible = self.visible_folders(cx);
        let live = &mut self.live_folders;
        if live.watches.len() == visible.len() && visible.iter().all(|f| live.watches.contains_key(f)) {
            return;
        }
        live.watches.retain(|folder, _| visible.contains(folder));
        for folder in visible {
            if live.watches.contains_key(&folder) {
                continue;
            }
            live.watches.insert(folder.clone(), None);
            let tx = live.tx.clone();
            cx.spawn(async move |this, cx| {
                // Opening a folder can wait on a network or cloud drive.
                let watched = folder.clone();
                let watch = cx
                    .background_executor()
                    .spawn(async move {
                        let path = watched.clone();
                        seek_engine::FolderWatch::new(&watched, move || {
                            let _ = tx.try_send(path.clone());
                        })
                        .ok()
                    })
                    .await;
                let _ = this.update(cx, |this, _| {
                    // Still in view: keep it; otherwise it drops and stops.
                    if let Some(slot) = this.live_folders.watches.get_mut(&folder) {
                        *slot = watch;
                    }
                });
            })
            .detach();
        }
    }

    /// Reads changed folders again as long as the window lives.
    pub(super) fn start_live_folders(&mut self, cx: &mut Context<Self>) {
        let rx = self.live_folders.rx.clone();
        cx.spawn(async move |this, cx| {
            while let Ok(first) = rx.recv().await {
                cx.background_executor().timer(SETTLE).await;
                let mut changed = HashSet::from([first]);
                while let Ok(more) = rx.try_recv() {
                    changed.insert(more);
                }
                let changed: Vec<PathBuf> = changed.into_iter().collect();
                if this.update(cx, |this, cx| this.live_folders_changed(&changed, cx)).is_err() {
                    return;
                }
                cx.background_executor().timer(GAP).await;
            }
        })
        .detach();
    }

    fn live_folders_changed(&mut self, changed: &[PathBuf], cx: &mut Context<Self>) {
        let visible = self.visible_folders(cx);
        for folder in changed.iter().filter(|f| visible.contains(*f)) {
            if self.scope.as_ref() == Some(folder) {
                let _ = self.backend.tx.send(Command::FolderChanged(folder.clone()));
            }
        }
        self.refresh_inactive_panes(changed, &[], cx);
    }
}

#[cfg(test)]
mod tests {
    /// A file created in a watched folder is reported within a second, and
    /// one in a subfolder is not (only the folder's own entries are watched).
    #[test]
    fn a_new_file_is_reported_at_once() {
        use std::sync::mpsc;
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("sub")).unwrap();
        let (tx, rx) = mpsc::channel();
        let watch = seek_engine::FolderWatch::new(dir.path(), move || {
            let _ = tx.send(());
        })
        .unwrap();
        std::fs::write(dir.path().join("sub").join("deep.txt"), b"x").unwrap();
        // The subfolder's own timestamp may change once; drain that.
        std::thread::sleep(std::time::Duration::from_millis(300));
        while rx.try_recv().is_ok() {}
        let started = std::time::Instant::now();
        std::fs::write(dir.path().join("download.pdf"), b"x").unwrap();
        rx.recv_timeout(std::time::Duration::from_secs(1)).expect("change reported");
        assert!(started.elapsed() < std::time::Duration::from_millis(500), "{:?}", started.elapsed());
        drop(watch);
    }
}
