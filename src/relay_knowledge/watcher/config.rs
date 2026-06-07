use std::time::Duration;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WatcherConfig {
    pub enabled: bool,
    pub debounce: Duration,
    pub max_watch_dirs: usize,
    pub hash_cache_capacity: usize,
}

impl Default for WatcherConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            debounce: Duration::from_secs(3),
            max_watch_dirs: 1024,
            hash_cache_capacity: 4096,
        }
    }
}

impl WatcherConfig {
    pub const DEFAULT_DEBOUNCE_MS: u64 = 3000;
    pub const DEFAULT_MAX_WATCH_DIRS: usize = 1024;
    pub const DEFAULT_HASH_CACHE_CAPACITY: usize = 4096;

    pub fn from_environment(overrides: &crate::env::WatcherEnvOverrides) -> Self {
        Self {
            enabled: overrides.enabled.unwrap_or(true),
            debounce: Duration::from_millis(
                overrides.debounce_ms.unwrap_or(Self::DEFAULT_DEBOUNCE_MS),
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
mod tests {
    use super::*;

    #[test]
    fn default_config_has_sensible_defaults() {
        let config = WatcherConfig::default();
        assert!(config.enabled);
        assert_eq!(config.debounce, Duration::from_secs(3));
        assert_eq!(config.max_watch_dirs, 1024);
        assert_eq!(config.hash_cache_capacity, 4096);
    }

    #[test]
    fn from_environment_applies_overrides() {
        let overrides = crate::env::WatcherEnvOverrides {
            enabled: Some(false),
            debounce_ms: Some(5000),
            max_watch_dirs: Some(2048),
            hash_cache_capacity: Some(8192),
        };
        let config = WatcherConfig::from_environment(&overrides);
        assert!(!config.enabled);
        assert_eq!(config.debounce, Duration::from_millis(5000));
        assert_eq!(config.max_watch_dirs, 2048);
        assert_eq!(config.hash_cache_capacity, 8192);
    }

    #[test]
    fn from_environment_uses_defaults_when_no_overrides() {
        let overrides = crate::env::WatcherEnvOverrides::default();
        let config = WatcherConfig::from_environment(&overrides);
        assert!(config.enabled);
        assert_eq!(config.debounce, Duration::from_secs(3));
        assert_eq!(config.max_watch_dirs, 1024);
        assert_eq!(config.hash_cache_capacity, 4096);
    }
}
