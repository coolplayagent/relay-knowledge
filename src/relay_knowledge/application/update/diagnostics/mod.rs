use crate::ports::release_metadata::ReleaseMetadataError;

use super::{config::UpdateSource, result::VersionCheckDiagnostic};

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

pub(super) fn release_metadata_diagnostic(
    source: Option<UpdateSource>,
    error: ReleaseMetadataError,
) -> VersionCheckDiagnostic {
    diagnostic(
        source,
        error.kind.diagnostic_code(),
        error.message,
        error.retryable,
    )
}

#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;
