//! Optional measurement hooks. No observer or journal exists in normal Seek builds.
use std::{
    sync::{Arc, OnceLock},
    time::Instant,
};

#[derive(Clone, Copy)]
pub enum SeekFrameEvent {
    Milestone(&'static str, Instant),
    InputReceived(Instant),
    /// Native input/focus message id only; never records typed text or coordinates.
    Activity(u32, Instant),
    Presented(Instant),
}

pub(crate) fn milestone(name: &'static str) {
    observe(SeekFrameEvent::Milestone(name, Instant::now()));
}

static OBSERVER: OnceLock<Arc<dyn Fn(SeekFrameEvent) + Send + Sync>> = OnceLock::new();

pub fn install_seek_frame_observer(observer: impl Fn(SeekFrameEvent) + Send + Sync + 'static) {
    assert!(
        OBSERVER.set(Arc::new(observer)).is_ok(),
        "one measurement observer per process"
    );
}

pub(crate) fn observe(event: SeekFrameEvent) {
    if let Some(observer) = OBSERVER.get() {
        observer(event);
    }
}
