use super::*;

#[test]
fn status_error_maps_http_429_to_qos_rejected() {
    let error = status_error(
        StatusCode::TOO_MANY_REQUESTS,
        std::borrow::Cow::Borrowed("request budget exhausted"),
    );

    assert_eq!(error.error_kind, ErrorKind::QosRejected);
    assert!(error.message.contains("request budget exhausted"));
}
