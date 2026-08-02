use std::time::Duration;

use crate::{
    ports::release_metadata::{
        ReleaseMetadataError, ReleaseMetadataFuture, ReleaseMetadataPort, ReleaseMetadataRequest,
        ReleaseMetadataSession,
    },
    project::GITHUB_REPOSITORY_FULL_NAME,
};

// Direct tests for release-source aggregation.

use super::*;

struct EmptyMetadataPort;

struct EmptyMetadataSession;

impl ReleaseMetadataPort for EmptyMetadataPort {
    fn open(&self) -> Result<Box<dyn ReleaseMetadataSession>, ReleaseMetadataError> {
        Ok(Box::new(EmptyMetadataSession))
    }
}

impl ReleaseMetadataSession for EmptyMetadataSession {
    fn fetch(&self, _request: ReleaseMetadataRequest) -> ReleaseMetadataFuture<'_> {
        panic!("empty source configuration must not request metadata")
    }
}

#[tokio::test]
async fn empty_source_configuration_returns_a_bounded_no_update_response() {
    let config = UpdateRuntimeConfig {
        enabled: true,
        sources: Vec::new(),
        check_interval: Duration::from_secs(1),
        github_repo: GITHUB_REPOSITORY_FULL_NAME.to_owned(),
    };

    let response = fetch_latest_version(&EmptyMetadataPort, &config, 42).await;

    assert!(!response.update_available);
    assert_eq!(response.checked_at_unix_ms, 42);
    assert!(response.latest_version.is_none());
    assert!(response.diagnostics.is_empty());
}
