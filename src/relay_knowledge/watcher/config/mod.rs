use std::time::Duration;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WatcherConfig {
    pub enabled: bool,
    pub debounce: Duration,
    pub commit_reconcile_interval: Duration,
    pub max_watch_dirs: usize,
    pub hash_cache_capacity: usize,
}

impl Default for WatcherConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            debounce: Duration::from_secs(3),
            commit_reconcile_interval: Duration::from_secs(5),
            max_watch_dirs: 1024,
            hash_cache_capacity: 4096,
        }
    }
}

impl WatcherConfig {
    pub const DEFAULT_DEBOUNCE_MS: u64 = 3000;
    pub const DEFAULT_COMMIT_RECONCILE_INTERVAL_MS: u64 = 5000;
    pub const DEFAULT_MAX_WATCH_DIRS: usize = 1024;
    pub const DEFAULT_HASH_CACHE_CAPACITY: usize = 4096;

    pub fn from_environment(overrides: &crate::env::WatcherEnvOverrides) -> Self {
        Self {
            enabled: overrides.enabled.unwrap_or(true),
            debounce: Duration::from_millis(
                overrides.debounce_ms.unwrap_or(Self::DEFAULT_DEBOUNCE_MS),
            ),
            commit_reconcile_interval: Duration::from_millis(
                overrides
                    .commit_reconcile_interval_ms
                    .unwrap_or(Self::DEFAULT_COMMIT_RECONCILE_INTERVAL_MS),
            ),
            max_watch_dirs: overrides
                .max_watch_dirs
                .unwrap_or(Self::DEFAULT_MAX_WATCH_DIRS),
            hash_cache_capacity: overrides
                .hash_cache_capacity
                .unwrap_or(Self::DEFAULT_HASH_CACHE_CAPACITY),
        }
    }
}

#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;
