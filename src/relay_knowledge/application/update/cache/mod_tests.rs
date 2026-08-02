use std::time::Duration;

use crate::{env::UpdateEnvOverrides, project::PROJECT_NAME};

// Direct tests for the version-check cache owner.

use super::*;

#[test]
fn cache_freshness_includes_the_interval_boundary() {
    let response = sample_version_response(env!("CARGO_PKG_VERSION"), 100);

    assert!(cache_is_fresh(&response, 200, Duration::from_millis(100)));
    assert!(!cache_is_fresh(&response, 201, Duration::from_millis(100)));
}

#[test]
fn cache_usability_requires_current_binary_and_source_configuration() {
    let config = UpdateRuntimeConfig::from_environment(&UpdateEnvOverrides::default())
        .expect("default config should parse");
    let cache = VersionCheckCache {
        cache_key: version_cache_key(&config),
        response: sample_version_response(env!("CARGO_PKG_VERSION"), 100),
    };

    assert!(cache_is_usable(
        &cache,
        200,
        Duration::from_millis(100),
        &config
    ));

    let mut previous_binary_cache = cache.clone();
    previous_binary_cache.response.current_version = "0.0.1".to_owned();
    assert!(!cache_is_usable(
        &previous_binary_cache,
        200,
        Duration::from_millis(100),
        &config
    ));

    let mut changed_source_cache = cache;
    changed_source_cache.cache_key = "sources=crates.io;github_repo=example/repo".to_owned();
    assert!(!cache_is_usable(
        &changed_source_cache,
        200,
        Duration::from_millis(100),
        &config
    ));
}

#[test]
fn cache_format_requires_the_configuration_key_wrapper() {
    let raw_response = serde_json::to_vec(&sample_version_response(env!("CARGO_PKG_VERSION"), 100))
        .expect("sample response should serialize");

    assert!(serde_json::from_slice::<VersionCheckCache>(&raw_response).is_err());
}

fn sample_version_response(current_version: &str, checked_at_unix_ms: u64) -> VersionCheckResponse {
    VersionCheckResponse {
        project_name: PROJECT_NAME.to_owned(),
        current_version: current_version.to_owned(),
        latest_version: Some("1.0.5".to_owned()),
        update_available: true,
        source: Some("github".to_owned()),
        release_url: None,
        checked_at_unix_ms,
        diagnostics: Vec::new(),
    }
}
