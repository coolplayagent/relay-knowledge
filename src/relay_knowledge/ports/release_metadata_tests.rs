use super::*;

#[test]
fn error_kind_exposes_stable_diagnostic_code_and_message() {
    let error = ReleaseMetadataError {
        kind: ReleaseMetadataErrorKind::NetworkTimeout,
        message: "release request timed out".to_owned(),
        retryable: true,
    };

    assert_eq!(error.kind.diagnostic_code(), "network_timeout");
    assert_eq!(error.to_string(), "release request timed out");
    assert!(error.retryable);
}
