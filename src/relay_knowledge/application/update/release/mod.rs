use crate::ports::release_metadata::{ReleaseMetadataPort, ReleaseMetadataSession};

use super::{
    candidate::{ReleaseCandidate, response_from_candidates},
    config::{UpdateRuntimeConfig, UpdateSource},
    diagnostics::release_metadata_diagnostic,
    result::{VersionCheckDiagnostic, VersionCheckResponse},
    sources::{fetch_crates_release, fetch_github_release},
    version::current_version,
};

pub(super) async fn fetch_latest_version(
    metadata: &dyn ReleaseMetadataPort,
    config: &UpdateRuntimeConfig,
    checked_at_unix_ms: u64,
) -> VersionCheckResponse {
    let current_version = current_version();
    let session = match metadata.open() {
        Ok(session) => session,
        Err(error) => {
            return response_from_candidates(
                current_version,
                Vec::new(),
                vec![release_metadata_diagnostic(None, error)],
                checked_at_unix_ms,
            );
        }
    };

    let mut candidates = Vec::new();
    let mut diagnostics = Vec::new();
    for source in &config.sources {
        let result = fetch_source(session.as_ref(), config, *source).await;
        match result {
            Ok(candidate) => candidates.push(candidate),
            Err(diagnostic) => diagnostics.push(diagnostic),
        }
    }

    response_from_candidates(current_version, candidates, diagnostics, checked_at_unix_ms)
}

async fn fetch_source(
    session: &dyn ReleaseMetadataSession,
    config: &UpdateRuntimeConfig,
    source: UpdateSource,
) -> Result<ReleaseCandidate, VersionCheckDiagnostic> {
    match source {
        UpdateSource::Github => fetch_github_release(session, &config.github_repo).await,
        UpdateSource::CratesIo => fetch_crates_release(session).await,
    }
}

#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;
