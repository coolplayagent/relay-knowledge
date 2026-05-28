//! CLI adapter for the shared application service.

#[path = "cli_grammar.rs"]
mod cli_grammar;
#[path = "cli_render.rs"]
mod cli_render;
#[path = "cli_spec.rs"]
mod cli_spec;
#[path = "files_cli.rs"]
mod files_cli;
#[path = "knowledge_cli.rs"]
mod knowledge_cli;
#[path = "map_cli.rs"]
mod map_cli;
#[path = "ops_cli.rs"]
mod ops_cli;
#[path = "repo_cli.rs"]
mod repo_cli;
#[path = "repo_set_cli.rs"]
mod repo_set_cli;
#[path = "service_cli.rs"]
mod service_cli;
#[path = "setup_cli.rs"]
mod setup_cli;
#[path = "version_cli.rs"]
mod version_cli;

use std::{error::Error, fmt};

use crate::{
    api::{
        ApiError, GraphInspectionRequest, HybridRetrievalRequest, IndexRefreshRequest,
        IngestEvidence, IngestRequest, InterfaceKind, RequestContext,
    },
    application::RelayKnowledgeService,
    domain::{FreshnessPolicy, IndexKind, ProposalState, ServiceManagerAction, WorkerKind},
};

use cli_render::{render_project_status, render_response, serialize_line};

/// Supported CLI output formats.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum OutputFormat {
    #[default]
    Text,
    Json,
    Markdown,
    StreamingJson,
}

impl OutputFormat {
    fn as_str(self) -> &'static str {
        match self {
            Self::Text => "text",
            Self::Json => "json",
            Self::Markdown => "markdown",
            Self::StreamingJson => "streaming-json",
        }
    }

    fn is_machine_readable(self) -> bool {
        matches!(self, Self::Json | Self::StreamingJson)
    }

    /// Parses a CLI output format value.
    pub fn parse(value: &str) -> Result<Self, CliError> {
        match value {
            "text" => Ok(Self::Text),
            "json" => Ok(Self::Json),
            "markdown" => Ok(Self::Markdown),
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
            CliAction::Help {
                path: help_path(action_tokens),
            }
        } else if version {
            if let Some(token) = action_tokens.first() {
                let error = CliError::UnexpectedArgument(token.clone());
                return Err(cli_grammar::diagnose(&action_tokens, error, format));
            }
            CliAction::Version
        } else {
            match parse_action(action_tokens.clone()) {
                Ok(action) => action,
                Err(error) => return Err(cli_grammar::diagnose(&action_tokens, error, format)),
            }
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
        "--source"
            | "--content"
            | "--entity"
            | "--limit"
            | "--freshness"
            | "--kind"
            | "--alias"
            | "--path"
            | "--language"
            | "--ref"
            | "--base"
            | "--head"
            | "--query"
            | "--description"
            | "--id"
            | "--priority"
            | "--mcp"
            | "--state"
            | "--by"
            | "--reason"
            | "--operation"
            | "--input"
            | "--root"
            | "--scope"
            | "--topic"
            | "--uri"
    )
}

fn is_command_word(token: &str) -> bool {
    matches!(
        token,
        "status"
            | "ingest"
            | "query"
            | "repo"
            | "repo-set"
            | "files"
            | "map"
            | "graph"
            | "index"
            | "worker"
            | "proposal"
            | "audit"
            | "provider"
            | "health"
            | "service"
            | "setup"
            | "version"
            | "help"
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
    FilesIndex {
        source_scope: Option<String>,
        roots: Vec<String>,
    },
    FilesQuery {
        query: String,
        source_scope: Option<String>,
        root_id: Option<String>,
        limit: usize,
    },
    GraphInspect,
    IndexRefresh {
        kinds: Vec<IndexKind>,
    },
    Map(map_cli::MapCommand),
    WorkerStatus {
        kind: Option<WorkerKind>,
    },
    WorkerRunOnce {
        kind: Option<WorkerKind>,
    },
    ProposalList {
        state: Option<ProposalState>,
        limit: usize,
    },
    ProposalShow {
        proposal_id: String,
    },
    ProposalAccept {
        proposal_id: String,
        actor: String,
        reason: Option<String>,
    },
    ProposalReject {
        proposal_id: String,
        actor: String,
        reason: Option<String>,
    },
    ProposalSupersede {
        proposal_id: String,
        actor: String,
        reason: Option<String>,
    },
    AuditQuery {
        operation: Option<String>,
        limit: usize,
    },
    ProviderProbe,
    Repo(repo_cli::RepoCommand),
    RepoSet(repo_set_cli::RepoSetCommand),
    Health,
    ServiceStatus,
    ServicePlan {
        action: ServiceManagerAction,
    },
    ServiceDefinitionWrite,
    ServiceOperatorStatus,
    ServiceOperatorPause,
    ServiceOperatorResume,
    ServiceRun {
        mcp: ServiceMcpTransport,
        web: bool,
    },
    SetupDoctor,
    SetupProfile {
        profile: setup_cli::SetupProfile,
    },
    Version,
    VersionCheck,
    Help {
        path: Vec<String>,
    },
}

/// MCP transport option for foreground service mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServiceMcpTransport {
    Configured,
    StreamableHttp,
}

/// CLI adapter error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CliError {
    Diagnostic(Box<CliDiagnostic>),
    InvalidFormat(String),
    InvalidCodeQueryKind(String),
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
    fn invalid_format(format: &str) -> Self {
        Self::InvalidFormat(format.to_owned())
    }

    pub(super) fn api_failed(error: ApiError, format: OutputFormat) -> Self {
        Self::ApiError {
            error: Box::new(error),
            format,
        }
    }

    pub(super) fn invalid_api_argument(message: impl Into<String>, format: OutputFormat) -> Self {
        Self::api_failed(ApiError::invalid_argument(message), format)
    }

    /// Returns the process exit code for the error.
    pub fn exit_code(&self) -> i32 {
        match self {
            Self::Diagnostic(_)
            | Self::InvalidFormat(_)
            | Self::InvalidCodeQueryKind(_)
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
                "invalid service action '{value}', expected install or uninstall"
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
    fn new(
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

/// Runs the CLI command and renders its response.
pub async fn run<I, S>(args: I) -> Result<String, CliError>
where
    I: IntoIterator<Item = S>,
    S: Into<String>,
{
    let command = CliCommand::parse(args)?;
    run_command(command).await
}

/// Rendered stdout/stderr for the process entry point.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CliProcessOutput {
    pub stdout: String,
    pub stderr: String,
}

/// Runs the CLI command and renders only the command result.
pub async fn run_process<I, S>(
    args: I,
    _interactive_text_output: bool,
) -> Result<CliProcessOutput, CliError>
where
    I: IntoIterator<Item = S>,
    S: Into<String>,
{
    let command = CliCommand::parse(args)?;
    let stdout = run_command(command).await?;

    Ok(CliProcessOutput {
        stdout,
        stderr: String::new(),
    })
}

/// Renders best-effort process-only notices after primary command output is emitted.
pub async fn process_update_notice<I, S>(args: I, interactive_text_output: bool) -> Option<String>
where
    I: IntoIterator<Item = S>,
    S: Into<String>,
{
    let command = CliCommand::parse(args).ok()?;
    version_cli::update_notice_for_process(&command, interactive_text_output).await
}

async fn run_command(command: CliCommand) -> Result<String, CliError> {
    if let CliAction::Help { path } = &command.action {
        return cli_spec::render_help(path, command.format);
    }
    if command.action == CliAction::Version {
        return version_cli::render_version(command.format);
    }
    if let CliAction::ServiceRun { mcp, web } = command.action.clone() {
        return service_cli::run_service(mcp, web).await;
    }
    if let CliAction::Map(map_command) = command.action.clone() {
        let context = RequestContext::for_interface(InterfaceKind::Cli);
        return map_cli::run_map(map_command, context, command.format).await;
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
    let format = command.format;
    if let Some(output) =
        ops_cli::run_operational_action(service, &command.action, context.clone(), format).await?
    {
        return Ok(output);
    }
    if let Some(output) =
        setup_cli::run_setup_action(service, &command.action, context.clone(), format)?
    {
        return Ok(output);
    }
    if let Some(output) =
        files_cli::run_files(service, &command.action, context.clone(), format).await?
    {
        return Ok(output);
    }
    match command.action {
        CliAction::Status => {
            let response = service
                .project_status(context)
                .await
                .map_err(|error| CliError::api_failed(error, format))?;

            render_project_status(&response, format)
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
                            source_path: None,
                            span: None,
                            confidence: None,
                            status: None,
                            content,
                            entity_labels,
                            extraction: None,
                        }],
                        relations: Vec::new(),
                        claims: Vec::new(),
                        events: Vec::new(),
                    },
                    context,
                )
                .await
                .map_err(|error| CliError::api_failed(error, format))?;

            render_response(
                "knowledge.ingest",
                response.metadata.clone(),
                &response,
                format,
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
                .map_err(|error| CliError::api_failed(error, format))?;

            render_response(
                "knowledge.retrieve_context",
                response.metadata.clone(),
                &response,
                format,
            )
        }
        CliAction::GraphInspect => {
            let response = service
                .inspect_graph(GraphInspectionRequest { source_scope: None }, context)
                .await
                .map_err(|error| CliError::api_failed(error, format))?;

            render_response(
                "graph.inspect",
                response.metadata.clone(),
                &response,
                format,
            )
        }
        CliAction::IndexRefresh { kinds } => {
            let response = service
                .refresh_indexes(IndexRefreshRequest { kinds }, context)
                .await
                .map_err(|error| CliError::api_failed(error, format))?;

            render_response(
                "index.refresh",
                response.metadata.clone(),
                &response,
                format,
            )
        }
        CliAction::Map(command) => map_cli::run_map(command, context, format).await,
        CliAction::Repo(command) => repo_cli::run_repo(service, command, context, format).await,
        CliAction::RepoSet(command) => {
            repo_set_cli::run_repo_set(service, command, context, format).await
        }
        CliAction::Health => {
            let response = service
                .health(context)
                .await
                .map_err(|error| CliError::api_failed(error, format))?;

            render_response(
                "service.health",
                response.metadata.clone(),
                &response,
                format,
            )
        }
        CliAction::ProviderProbe => {
            let response = service
                .probe_embedding_provider(context)
                .await
                .map_err(|error| CliError::api_failed(error, format))?;

            render_response(
                "provider.embedding.probe",
                response.metadata.clone(),
                &response,
                format,
            )
        }
        CliAction::VersionCheck => version_cli::run_version_check(service, format).await,
        CliAction::ServiceRun { .. } => Err(CliError::ServiceRunFailed(
            "service run requires process runtime".to_owned(),
        )),
        CliAction::Help { path } => cli_spec::render_help(&path, format),
        CliAction::WorkerStatus { .. }
        | CliAction::FilesIndex { .. }
        | CliAction::FilesQuery { .. }
        | CliAction::WorkerRunOnce { .. }
        | CliAction::ProposalList { .. }
        | CliAction::ProposalShow { .. }
        | CliAction::ProposalAccept { .. }
        | CliAction::ProposalReject { .. }
        | CliAction::ProposalSupersede { .. }
        | CliAction::AuditQuery { .. }
        | CliAction::ServiceStatus
        | CliAction::ServicePlan { .. }
        | CliAction::ServiceDefinitionWrite
        | CliAction::ServiceOperatorStatus
        | CliAction::ServiceOperatorPause
        | CliAction::ServiceOperatorResume
        | CliAction::SetupDoctor
        | CliAction::SetupProfile { .. } => Err(CliError::ApiFailed(
            "operational command was not handled by the service adapter".to_owned(),
        )),
        CliAction::Version => version_cli::render_version(command.format),
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
        "ingest" => knowledge_cli::parse_ingest(&tokens[1..]),
        "query" => knowledge_cli::parse_query(&tokens[1..]),
        "files" => files_cli::parse_files(&tokens[1..]),
        "map" => map_cli::parse_map(&tokens[1..]),
        "repo" => repo_cli::parse_repo(&tokens[1..]).map(CliAction::Repo),
        "repo-set" => repo_set_cli::parse_repo_set(&tokens[1..]).map(CliAction::RepoSet),
        "graph" => knowledge_cli::parse_graph(&tokens[1..]),
        "index" => knowledge_cli::parse_index(&tokens[1..]),
        "worker" => ops_cli::parse_worker(&tokens[1..]),
        "proposal" => ops_cli::parse_proposal(&tokens[1..]),
        "audit" => ops_cli::parse_audit(&tokens[1..]),
        "provider" => parse_provider(&tokens[1..]),
        "health" if tokens.len() == 1 => Ok(CliAction::Health),
        "service" => ops_cli::parse_service(&tokens[1..]),
        "setup" => setup_cli::parse_setup(&tokens[1..]),
        "version" if tokens.len() == 1 => Ok(CliAction::Version),
        "version" if tokens == ["version", "check"] => Ok(CliAction::VersionCheck),
        "help" => Ok(CliAction::Help {
            path: help_path(tokens[1..].to_vec()),
        }),
        other => Err(CliError::UnexpectedArgument(other.to_owned())),
    }
}

fn help_path(tokens: Vec<String>) -> Vec<String> {
    tokens
        .into_iter()
        .filter(|token| token != "--")
        .filter(|token| !token.starts_with('-'))
        .collect()
}

fn parse_provider(tokens: &[String]) -> Result<CliAction, CliError> {
    if tokens == ["probe"] {
        return Ok(CliAction::ProviderProbe);
    }

    Err(CliError::UnexpectedArgument(
        tokens
            .first()
            .cloned()
            .unwrap_or_else(|| "provider".to_owned()),
    ))
}

pub(super) fn value_after(
    tokens: &[String],
    index: usize,
    flag: &'static str,
) -> Result<String, CliError> {
    tokens
        .get(index + 1)
        .cloned()
        .ok_or(CliError::MissingValue(flag))
}

pub(super) fn parse_freshness(value: &str) -> Result<FreshnessPolicy, CliError> {
    match value {
        "allow-stale" => Ok(FreshnessPolicy::AllowStale),
        "wait-until-fresh" => Ok(FreshnessPolicy::WaitUntilFresh),
        "graph-only" => Ok(FreshnessPolicy::GraphOnly),
        other => Err(CliError::InvalidFreshness(other.to_owned())),
    }
}

#[cfg(test)]
use service_cli::ensure_web_remote_bind_allowed;

#[cfg(test)]
#[path = "cli_naming_tests.rs"]
mod cli_naming_tests;

#[cfg(test)]
#[path = "cli_tests.rs"]
mod cli_tests;

#[cfg(test)]
#[path = "cli_map_tests.rs"]
mod cli_map_tests;

#[cfg(test)]
#[path = "cli_service_tests.rs"]
mod cli_service_tests;

#[cfg(test)]
#[path = "cli_version_tests.rs"]
mod cli_version_tests;
