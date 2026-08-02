use crate::ports::release_metadata::{
    ReleaseMetadataError, ReleaseMetadataErrorKind, ReleaseMetadataFuture,
};

use super::*;

struct StubSession {
    result: Result<Vec<u8>, ReleaseMetadataError>,
}

impl ReleaseMetadataSession for StubSession {
    fn fetch(&self, _request: ReleaseMetadataRequest) -> ReleaseMetadataFuture<'_> {
        Box::pin(std::future::ready(self.result.clone()))
    }
}

#[tokio::test]
async fn fetch_json_maps_payloads_and_port_failures() {
    let payload: serde_json::Value = fetch_json(
        &StubSession {
            result: Ok(br#"{"version":"1.2.3"}"#.to_vec()),
        },
        "https://release.example/latest".to_owned(),
        UpdateSource::Github,
    )
    .await
    .expect("valid metadata should decode");
    let diagnostic = fetch_json::<serde_json::Value>(
        &StubSession {
            result: Err(ReleaseMetadataError {
                kind: ReleaseMetadataErrorKind::NetworkTimeout,
                message: "request timed out".to_owned(),
                retryable: true,
            }),
        },
        "https://release.example/latest".to_owned(),
        UpdateSource::Github,
    )
    .await
    .expect_err("port failure should become a source diagnostic");

    assert_eq!(payload["version"], "1.2.3");
    assert_eq!(diagnostic.code, "network_timeout");
    assert!(diagnostic.retryable);
}
