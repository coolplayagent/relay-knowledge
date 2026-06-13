use reqwest::StatusCode;

use super::{UpdateSource, VersionCheckDiagnostic};

pub(super) fn diagnostic(
    source: Option<UpdateSource>,
    code: impl Into<String>,
    message: impl Into<String>,
    retryable: bool,
) -> VersionCheckDiagnostic {
    VersionCheckDiagnostic {
        source: source.map(|value| value.as_str().to_owned()),
        code: code.into(),
        message: message.into(),
        retryable,
    }
}

pub(super) fn transport_diagnostic(
    source: UpdateSource,
    error: reqwest::Error,
) -> VersionCheckDiagnostic {
    diagnostic(Some(source), "transport_failed", error.to_string(), true)
}

pub(super) fn status_diagnostic(
    source: UpdateSource,
    status: StatusCode,
) -> VersionCheckDiagnostic {
    diagnostic(
        Some(source),
        "http_status",
        format!("release metadata request returned HTTP {}", status.as_u16()),
        status.is_server_error()
            || status == StatusCode::REQUEST_TIMEOUT
            || status == StatusCode::TOO_MANY_REQUESTS,
    )
}

pub(super) fn response_body_too_large_diagnostic(
    source: UpdateSource,
    max_response_bytes: u64,
) -> VersionCheckDiagnostic {
    diagnostic(
        Some(source),
        "response_body_too_large",
        format!("release metadata response exceeded {max_response_bytes} bytes"),
        false,
    )
}
