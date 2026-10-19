//! Seek's Windows port of the pinned MIT fsearch-crossplatform filename core.
#![cfg(windows)]
mod engine;
pub mod helper;
pub mod index;
pub mod kernel;
pub mod live;
pub mod ntfs;
pub mod paths;
pub mod places;
pub mod qos;
pub mod query;
pub mod walk;
mod watch;
pub use engine::{Engine, Found, Options, SourceMode, Status};
pub use query::Query;
pub fn no_materialize() {}
