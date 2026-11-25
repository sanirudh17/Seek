//! Cooperative cancellation for queued/native file work; no polling thread.
use std::{
    cell::RefCell,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};
pub type Token = Arc<AtomicBool>;
thread_local! { static CURRENT: RefCell<Option<Token>> = const { RefCell::new(None) }; }
pub struct Scope(Option<Token>);
pub fn enter(token: Token) -> Scope {
    Scope(CURRENT.with(|current| current.replace(Some(token))))
}
impl Drop for Scope {
    fn drop(&mut self) {
        CURRENT.with(|current| current.replace(self.0.take()));
    }
}
pub fn cancelled() -> bool {
    CURRENT.with(|current| {
        current
            .borrow()
            .as_ref()
            .is_some_and(|token| token.load(Ordering::Relaxed))
    })
}
pub fn check() -> Result<(), String> {
    if cancelled() {
        Err("File operation cancelled.".into())
    } else {
        Ok(())
    }
}
