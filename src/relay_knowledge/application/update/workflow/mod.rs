use crate::{
    clock::system_now_millis_or_zero, paths::RuntimePaths,
    ports::release_metadata::ReleaseMetadataPort, project::PROJECT_NAME,
};

use super::{
    cache::{read_fresh_cache, write_cache},
    config::UpdateRuntimeConfig,
    release::fetch_latest_version,
    result::VersionCheckResponse,
};

pub async fn check_for_updates(
    paths: &RuntimePaths,
    metadata: &dyn ReleaseMetadataPort,
    config: &UpdateRuntimeConfig,
    force_refresh: bool,
) -> VersionCheckResponse {
    let now_ms = system_now_millis_or_zero();
    let cache_path = paths.version_check_cache_file();
    if !force_refresh
        && let Some(cached) =
            read_fresh_cache(&cache_path, now_ms, config.check_interval, config).await
    {
        return cached;
    }

    let response = fetch_latest_version(metadata, config, now_ms).await;
    let _ = write_cache(&cache_path, &response, config).await;
    response
}

pub async fn update_notice(
    paths: &RuntimePaths,
    metadata: &dyn ReleaseMetadataPort,
    config: &UpdateRuntimeConfig,
) -> Option<String> {
    if !config.enabled {
        return None;
    }
    let response = check_for_updates(paths, metadata, config, false).await;
    notice_from_response(response)
}

fn notice_from_response(response: VersionCheckResponse) -> Option<String> {
    if !response.update_available {
        return None;
    }

    Some(format!(
        "{} {} is available; current {}. Run `relay-knowledge version check` for details.\n",
        PROJECT_NAME,
        response
            .latest_version
            .unwrap_or_else(|| "unknown".to_owned()),
        response.current_version
    ))
}

#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;
