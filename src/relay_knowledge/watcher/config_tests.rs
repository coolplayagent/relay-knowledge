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
