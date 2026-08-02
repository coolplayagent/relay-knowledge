use std::{path::Path, time::Duration};

use serde::{Deserialize, Serialize};

use super::{
    config::{UpdateRuntimeConfig, duration_millis},
    result::VersionCheckResponse,
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct VersionCheckCache {
    cache_key: String,
    response: VersionCheckResponse,
}

pub(super) async fn read_fresh_cache(
    path: &Path,
    now_ms: u64,
    interval: Duration,
    config: &UpdateRuntimeConfig,
) -> Option<VersionCheckResponse> {
    let bytes = tokio::fs::read(path).await.ok()?;
    let cache = serde_json::from_slice::<VersionCheckCache>(&bytes).ok()?;
    if cache_is_usable(&cache, now_ms, interval, config) {
        Some(cache.response)
    } else {
        None
    }
}

fn cache_is_usable(
    cache: &VersionCheckCache,
    now_ms: u64,
    interval: Duration,
    config: &UpdateRuntimeConfig,
) -> bool {
    cache.cache_key == version_cache_key(config)
        && cache.response.current_version == env!("CARGO_PKG_VERSION")
        && cache_is_fresh(&cache.response, now_ms, interval)
}

fn cache_is_fresh(response: &VersionCheckResponse, now_ms: u64, interval: Duration) -> bool {
    now_ms
        .checked_sub(response.checked_at_unix_ms)
        .is_some_and(|age| age <= duration_millis(interval))
}

pub(super) async fn write_cache(
    path: &Path,
    response: &VersionCheckResponse,
    config: &UpdateRuntimeConfig,
) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    let cache = VersionCheckCache {
        cache_key: version_cache_key(config),
        response: response.clone(),
    };
    let bytes = serde_json::to_vec(&cache)?;
    tokio::fs::write(path, bytes).await
}

fn version_cache_key(config: &UpdateRuntimeConfig) -> String {
    let sources = config
        .sources
        .iter()
        .map(|source| source.as_str())
        .collect::<Vec<_>>()
        .join(",");
    format!("sources={sources};github_repo={}", config.github_repo)
}

#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;
