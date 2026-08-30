use std::{error::Error, fmt};

use crate::api::ApiError;

use super::super::OutputFormat;

#[cfg(test)]
#[path = "diagnostics_tests.rs"]
mod tests;

/// CLI adapter error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CliError {
    Diagnostic(Box<CliDiagnostic>),
    InvalidFormat(String),
    InvalidCodeQueryKind(String),
    InvalidSoftwareKind(String),
    InvalidFreshness(String),
    InvalidIndexKind(String),
    InvalidMapSourceKind(String),
    InvalidWorkerKind(String),
    InvalidProposalState(String),
    InvalidServiceAction(String),
    InvalidLimit(String),
    MissingFormatValue,
    MissingValue(&'static str),
    UnsupportedVersionFormat(OutputFormat),
    UnknownHelpTopic(String),
    UnexpectedArgument(String),
    RuntimeConfigFailed(String),
    ApiFailed(String),
    ApiError {
        error: Box<ApiError>,
        format: OutputFormat,
    },
    ServiceRunFailed(String),
    RenderFailed(String),
}

impl CliError {
    pub(crate) fn api_failed(error: ApiError, format: OutputFormat) -> Self {
        Self::ApiError {
            error: Box::new(error),
            format,
        }
    }

    pub(crate) fn invalid_api_argument(message: impl Into<String>, format: OutputFormat) -> Self {
        Self::api_failed(ApiError::invalid_argument(message), format)
    }

    /// Returns the process exit code for the error.
    pub fn exit_code(&self) -> i32 {
        match self {
            Self::Diagnostic(_)
            | Self::InvalidFormat(_)
            | Self::InvalidCodeQueryKind(_)
            | Self::InvalidSoftwareKind(_)
            | Self::InvalidFreshness(_)
            | Self::InvalidIndexKind(_)
            | Self::InvalidMapSourceKind(_)
            | Self::InvalidWorkerKind(_)
            | Self::InvalidProposalState(_)
            | Self::InvalidServiceAction(_)
            | Self::InvalidLimit(_)
            | Self::MissingFormatValue
            | Self::MissingValue(_)
            | Self::UnsupportedVersionFormat(_)
            | Self::UnknownHelpTopic(_)
            | Self::UnexpectedArgument(_) => 2,
            Self::RuntimeConfigFailed(_)
            | Self::ApiFailed(_)
            | Self::ApiError { .. }
            | Self::ServiceRunFailed(_)
            | Self::RenderFailed(_) => 1,
        }
    }

    /// Renders the process stderr payload for this error.
    pub fn render_stderr(&self) -> String {
        match self {
            Self::Diagnostic(diagnostic) => diagnostic.render_stderr(),
            Self::ApiError { error, format } if format.is_machine_readable() => {
                serde_json::to_string(error).unwrap_or_else(|_| error.message.clone())
            }
            _ => self.to_string(),
        }
    }
}

impl fmt::Display for CliError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Diagnostic(diagnostic) => write!(formatter, "{}", diagnostic.render_text()),
            Self::InvalidFormat(format) => write!(
                formatter,
                "invalid --format value '{format}', expected text, json, markdown, or streaming-json"
            ),
            Self::InvalidCodeQueryKind(value) => write!(
                formatter,
                "invalid --kind value '{value}', expected hybrid, symbol, definition, references, callers, callees, imports, or sbom"
            ),
            Self::InvalidSoftwareKind(value) => write!(
                formatter,
                "invalid --kind value '{value}', expected dependencies, sdks, files, topics, relationships, build, iac, design, systems, apis, resources, tests, deployments, releases, statements, conflicts, or all"
            ),
            Self::InvalidFreshness(value) => write!(
                formatter,
                "invalid --freshness value '{value}', expected allow-stale, wait-until-fresh, or graph-only"
            ),
            Self::InvalidIndexKind(value) => write!(
                formatter,
                "invalid --kind value '{value}', expected bm25, semantic, or vector"
            ),
            Self::InvalidMapSourceKind(value) => write!(
                formatter,
                "invalid --kind value '{value}', expected repo, file, doc, config, db, ci, runtime, wiki, or monitoring"
            ),
            Self::InvalidWorkerKind(value) => write!(
                formatter,
                "invalid worker kind '{value}', expected embedding, ocr, vision, or extractor"
            ),
            Self::InvalidProposalState(value) => write!(
                formatter,
                "invalid proposal state '{value}', expected proposed, accepted, rejected, or superseded"
            ),
            Self::InvalidServiceAction(value) => write!(
                formatter,
                "invalid service action '{value}', expected install, upgrade, rollback, or uninstall"
            ),
            Self::InvalidLimit(value) => write!(formatter, "invalid --limit value '{value}'"),
            Self::MissingFormatValue => write!(formatter, "missing value for --format"),
            Self::MissingValue(flag) => write!(formatter, "missing value for {flag}"),
            Self::UnsupportedVersionFormat(format) => {
                write!(
                    formatter,
                    "version does not support --format {}",
                    format.as_str()
                )
            }
            Self::UnknownHelpTopic(topic) => write!(formatter, "unknown help topic '{topic}'"),
            Self::UnexpectedArgument(argument) => {
                write!(formatter, "unexpected argument '{argument}'")
            }
            Self::RuntimeConfigFailed(message) => {
                write!(formatter, "failed to load runtime configuration: {message}")
            }
            Self::ApiFailed(message) => write!(formatter, "{message}"),
            Self::ApiError { error, .. } => write!(formatter, "{}", error.message),
            Self::ServiceRunFailed(message) => write!(formatter, "{message}"),
            Self::RenderFailed(message) => write!(formatter, "failed to render output: {message}"),
        }
    }
}

impl Error for CliError {}

/// Structured parse diagnostic produced from the CLI grammar.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CliDiagnostic {
    message: String,
    usage: Option<String>,
    suggestion: Option<String>,
    matched_path: Vec<String>,
    unexpected_token: Option<String>,
    expected: Vec<String>,
    format: OutputFormat,
}

impl CliDiagnostic {
    pub(in crate::interfaces::cli) fn new(
        message: String,
        usage: Option<String>,
        suggestion: Option<String>,
        matched_path: Vec<String>,
        unexpected_token: Option<String>,
        expected: Vec<String>,
        format: OutputFormat,
    ) -> Self {
        Self {
            message,
            usage,
            suggestion,
            matched_path,
            unexpected_token,
            expected,
            format,
        }
    }

    fn render_text(&self) -> String {
        let mut output = self.message.clone();
        if let Some(suggestion) = &self.suggestion {
            output.push_str("\nTry: ");
            output.push_str(suggestion);
        }
        if let Some(usage) = &self.usage {
            output.push_str("\nUsage: ");
            output.push_str(usage);
        }

        output
    }

    fn render_stderr(&self) -> String {
        if self.format.is_machine_readable() {
            return serde_json::json!({
                "error": self.message,
                "usage": self.usage,
                "suggestion": self.suggestion,
                "matched_path": self.matched_path,
                "unexpected_token": self.unexpected_token,
                "expected": self.expected,
            })
            .to_string();
        }

        self.render_text()
    }
}
