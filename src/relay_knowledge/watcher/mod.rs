mod config;
mod engine;
mod event_filter;
mod hash_cache;
mod task_seed;

pub use config::WatcherConfig;
pub use engine::{FileWatcher, WatcherDiagnostics, WatcherHandle, WatcherState};
pub use event_filter::WatcherEventFilter;
pub use hash_cache::ContentHashCache;
pub use task_seed::build_incremental_task_seed;

use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WatchedRepository {
    pub repository_id: String,
    pub alias: String,
    pub root: PathBuf,
    pub path_filters: Vec<String>,
    pub language_filters: Vec<String>,
    pub source_scope: String,
}
