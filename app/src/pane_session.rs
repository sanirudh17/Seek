//! Owned-profile navigation persistence; no index/results/drafts are serialized.
use crate::pane_layout::Session;
use std::{
    io::Read,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, mpsc},
};

const NAME: &str = "pane-session.json";
const LIMIT: usize = 1024 * 1024;

pub fn load(directory: &Path) -> Result<Option<Session>, String> {
    let file = match std::fs::File::open(directory.join(NAME)) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.to_string()),
    };
    let mut bytes = vec![];
    file.take(LIMIT as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| error.to_string())?;
    if bytes.len() > LIMIT {
        return Err("Session exceeds its size limit; original file preserved".into());
    }
    let value: serde_json::Value = serde_json::from_slice(&bytes)
        .map_err(|error| format!("Session could not be read; original file preserved: {error}"))?;
    if value["version"].as_u64() != Some(crate::pane_layout::VERSION.into()) {
        return Err("Session belongs to an unsupported version; original file preserved".into());
    }
    let session: Session = serde_json::from_value(value).map_err(|error| error.to_string())?;
    session.validate()?;
    Ok(Some(session))
}

pub struct Store {
    state: Arc<State>,
    tx: mpsc::SyncSender<()>,
}
struct State {
    directory: PathBuf,
    latest: Mutex<Option<Session>>,
}
impl State {
    fn flush(&self) -> Result<(), String> {
        let _guard = crate::settings::FILE_LOCK
            .lock()
            .map_err(|error| error.to_string())?;
        let value = self
            .latest
            .lock()
            .map_err(|error| error.to_string())?
            .clone();
        let Some(session) = value else {
            return Ok(());
        };
        session.validate()?;
        // Recheck before each write: a newer/corrupt file installed externally
        // after startup must not be overwritten by this process either.
        load(&self.directory)?;
        let value = serde_json::to_value(session).map_err(|error| error.to_string())?;
        if serde_json::to_vec_pretty(&value)
            .map_err(|error| error.to_string())?
            .len()
            > LIMIT
        {
            return Err("Session exceeds its save limit; previous session preserved".into());
        }
        crate::settings::atomic_json(&self.directory, NAME, &value)
            .map_err(|error| error.to_string())
    }
}
impl Store {
    pub fn new(directory: PathBuf, cx: &mut gpui::App) -> Self {
        let store = Self::start(directory);
        let state = store.state.clone();
        cx.on_app_quit(move |cx| {
            let state = state.clone();
            cx.background_executor().spawn(async move {
                if let Err(error) = state.flush() {
                    eprintln!("Cannot save pane session: {error}");
                }
            })
        })
        .detach();
        store
    }
    fn start(directory: PathBuf) -> Self {
        let state = Arc::new(State {
            directory,
            latest: Mutex::new(None),
        });
        let worker = state.clone();
        let (tx, rx) = mpsc::sync_channel(1);
        std::thread::Builder::new()
            .name("seek-pane-session".into())
            .spawn(move || {
                while rx.recv().is_ok() {
                    while rx
                        .recv_timeout(std::time::Duration::from_millis(80))
                        .is_ok()
                    {}
                    if let Err(error) = worker.flush() {
                        eprintln!("Cannot save pane session: {error}");
                    }
                }
            })
            .expect("start pane session writer");
        Self { state, tx }
    }
    pub fn remember(&self, session: Session) -> Result<(), String> {
        session.validate()?;
        *self
            .state
            .latest
            .lock()
            .map_err(|error| error.to_string())? = Some(session);
        let _ = self.tx.try_send(());
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        pane_layout::{Axis, PaneId},
        settings::View,
    };
    #[test]
    fn sessions_round_trip_and_newer_corrupt_or_oversized_files_are_preserved() {
        let root = tempfile::tempdir().unwrap();
        assert!(load(root.path()).unwrap().is_none());
        let store = Store::start(root.path().into());
        let mut session = Session::initial(View::Gallery);
        session.split(PaneId(1), Axis::Horizontal).unwrap();
        store.remember(session.clone()).unwrap();
        store.state.flush().unwrap();
        assert_eq!(load(root.path()).unwrap(), Some(session));
        for bytes in [
            b"{\"version\":99}".to_vec(),
            b"{invalid".to_vec(),
            vec![b' '; LIMIT + 1],
        ] {
            let _guard = crate::settings::FILE_LOCK.lock().unwrap();
            std::fs::write(root.path().join(NAME), &bytes).unwrap();
            drop(_guard);
            assert!(load(root.path()).is_err());
            assert!(store.state.flush().is_err());
            assert_eq!(std::fs::read(root.path().join(NAME)).unwrap(), bytes);
        }
    }
}
