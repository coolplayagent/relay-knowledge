use std::{error::Error, fmt, future::Future, pin::Pin};

/// One release-metadata endpoint requested by the update workflow.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReleaseMetadataRequest {
    pub url: String,
}

/// Stable failure categories exposed by release-metadata adapters.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReleaseMetadataErrorKind {
    ClientBuild,
    NetworkTimeout,
    Network,
    Transport,
    HttpStatus,
    ResponseTooLarge,
}

impl ReleaseMetadataErrorKind {
    /// Returns the low-cardinality diagnostic code used by application responses.
    pub const fn diagnostic_code(self) -> &'static str {
        match self {
            Self::ClientBuild => "client_build_failed",
            Self::NetworkTimeout => "network_timeout",
            Self::Network => "network_error",
            Self::Transport => "transport_failed",
            Self::HttpStatus => "http_status",
            Self::ResponseTooLarge => "response_body_too_large",
        }
    }
}

/// Adapter-neutral release-metadata failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReleaseMetadataError {
    pub kind: ReleaseMetadataErrorKind,
    pub message: String,
    pub retryable: bool,
}

impl fmt::Display for ReleaseMetadataError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}", self.message)
    }
}

impl Error for ReleaseMetadataError {}

/// Future returned by one bounded metadata request.
pub type ReleaseMetadataFuture<'a> =
    Pin<Box<dyn Future<Output = Result<Vec<u8>, ReleaseMetadataError>> + Send + 'a>>;

/// Prepared release-metadata session that may reuse one client across sources.
pub trait ReleaseMetadataSession: Send + Sync {
    fn fetch(&self, request: ReleaseMetadataRequest) -> ReleaseMetadataFuture<'_>;
}

/// Port that prepares a release-metadata session from current runtime policy.
pub trait ReleaseMetadataPort: Send + Sync {
    fn open(&self) -> Result<Box<dyn ReleaseMetadataSession>, ReleaseMetadataError>;
}

#[cfg(test)]
#[path = "release_metadata_tests.rs"]
mod tests;
