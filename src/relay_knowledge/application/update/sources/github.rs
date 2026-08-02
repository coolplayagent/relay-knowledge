use serde::Deserialize;

use crate::ports::release_metadata::ReleaseMetadataSession;

use super::super::{
    candidate::ReleaseCandidate, config::UpdateSource, diagnostics::diagnostic,
    result::VersionCheckDiagnostic, version::stable_version,
};
use super::metadata::fetch_json;

pub(in crate::application::update) async fn fetch_github_release(
    session: &dyn ReleaseMetadataSession,
    repo: &str,
) -> Result<ReleaseCandidate, VersionCheckDiagnostic> {
    let url = format!("https://api.github.com/repos/{repo}/releases/latest");
    let payload = fetch_json(session, url, UpdateSource::Github).await?;
    github_candidate(payload)
}

#[derive(Debug, Deserialize)]
struct GithubLatestRelease {
    tag_name: String,
    html_url: String,
    prerelease: bool,
}

fn github_candidate(
    release: GithubLatestRelease,
) -> Result<ReleaseCandidate, VersionCheckDiagnostic> {
    if release.prerelease {
        return Err(diagnostic(
            Some(UpdateSource::Github),
            "prerelease_ignored",
            format!("GitHub release '{}' is a prerelease", release.tag_name),
            false,
        ));
    }
    let version = stable_version(&release.tag_name).map_err(|message| {
        diagnostic(
            Some(UpdateSource::Github),
            "invalid_version",
            message,
            false,
        )
    })?;

    Ok(ReleaseCandidate {
        source: UpdateSource::Github,
        version,
        release_url: release.html_url,
    })
}

#[cfg(test)]
#[path = "github_tests.rs"]
mod tests;
