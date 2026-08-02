use serde::Deserialize;

use crate::{ports::release_metadata::ReleaseMetadataSession, project::PROJECT_NAME};

use super::super::{
    candidate::ReleaseCandidate, config::UpdateSource, diagnostics::diagnostic,
    result::VersionCheckDiagnostic, version::stable_version,
};
use super::metadata::fetch_json;

pub(in crate::application::update) async fn fetch_crates_release(
    session: &dyn ReleaseMetadataSession,
) -> Result<ReleaseCandidate, VersionCheckDiagnostic> {
    let url = format!("https://crates.io/api/v1/crates/{PROJECT_NAME}");
    let response = fetch_json(session, url, UpdateSource::CratesIo).await?;
    crates_candidate(response)
}

#[derive(Debug, Deserialize)]
struct CratesPackageResponse {
    #[serde(rename = "crate")]
    package: CratesPackage,
}

#[derive(Debug, Deserialize)]
struct CratesPackage {
    max_stable_version: Option<String>,
}

fn crates_candidate(
    response: CratesPackageResponse,
) -> Result<ReleaseCandidate, VersionCheckDiagnostic> {
    let Some(max_stable_version) = response.package.max_stable_version else {
        return Err(diagnostic(
            Some(UpdateSource::CratesIo),
            "stable_version_unavailable",
            "crates.io response did not include a stable release version",
            false,
        ));
    };
    let version = stable_version(&max_stable_version).map_err(|message| {
        diagnostic(
            Some(UpdateSource::CratesIo),
            "invalid_version",
            message,
            false,
        )
    })?;

    Ok(ReleaseCandidate {
        source: UpdateSource::CratesIo,
        version,
        release_url: format!("https://crates.io/crates/{PROJECT_NAME}"),
    })
}

#[cfg(test)]
#[path = "crates_io_tests.rs"]
mod tests;
