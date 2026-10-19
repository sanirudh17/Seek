//! Opt-in benchmark diagnostics; disk writes run on their own blocking worker.
#[cfg(feature = "benchmarks")]
mod enabled {
    use gpui_windows::seek_observer::{SeekFrameEvent, install_seek_frame_observer};
    use std::{
        io::Write,
        path::PathBuf,
        sync::{Mutex, OnceLock, mpsc},
        time::{Instant, SystemTime, UNIX_EPOCH},
    };
    struct Pending {
        record: serde_json::Value,
        started: Option<Instant>,
    }
    struct State {
        boot: Instant,
        tx: mpsc::Sender<serde_json::Value>,
        input: Option<Instant>,
        pending: Option<Pending>,
        trace_frames: bool,
        profile_ui: bool,
        spans: std::collections::BTreeMap<&'static str, (u64, u32)>,
    }
    static STATE: OnceLock<Mutex<State>> = OnceLock::new();
    static PROFILE_UI: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    pub fn init(boot: Instant) {
        let args: Vec<_> = std::env::args().collect();
        let Some(path) = args
            .windows(2)
            .find(|w| w[0] == "--metrics")
            .map(|w| PathBuf::from(&w[1]))
        else {
            return;
        };
        PROFILE_UI.store(
            args.iter().any(|a| a == "--profile-ui"),
            std::sync::atomic::Ordering::Relaxed,
        );
        let (tx, rx) = mpsc::channel::<serde_json::Value>();
        // Open before UI construction, so an invalid benchmark output fails clearly.
        // Whole-drive measurements cannot write a trace onto the indexed drive:
        // the trace would itself create filesystem changes and redraws.
        let file = if path.to_string_lossy().starts_with(r"\\.\pipe\") {
            std::fs::OpenOptions::new().write(true).open(path)
        } else {
            std::fs::File::create(path)
        }
        .expect("open benchmark output");
        std::thread::Builder::new()
            .name("seek-metrics".into())
            .spawn(move || {
                let mut writer = std::io::BufWriter::new(file);
                for record in rx {
                    serde_json::to_writer(&mut writer, &record).unwrap();
                    writeln!(writer).unwrap();
                    writer.flush().unwrap();
                }
            })
            .unwrap();
        assert!(
            STATE
                .set(Mutex::new(State {
                    boot,
                    tx,
                    input: None,
                    pending: None,
                    trace_frames: args.iter().any(|a| a == "--trace-frames"),
                    profile_ui: args.iter().any(|a| a == "--profile-ui"),
                    spans: Default::default(),
                }))
                .is_ok()
        );
        install_seek_frame_observer(|event| {
            let mut state = STATE.get().unwrap().lock().unwrap();
            match event {
                SeekFrameEvent::Milestone(name, at) => {
                    let _ = state.tx.send(serde_json::json!({
                        "phase": name,
                        "entry_ms": at.duration_since(state.boot).as_secs_f64() * 1000.,
                    }));
                }
                SeekFrameEvent::InputReceived(at) => state.input = Some(at),
                SeekFrameEvent::Activity(message, at) => {
                    if state.trace_frames {
                        let _ = state.tx.send(serde_json::json!({
                            "native_activity": message,
                            "entry_ms": at.duration_since(state.boot).as_secs_f64() * 1000.,
                        }));
                    }
                }
                SeekFrameEvent::Presented(at) => {
                    if state.trace_frames {
                        let _ = state.tx.send(serde_json::json!({
                            "frame_entry_ms": at.duration_since(state.boot).as_secs_f64() * 1000.,
                        }));
                    }
                    let spans = std::mem::take(&mut state.spans);
                    if state.profile_ui {
                        let _=state.tx.send(serde_json::json!({"ui_profile":spans,"frame_entry_ms":at.duration_since(state.boot).as_secs_f64()*1000.}));
                    }
                    if let Some(mut pending) = state.pending.take() {
                        pending.record["entry_to_present_ms"] =
                            (at.duration_since(state.boot).as_secs_f64() * 1000.).into();
                        pending.record["presented_unix_ms"] = (SystemTime::now()
                            .duration_since(UNIX_EPOCH)
                            .unwrap()
                            .as_secs_f64()
                            * 1000.)
                            .into();
                        if let Some(started) = pending.started {
                            pending.record["query_to_present_ms"] =
                                (at.duration_since(started).as_secs_f64() * 1000.).into();
                        }
                        let _ = state.tx.send(pending.record);
                    }
                }
            }
        });
    }
    pub struct Span {
        name: &'static str,
        start: Option<Instant>,
    }
    pub fn span(name: &'static str) -> Span {
        Span {
            name,
            start: PROFILE_UI
                .load(std::sync::atomic::Ordering::Relaxed)
                .then(Instant::now),
        }
    }
    impl Drop for Span {
        fn drop(&mut self) {
            if let Some(start) = self.start
                && let Some(state) = STATE.get()
            {
                let mut state = state.lock().unwrap();
                let entry = state.spans.entry(self.name).or_default();
                entry.0 += start.elapsed().as_nanos() as u64;
                entry.1 += 1;
            }
        }
    }
    pub fn phase(name: &'static str) {
        if let Some(state) = STATE.get() {
            let state = state.lock().unwrap();
            let _ = state.tx.send(serde_json::json!({
                "phase": name, "entry_ms": state.boot.elapsed().as_secs_f64() * 1000.,
            }));
        }
    }
    pub fn interaction(name: &str, detail: serde_json::Value) {
        if let Some(state) = STATE.get() {
            let state = state.lock().unwrap();
            let _ = state.tx.send(serde_json::json!({
                "interaction": name, "detail": detail,
                "entry_ms": state.boot.elapsed().as_secs_f64() * 1000.,
            }));
        }
    }
    pub fn input_start() -> (Instant, bool) {
        let input = STATE.get().and_then(|s| s.lock().unwrap().input.take());
        (input.unwrap_or_else(Instant::now), input.is_some())
    }
    pub fn applied(
        generation: u64,
        started: Option<Instant>,
        native_input: bool,
        query: &str,
        rows: &[crate::backend::Row],
        status: &seek_engine::Status,
        accessibility_active: bool,
    ) {
        if let Some(state) = STATE.get() {
            let mut state = state.lock().unwrap();
            state.pending = Some(Pending {
                record: serde_json::json!({ "generation": generation, "query": query, "native_input": native_input && started.is_some(),
                    "accessibility_active":accessibility_active, "applied_entry_ms": state.boot.elapsed().as_secs_f64() * 1000.,
                    "rows": rows.iter().map(|r| &r.name).collect::<Vec<_>>(), "status": status }),
                started,
            });
        }
    }
}
#[cfg(feature = "benchmarks")]
pub use enabled::*;
#[cfg(not(feature = "benchmarks"))]
pub fn init(_: std::time::Instant) {}
#[cfg(not(feature = "benchmarks"))]
pub fn phase(_: &'static str) {}
#[cfg(not(feature = "benchmarks"))]
pub fn interaction(_: &str, _: serde_json::Value) {}
#[cfg(not(feature = "benchmarks"))]
pub fn input_start() -> (std::time::Instant, bool) {
    (std::time::Instant::now(), false)
}
#[cfg(not(feature = "benchmarks"))]
pub fn applied(
    _: u64,
    _: Option<std::time::Instant>,
    _: bool,
    _: &str,
    _: &[crate::backend::Row],
    _: &seek_engine::Status,
    _: bool,
) {
}

#[cfg(not(feature = "benchmarks"))]
pub struct Span;
#[cfg(not(feature = "benchmarks"))]
pub fn span(_: &'static str) -> Span {
    Span
}
