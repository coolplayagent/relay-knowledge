use super::{UpdateSource, VersionCheckDiagnostic, diagnostic};
use crate::net::http;

pub(super) fn qos_transport_diagnostic(
    source: UpdateSource,
    error: http::QosHttpClientError,
) -> VersionCheckDiagnostic {
    diagnostic(
        Some(source),
        if error.is_timeout() {
            "network_timeout"
        } else {
            "network_error"
        },
        error.to_string(),
        true,
    )
}
