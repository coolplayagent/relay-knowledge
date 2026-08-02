// Direct tests for update diagnostics.

use super::*;

#[test]
fn response_limit_diagnostic_keeps_source_and_non_retryable_contract() {
    let diagnostic = release_metadata_diagnostic(
        Some(UpdateSource::Github),
        ReleaseMetadataError {
            kind: crate::ports::release_metadata::ReleaseMetadataErrorKind::ResponseTooLarge,
            message: "release metadata response exceeded 4096 bytes".to_owned(),
            retryable: false,
        },
    );

    assert_eq!(diagnostic.source.as_deref(), Some("github"));
    assert_eq!(diagnostic.code, "response_body_too_large");
    assert!(diagnostic.message.contains("4096 bytes"));
    assert!(!diagnostic.retryable);
}
