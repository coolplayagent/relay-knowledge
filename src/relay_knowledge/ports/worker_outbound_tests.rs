use super::*;

#[test]
fn worker_error_is_safe_to_surface_as_degradation() {
    let error = WorkerOutboundError {
        message: "request timed out".to_owned(),
    };

    assert_eq!(error.to_string(), "request timed out");
}
