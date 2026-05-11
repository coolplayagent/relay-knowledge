//! CLI adapter for the shared application service.

use std::{error::Error, fmt};

use crate::{
    api::{ApiStreamEvent, InterfaceKind, ProjectStatusResponse, RequestContext, StreamEventKind},
    application::RelayKnowledgeService,
};

/// Supported CLI output formats.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum OutputFormat {
    #[default]
    Text,
    Json,
    StreamingJson,
}

impl OutputFormat {
    /// Parses a CLI output format value.
    pub fn parse(value: &str) -> Result<Self, CliError> {
        match value {
            "text" => Ok(Self::Text),
            "json" => Ok(Self::Json),
            "streaming-json" => Ok(Self::StreamingJson),
            other => Err(CliError::invalid_format(other)),
        }
    }
}

/// Parsed CLI command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CliCommand {
    pub format: OutputFormat,
    pub help: bool,
}

impl CliCommand {
    /// Parses the CLI arguments after the binary name.
    pub fn parse<I, S>(args: I) -> Result<Self, CliError>
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        let mut format = OutputFormat::default();
        let mut help = false;
        let mut args = args.into_iter().map(Into::into).peekable();

        while let Some(arg) = args.next() {
            if arg == "--format" {
                let value = args.next().ok_or(CliError::MissingFormatValue)?;
                format = OutputFormat::parse(&value)?;
            } else if let Some(value) = arg.strip_prefix("--format=") {
                format = OutputFormat::parse(value)?;
            } else if arg == "--help" || arg == "-h" {
                help = true;
            } else {
                return Err(CliError::UnexpectedArgument(arg));
            }
        }

        Ok(Self { format, help })
    }
}

/// CLI adapter error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CliError {
    InvalidFormat(String),
    MissingFormatValue,
    UnexpectedArgument(String),
    RenderFailed(String),
}

impl CliError {
    fn invalid_format(format: &str) -> Self {
        Self::InvalidFormat(format.to_owned())
    }

    /// Returns the process exit code for the error.
    pub fn exit_code(&self) -> i32 {
        match self {
            Self::InvalidFormat(_) | Self::MissingFormatValue | Self::UnexpectedArgument(_) => 2,
            Self::RenderFailed(_) => 1,
        }
    }
}

impl fmt::Display for CliError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidFormat(format) => write!(
                formatter,
                "invalid --format value '{format}', expected text, json, or streaming-json"
            ),
            Self::MissingFormatValue => {
                write!(formatter, "missing value for --format")
            }
            Self::UnexpectedArgument(argument) => {
                write!(formatter, "unexpected argument '{argument}'")
            }
            Self::RenderFailed(message) => write!(formatter, "failed to render output: {message}"),
        }
    }
}

impl Error for CliError {}

/// Runs the default CLI command and renders its response.
pub fn run<I, S>(args: I) -> Result<String, CliError>
where
    I: IntoIterator<Item = S>,
    S: Into<String>,
{
    let command = CliCommand::parse(args)?;
    if command.help {
        return Ok(help_text().to_owned());
    }

    let service = RelayKnowledgeService::new();
    let context = RequestContext::for_interface(InterfaceKind::Cli);
    let response = service.project_status(context);

    render_project_status(&response, command.format)
}

/// Returns the CLI help text.
pub fn help_text() -> &'static str {
    "Usage: relay-knowledge [--format text|json|streaming-json]\n"
}

/// Renders a project status response in the requested CLI format.
pub fn render_project_status(
    response: &ProjectStatusResponse,
    format: OutputFormat,
) -> Result<String, CliError> {
    match format {
        OutputFormat::Text => Ok(format!("{}\n", response.project_name)),
        OutputFormat::Json => serialize_line(response),
        OutputFormat::StreamingJson => render_streaming_project_status(response),
    }
}

fn render_streaming_project_status(response: &ProjectStatusResponse) -> Result<String, CliError> {
    let events = [
        ApiStreamEvent::project_status(
            StreamEventKind::Started,
            response,
            Some("project status request started"),
        ),
        ApiStreamEvent::project_status(
            StreamEventKind::Progress,
            response,
            Some("application service returned project status"),
        ),
        ApiStreamEvent::project_status(StreamEventKind::Item, response, None),
        ApiStreamEvent::project_status(
            StreamEventKind::Completed,
            response,
            Some("project status request completed"),
        ),
    ];

    let mut output = String::new();
    for event in events {
        output.push_str(&serialize_line(&event)?);
    }

    Ok(output)
}

fn serialize_line<T>(value: &T) -> Result<String, CliError>
where
    T: serde::Serialize,
{
    let line =
        serde_json::to_string(value).map_err(|error| CliError::RenderFailed(error.to_string()))?;

    Ok(format!("{line}\n"))
}
