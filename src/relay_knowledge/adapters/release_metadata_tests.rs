use super::*;

#[test]
fn bounded_body_accepts_boundary_and_rejects_overflow() {
    let mut body = b"{}".to_vec();

    append_bounded_body(&mut body, b"\n", 3).expect("boundary-sized body should pass");
    let error = append_bounded_body(&mut body, b"x", 3).expect_err("body over limit should fail");

    assert_eq!(error.kind, ReleaseMetadataErrorKind::ResponseTooLarge);
    assert_eq!(error.message, "release metadata response exceeded 3 bytes");
    assert!(!error.retryable);
}

#[test]
fn retryable_http_statuses_are_distinguished_from_client_errors() {
    let retryable = validate_status(StatusCode::TOO_MANY_REQUESTS)
        .expect_err("rate limiting should be reported");
    let permanent =
        validate_status(StatusCode::NOT_FOUND).expect_err("missing release should be reported");

    assert!(retryable.retryable);
    assert!(!permanent.retryable);
}
