//! CLI adapter for the shared application service.

use std::{error::Error, fmt};

use crate::{
    api::{
        ApiMetadata, ApiStreamEvent, GraphInspectionRequest, HybridRetrievalRequest,
        IndexRefreshRequest, IngestEvidence, IngestRequest, InterfaceKind, ProjectStatusResponse,
        RequestContext, StreamEventKind,
    },
    application::RelayKnowledgeService,
    domain::{FreshnessPolicy, IndexKind},
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
    fn as_str(self) -> &'static str {
        match self {
            Self::Text => "text",
            Self::Json => "json",
            Self::StreamingJson => "streaming-json",
        }
    }

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
    pub action: CliAction,
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
        let tokens = args.into_iter().map(Into::into).collect::<Vec<_>>();
        let mut action_tokens = Vec::new();
        let mut format = OutputFormat::default();
        let mut help = false;
        let mut version = false;
        let mut command_seen = false;
        let mut delimiter_value = false;
        let mut index = 0;

        while index < tokens.len() {
            let arg = &tokens[index];
            if delimiter_value {
                action_tokens.push(arg.clone());
                delimiter_value = false;
                index += 1;
            } else if arg == "--format" {
                let value = tokens
                    .get(index + 1)
                    .ok_or(CliError::MissingFormatValue)?
                    .clone();
                format = OutputFormat::parse(&value)?;
                index += 2;
            } else if let Some(value) = arg.strip_prefix("--format=") {
                format = OutputFormat::parse(value)?;
                index += 1;
            } else if arg == "--help" || arg == "-h" {
                help = true;
                index += 1;
            } else if arg == "--version" && !command_seen {
                version = true;
                index += 1;
            } else if arg == "--" {
                action_tokens.push(arg.clone());
                delimiter_value = true;
                index += 1;
            } else if option_consumes_value(arg) {
                action_tokens.push(arg.clone());
                if let Some(value) = tokens.get(index + 1) {
                    action_tokens.push(value.clone());
                    index += 2;
                } else {
                    index += 1;
                }
            } else {
                command_seen |= is_command_word(arg);
                action_tokens.push(arg.clone());
                index += 1;
            }
        }

        let action = if help {
            CliAction::Status
        } else if version {
            if let Some(token) = action_tokens.first() {
                return Err(CliError::UnexpectedArgument(token.clone()));
            }
            CliAction::Version
        } else {
            parse_action(action_tokens)?
        };

        Ok(Self {
            action,
            format,
            help,
        })
    }
}

fn option_consumes_value(option: &str) -> bool {
    matches!(
        option,
        "--source" | "--content" | "--entity" | "--limit" | "--freshness" | "--kind"
    )
}

fn is_command_word(token: &str) -> bool {
    matches!(
        token,
        "status" | "ingest" | "query" | "graph" | "index" | "health" | "service" | "version"
    )
}

/// CLI action after global options are removed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CliAction {
    Status,
    Ingest {
        source_scope: String,
        content: String,
        entity_labels: Vec<String>,
    },
    Query {
        query: String,
        source_scope: Option<String>,
        limit: usize,
        freshness: FreshnessPolicy,
    },
    GraphInspect,
    IndexRefresh {
        kinds: Vec<IndexKind>,
    },
    Health,
    ServiceStatus,
    Version,
}

/// CLI adapter error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CliError {
    InvalidFormat(String),
    InvalidFreshness(String),
    InvalidIndexKind(String),
    InvalidLimit(String),
    MissingFormatValue,
    MissingValue(&'static str),
    UnsupportedVersionFormat(OutputFormat),
    UnexpectedArgument(String),
    RuntimeConfigFailed(String),
    ApiFailed(String),
    RenderFailed(String),
}

impl CliError {
    fn invalid_format(format: &str) -> Self {
        Self::InvalidFormat(format.to_owned())
    }

    /// Returns the process exit code for the error.
    pub fn exit_code(&self) -> i32 {
        match self {
            Self::InvalidFormat(_)
            | Self::InvalidFreshness(_)
            | Self::InvalidIndexKind(_)
            | Self::InvalidLimit(_)
            | Self::MissingFormatValue
            | Self::MissingValue(_)
            | Self::UnsupportedVersionFormat(_)
            | Self::UnexpectedArgument(_) => 2,
            Self::RuntimeConfigFailed(_) | Self::ApiFailed(_) | Self::RenderFailed(_) => 1,
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
            Self::InvalidFreshness(value) => write!(
                formatter,
                "invalid --freshness value '{value}', expected allow-stale, wait-until-fresh, or graph-only"
            ),
            Self::InvalidIndexKind(value) => write!(
                formatter,
                "invalid --kind value '{value}', expected bm25, semantic, or vector"
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
            Self::UnexpectedArgument(argument) => {
                write!(formatter, "unexpected argument '{argument}'")
            }
            Self::RuntimeConfigFailed(message) => {
                write!(formatter, "failed to load runtime configuration: {message}")
            }
            Self::ApiFailed(message) => write!(formatter, "{message}"),
            Self::RenderFailed(message) => write!(formatter, "failed to render output: {message}"),
        }
    }
}

impl Error for CliError {}

/// Runs the CLI command and renders its response.
pub async fn run<I, S>(args: I) -> Result<String, CliError>
where
    I: IntoIterator<Item = S>,
    S: Into<String>,
{
    let command = CliCommand::parse(args)?;
    if command.help {
        return Ok(help_text().to_owned());
    }
    if command.action == CliAction::Version {
        return render_version(command.format);
    }

    let service = RelayKnowledgeService::from_process_environment()
        .await
        .map_err(|error| CliError::RuntimeConfigFailed(error.to_string()))?;
    let context = RequestContext::for_interface(InterfaceKind::Cli);

    run_with_service(&service, command, context).await
}

/// Runs a parsed CLI command with an already composed service.
pub async fn run_with_service(
    service: &RelayKnowledgeService,
    command: CliCommand,
    context: RequestContext,
) -> Result<String, CliError> {
    match command.action {
        CliAction::Status => {
            let response = service
                .project_status(context)
                .await
                .map_err(|error| CliError::ApiFailed(error.message))?;

            render_project_status(&response, command.format)
        }
        CliAction::Ingest {
            source_scope,
            content,
            entity_labels,
        } => {
            let response = service
                .ingest(
                    IngestRequest {
                        source_scope,
                        evidence: vec![IngestEvidence {
                            id: None,
                            content,
                            entity_labels,
                        }],
                    },
                    context,
                )
                .await
                .map_err(|error| CliError::ApiFailed(error.message))?;

            render_response(
                "knowledge.ingest",
                response.metadata.clone(),
                &response,
                command.format,
            )
        }
        CliAction::Query {
            query,
            source_scope,
            limit,
            freshness,
        } => {
            let response = service
                .retrieve_context(
                    HybridRetrievalRequest {
                        query,
                        source_scope,
                        limit,
                        freshness,
                    },
                    context,
                )
                .await
                .map_err(|error| CliError::ApiFailed(error.message))?;

            render_response(
                "knowledge.retrieve_context",
                response.metadata.clone(),
                &response,
                command.format,
            )
        }
        CliAction::GraphInspect => {
            let response = service
                .inspect_graph(GraphInspectionRequest { source_scope: None }, context)
                .await
                .map_err(|error| CliError::ApiFailed(error.message))?;

            render_response(
                "graph.inspect",
                response.metadata.clone(),
                &response,
                command.format,
            )
        }
        CliAction::IndexRefresh { kinds } => {
            let response = service
                .refresh_indexes(IndexRefreshRequest { kinds }, context)
                .await
                .map_err(|error| CliError::ApiFailed(error.message))?;

            render_response(
                "index.refresh",
                response.metadata.clone(),
                &response,
                command.format,
            )
        }
        CliAction::Health => {
            let response = service
                .health(context)
                .await
                .map_err(|error| CliError::ApiFailed(error.message))?;

            render_response(
                "service.health",
                response.metadata.clone(),
                &response,
                command.format,
            )
        }
        CliAction::ServiceStatus => {
            let response = service
                .service_status(context)
                .await
                .map_err(|error| CliError::ApiFailed(error.message))?;

            render_response(
                "service.status",
                response.metadata.clone(),
                &response,
                command.format,
            )
        }
        CliAction::Version => render_version(command.format),
    }
}

/// Returns the CLI help text.
pub fn help_text() -> &'static str {
    concat!(
        "Usage: relay-knowledge [status] [--format text|json|streaming-json]\n",
        "Commands:\n",
        "  status\n",
        "  ingest --source <scope> --content <text> [--entity <label>]\n",
        "  query <text> [--source <scope>] [--limit <n>] ",
        "[--freshness allow-stale|wait-until-fresh|graph-only]\n",
        "  graph inspect\n",
        "  index refresh [--kind bm25|semantic|vector]\n",
        "  health\n",
        "  service status|doctor\n",
        "  version [--format text|json]\n",
        "  --version [--format text|json]\n",
    )
}

#[derive(serde::Serialize)]
struct VersionResponse {
    project_name: &'static str,
    version: &'static str,
}

fn render_version(format: OutputFormat) -> Result<String, CliError> {
    match format {
        OutputFormat::Text => Ok(format!("relay-knowledge {}\n", env!("CARGO_PKG_VERSION"))),
        OutputFormat::Json => serialize_line(&VersionResponse {
            project_name: "relay-knowledge",
            version: env!("CARGO_PKG_VERSION"),
        }),
        OutputFormat::StreamingJson => Err(CliError::UnsupportedVersionFormat(format)),
    }
}

/// Renders a project status response in the requested CLI format.
pub fn render_project_status(
    response: &ProjectStatusResponse,
    format: OutputFormat,
) -> Result<String, CliError> {
    match format {
        OutputFormat::Text => render_text("project.status", response),
        OutputFormat::Json => serialize_line(response),
        OutputFormat::StreamingJson => render_streaming_project_status(response),
    }
}

fn parse_action(tokens: Vec<String>) -> Result<CliAction, CliError> {
    if tokens.is_empty() || tokens == ["status"] {
        return Ok(CliAction::Status);
    }

    match tokens[0].as_str() {
        "status" => Err(CliError::UnexpectedArgument(
            tokens
                .get(1)
                .cloned()
                .unwrap_or_else(|| "status".to_owned()),
        )),
        "ingest" => parse_ingest(&tokens[1..]),
        "query" => parse_query(&tokens[1..]),
        "graph" => parse_graph(&tokens[1..]),
        "index" => parse_index(&tokens[1..]),
        "health" if tokens.len() == 1 => Ok(CliAction::Health),
        "service" => parse_service(&tokens[1..]),
        "version" if tokens.len() == 1 => Ok(CliAction::Version),
        other => Err(CliError::UnexpectedArgument(other.to_owned())),
    }
}

fn parse_ingest(tokens: &[String]) -> Result<CliAction, CliError> {
    let mut source_scope = None;
    let mut content = None;
    let mut entity_labels = Vec::new();
    let mut index = 0;

    while index < tokens.len() {
        match tokens[index].as_str() {
            "--source" => {
                source_scope = Some(value_after(tokens, index, "--source")?);
                index += 2;
            }
            "--content" => {
                content = Some(value_after(tokens, index, "--content")?);
                index += 2;
            }
            "--entity" => {
                entity_labels.push(value_after(tokens, index, "--entity")?);
                index += 2;
            }
            other => return Err(CliError::UnexpectedArgument(other.to_owned())),
        }
    }

    Ok(CliAction::Ingest {
        source_scope: source_scope.ok_or(CliError::MissingValue("--source"))?,
        content: content.ok_or(CliError::MissingValue("--content"))?,
        entity_labels,
    })
}

fn parse_query(tokens: &[String]) -> Result<CliAction, CliError> {
    let mut query = None;
    let mut source_scope = None;
    let mut limit = 10;
    let mut freshness = FreshnessPolicy::default();
    let mut index = 0;

    while index < tokens.len() {
        match tokens[index].as_str() {
            "--" if query.is_none() => {
                query = Some(value_after(tokens, index, "query")?);
                index += 2;
            }
            "--source" => {
                source_scope = Some(value_after(tokens, index, "--source")?);
                index += 2;
            }
            "--limit" => {
                let value = value_after(tokens, index, "--limit")?;
                limit = value
                    .parse::<usize>()
                    .map_err(|_| CliError::InvalidLimit(value.clone()))?;
                index += 2;
            }
            "--freshness" => {
                freshness = parse_freshness(&value_after(tokens, index, "--freshness")?)?;
                index += 2;
            }
            other if !other.starts_with('-') && query.is_none() => {
                query = Some(other.to_owned());
                index += 1;
            }
            other => return Err(CliError::UnexpectedArgument(other.to_owned())),
        }
    }

    Ok(CliAction::Query {
        query: query.ok_or(CliError::MissingValue("query"))?,
        source_scope,
        limit,
        freshness,
    })
}

fn parse_graph(tokens: &[String]) -> Result<CliAction, CliError> {
    if tokens == ["inspect"] {
        return Ok(CliAction::GraphInspect);
    }

    Err(CliError::UnexpectedArgument(
        tokens
            .first()
            .cloned()
            .unwrap_or_else(|| "graph".to_owned()),
    ))
}

fn parse_index(tokens: &[String]) -> Result<CliAction, CliError> {
    if tokens.first().map(String::as_str) != Some("refresh") {
        return Err(CliError::UnexpectedArgument(
            tokens
                .first()
                .cloned()
                .unwrap_or_else(|| "index".to_owned()),
        ));
    }

    let mut kinds = Vec::new();
    let mut index = 1;
    while index < tokens.len() {
        match tokens[index].as_str() {
            "--kind" => {
                kinds.push(parse_index_kind(&value_after(tokens, index, "--kind")?)?);
                index += 2;
            }
            other => return Err(CliError::UnexpectedArgument(other.to_owned())),
        }
    }

    Ok(CliAction::IndexRefresh { kinds })
}

fn parse_service(tokens: &[String]) -> Result<CliAction, CliError> {
    if tokens == ["status"] || tokens == ["doctor"] {
        return Ok(CliAction::ServiceStatus);
    }

    Err(CliError::UnexpectedArgument(
        tokens
            .first()
            .cloned()
            .unwrap_or_else(|| "service".to_owned()),
    ))
}

fn value_after(tokens: &[String], index: usize, flag: &'static str) -> Result<String, CliError> {
    tokens
        .get(index + 1)
        .cloned()
        .ok_or(CliError::MissingValue(flag))
}

fn parse_freshness(value: &str) -> Result<FreshnessPolicy, CliError> {
    match value {
        "allow-stale" => Ok(FreshnessPolicy::AllowStale),
        "wait-until-fresh" => Ok(FreshnessPolicy::WaitUntilFresh),
        "graph-only" => Ok(FreshnessPolicy::GraphOnly),
        other => Err(CliError::InvalidFreshness(other.to_owned())),
    }
}

fn parse_index_kind(value: &str) -> Result<IndexKind, CliError> {
    match value {
        "bm25" => Ok(IndexKind::Bm25),
        "semantic" => Ok(IndexKind::Semantic),
        "vector" => Ok(IndexKind::Vector),
        other => Err(CliError::InvalidIndexKind(other.to_owned())),
    }
}

fn render_response<T>(
    operation: &str,
    metadata: ApiMetadata,
    response: &T,
    format: OutputFormat,
) -> Result<String, CliError>
where
    T: serde::Serialize,
{
    match format {
        OutputFormat::Text => render_text(operation, response),
        OutputFormat::Json => serialize_line(response),
        OutputFormat::StreamingJson => render_streaming_response(operation, metadata, response),
    }
}

fn render_text<T>(operation: &str, response: &T) -> Result<String, CliError>
where
    T: serde::Serialize,
{
    let value = serde_json::to_value(response)
        .map_err(|error| CliError::RenderFailed(error.to_string()))?;
    let line = match operation {
        "project.status" => value["project_name"]
            .as_str()
            .unwrap_or("relay-knowledge")
            .to_owned(),
        "knowledge.ingest" => format!(
            "ingested graph_version={} evidence_count={}",
            value["metadata"]["graph_version"].as_u64().unwrap_or(0),
            value["receipt"]["evidence_count"].as_u64().unwrap_or(0)
        ),
        "knowledge.retrieve_context" => {
            format!(
                "results={}",
                value["results"].as_array().map_or(0, Vec::len)
            )
        }
        "graph.inspect" => format!(
            "graph_version={} entities={} evidence={} code_files={} code_symbols={}",
            value["graph"]["graph_version"].as_u64().unwrap_or(0),
            value["graph"]["entity_count"].as_u64().unwrap_or(0),
            value["graph"]["evidence_count"].as_u64().unwrap_or(0),
            value["graph"]["code_file_count"].as_u64().unwrap_or(0),
            value["graph"]["code_symbol_count"].as_u64().unwrap_or(0)
        ),
        "index.refresh" => format!(
            "refreshed_indexes={}",
            value["indexes"].as_array().map_or(0, Vec::len)
        ),
        "service.health" => format!("healthy={}", value["healthy"].as_bool().unwrap_or(false)),
        "service.status" => format!(
            "service={} mode={}",
            value["service_name"].as_str().unwrap_or("relay-knowledge"),
            value["mode"].as_str().unwrap_or("disabled")
        ),
        _ => operation.to_owned(),
    };

    Ok(format!("{line}\n"))
}

fn render_streaming_response<T>(
    operation: &str,
    metadata: ApiMetadata,
    response: &T,
) -> Result<String, CliError>
where
    T: serde::Serialize,
{
    let payload = serde_json::to_value(response)
        .map_err(|error| CliError::RenderFailed(error.to_string()))?;
    let events = [
        ApiStreamEvent::operation(
            StreamEventKind::Started,
            operation,
            metadata.clone(),
            Some("operation started"),
            None,
        ),
        ApiStreamEvent::operation(
            StreamEventKind::Item,
            operation,
            metadata.clone(),
            None,
            Some(payload),
        ),
        ApiStreamEvent::operation(
            StreamEventKind::Completed,
            operation,
            metadata,
            Some("operation completed"),
            None,
        ),
    ];
    let mut output = String::new();
    for event in events {
        output.push_str(&serialize_line(&event)?);
    }

    Ok(output)
}

fn render_streaming_project_status(response: &ProjectStatusResponse) -> Result<String, CliError> {
    let events = [
        ApiStreamEvent::project_status(StreamEventKind::Started, response, Some("status started")),
        ApiStreamEvent::project_status(
            StreamEventKind::Progress,
            response,
            Some("runtime configuration loaded"),
        ),
        ApiStreamEvent::project_status(StreamEventKind::Item, response, None),
        ApiStreamEvent::project_status(
            StreamEventKind::Completed,
            response,
            Some("status completed"),
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

#[cfg(test)]
#[path = "cli_tests.rs"]
mod cli_tests;
