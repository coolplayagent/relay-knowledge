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
