use axum::http::{HeaderMap, HeaderValue};

use super::*;

#[test]
fn protocol_version_header_distinguishes_optional_missing_and_invalid_values() {
    let mut headers = HeaderMap::new();

    assert_eq!(validate_protocol_version_header(&headers, false), Ok(()));
    assert_eq!(
        validate_protocol_version_header(&headers, true),
        Err(StatusCode::BAD_REQUEST)
    );
    headers.insert(
        MCP_PROTOCOL_VERSION_HEADER,
        HeaderValue::from_static(MCP_PROTOCOL_VERSION),
    );
    assert_eq!(validate_protocol_version_header(&headers, true), Ok(()));
}

#[test]
fn exact_accept_rejection_overrides_a_positive_wildcard() {
    let ranges = [
        AcceptRange::parse("*/*; q=1").expect("wildcard range"),
        AcceptRange::parse("application/json; q=0").expect("exact range"),
        AcceptRange::parse("text/event-stream").expect("event range"),
    ];

    assert!(!accepts_media_type(&ranges, "application", "json"));
    assert!(accepts_media_type(&ranges, "text", "event-stream"));
    assert!(is_loopback_origin("http://[::1]:8791/mcp"));
    assert!(!is_loopback_origin("https://example.com/mcp"));
}
