mod config;
mod engine;
mod event_filter;
mod hash_cache;

pub use config::WatcherConfig;
pub use engine::{
    FileWatcher, WatcherDiagnostics, WatcherHandle, WatcherState, build_incremental_task_seed,
};
pub use event_filter::WatcherEventFilter;
pub use hash_cache::ContentHashCache;

use std::path::PathBuf;

#[derive(Debug)]
pub struct WatchedRepository {
    pub repository_id: String,
    pub alias: String,
    pub root: PathBuf,
    pub path_filters: Vec<String>,
    pub language_filters: Vec<String>,
    pub source_scope: String,
}
