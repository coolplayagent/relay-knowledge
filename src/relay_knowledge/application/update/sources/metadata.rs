use serde::de::DeserializeOwned;

use crate::ports::release_metadata::{ReleaseMetadataRequest, ReleaseMetadataSession};

use super::super::{
    config::UpdateSource,
    diagnostics::{diagnostic, release_metadata_diagnostic},
    result::VersionCheckDiagnostic,
};

pub(super) async fn fetch_json<T>(
    session: &dyn ReleaseMetadataSession,
    url: String,
    source: UpdateSource,
) -> Result<T, VersionCheckDiagnostic>
where
    T: DeserializeOwned,
{
    let body = session
        .fetch(ReleaseMetadataRequest { url })
        .await
        .map_err(|error| release_metadata_diagnostic(Some(source), error))?;

    serde_json::from_slice(&body).map_err(|error| {
        diagnostic(
            Some(source),
            "invalid_response_json",
            error.to_string(),
            false,
        )
    })
}

#[cfg(test)]
#[path = "metadata_tests.rs"]
mod tests;
