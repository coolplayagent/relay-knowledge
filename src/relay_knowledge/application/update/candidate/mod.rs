use crate::project::PROJECT_NAME;

use super::{
    config::UpdateSource,
    result::{VersionCheckDiagnostic, VersionCheckResponse},
    version::StableVersion,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ReleaseCandidate {
    pub(super) source: UpdateSource,
    pub(super) version: StableVersion,
    pub(super) release_url: String,
}

pub(super) fn response_from_candidates(
    current_version: StableVersion,
    candidates: Vec<ReleaseCandidate>,
    diagnostics: Vec<VersionCheckDiagnostic>,
    checked_at_unix_ms: u64,
) -> VersionCheckResponse {
    let latest = candidates
        .into_iter()
        .max_by(|left, right| left.version.cmp(&right.version));
    let update_available = latest
        .as_ref()
        .is_some_and(|candidate| candidate.version > current_version);

    VersionCheckResponse {
        project_name: PROJECT_NAME.to_owned(),
        current_version: env!("CARGO_PKG_VERSION").to_owned(),
        latest_version: latest
            .as_ref()
            .map(|candidate| candidate.version.to_string()),
        update_available,
        source: latest
            .as_ref()
            .map(|candidate| candidate.source.as_str().to_owned()),
        release_url: latest
            .as_ref()
            .map(|candidate| candidate.release_url.clone()),
        checked_at_unix_ms,
        diagnostics,
    }
}

#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;
