//! Seek's demand-driven Windows frame source. VSync only runs while work is queued.
use std::sync::{
    Arc, Condvar, Mutex,
    atomic::{AtomicBool, Ordering},
};

#[derive(Default)]
pub(crate) struct FrameScheduler {
    state: Mutex<(bool, bool)>, // wake pending, stopped
    changed: Condvar,
}

impl FrameScheduler {
    pub(crate) fn wake(&self) {
        self.state.lock().unwrap().0 = true;
        self.changed.notify_one();
    }

    /// Blocks indefinitely at idle. A request racing with wait is retained by the predicate.
    pub(crate) fn wait(&self) -> bool {
        let mut state = self.state.lock().unwrap();
        while !state.0 && !state.1 {
            state = self.changed.wait(state).unwrap();
        }
        state.0 = false;
        !state.1
    }

    pub(crate) fn stop(&self) {
        self.state.lock().unwrap().1 = true;
        self.changed.notify_one();
    }
}

pub(crate) struct FrameDemand {
    pub(crate) scheduler: Arc<FrameScheduler>,
    pending: AtomicBool,
}

impl FrameDemand {
    pub(crate) fn new(scheduler: Arc<FrameScheduler>) -> Self {
        Self {
            scheduler,
            pending: AtomicBool::new(false),
        }
    }

    pub(crate) fn request(&self) {
        // Always wake: a request can arrive after the thread consumed its wake but before
        // it consumes this window's pending flag. One harmless extra wake preserves retries.
        self.pending.store(true, Ordering::Release);
        self.scheduler.wake();
    }

    pub(crate) fn take(&self) -> bool {
        self.pending.swap(false, Ordering::AcqRel)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pre_wait_requests_coalesce_and_stop_wakes_waiter() {
        let scheduler = Arc::new(FrameScheduler::default());
        let a = FrameDemand::new(scheduler.clone());
        let b = FrameDemand::new(scheduler.clone());
        a.request();
        a.request();
        assert!(scheduler.wait());
        assert!(a.take());
        assert!(!a.take());
        assert!(!b.take());
        let worker = std::thread::spawn(move || scheduler.wait());
        a.scheduler.stop();
        assert!(!worker.join().unwrap());
    }

    #[test]
    fn requests_at_wait_boundaries_are_not_lost() {
        let scheduler = Arc::new(FrameScheduler::default());
        let demand = Arc::new(FrameDemand::new(scheduler.clone()));
        let worker_demand = demand.clone();
        let (tx, rx) = std::sync::mpsc::channel();
        let worker = std::thread::spawn(move || {
            while worker_demand.scheduler.wait() {
                if worker_demand.take() {
                    tx.send(()).unwrap();
                }
            }
        });
        for _ in 0..1000 {
            demand.request();
            rx.recv_timeout(std::time::Duration::from_secs(2))
                .expect("lost frame wake");
        }
        scheduler.stop();
        worker.join().unwrap();
    }
}
