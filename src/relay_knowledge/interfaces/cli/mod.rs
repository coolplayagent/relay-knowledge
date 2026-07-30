//! CLI adapter for the shared application service.

#[path = "grammar.rs"]
mod cli_grammar;
#[path = "render/mod.rs"]
mod cli_render;
#[path = "spec/mod.rs"]
mod cli_spec;
#[path = "command/mod.rs"]
mod command;
#[path = "files.rs"]
mod files_cli;
#[path = "knowledge.rs"]
mod knowledge_cli;
#[path = "map.rs"]
pub(crate) mod map_cli;
#[path = "operations.rs"]
mod ops_cli;
#[path = "remote.rs"]
mod remote_cli;
#[path = "repo/mod.rs"]
mod repo_cli;
#[path = "repo/view.rs"]
pub(crate) mod repo_cli_view;
#[path = "repo_set/mod.rs"]
mod repo_set_cli;
#[path = "runtime/mod.rs"]
mod runtime;
#[path = "service.rs"]
mod service_cli;
#[path = "setup/mod.rs"]
mod setup_cli;
#[path = "version.rs"]
mod version_cli;

use crate::{
    api::ServicePlanRequest,
    domain::{FreshnessPolicy, IndexKind, ProposalState, WorkerKind},
};

use cli_render::{render_response, serialize_line};
pub use command::{CliDiagnostic, CliError};
pub(crate) use runtime::run_command;
pub use runtime::run_with_service;

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
}

/// Parsed CLI command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CliCommand {
    pub action: CliAction,
    pub format: OutputFormat,
    pub remote_base_url: Option<String>,
    pub help: bool,
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
        freshness: FreshnessPolicy,
    },
    FilesContentQuery {
        query: String,
        source_scope: Option<String>,
        root_id: Option<String>,
        limit: usize,
        freshness: FreshnessPolicy,
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
    ServicePlan(ServicePlanRequest),
    ServiceDefinitionWrite,
    ServiceOperatorStatus,
    ServiceOperatorPause,
    ServiceOperatorResume,
    ServiceWorkerRun {
        task_id: Option<String>,
    },
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

/// Runs the CLI command and renders its response.
pub async fn run<I, S>(args: I) -> Result<String, CliError>
where
    I: IntoIterator<Item = S>,
    S: Into<String>,
{
    let output = crate::bootstrap::cli::run_process(args, false).await?;
    Ok(output.stdout)
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
    interactive_text_output: bool,
) -> Result<CliProcessOutput, CliError>
where
    I: IntoIterator<Item = S>,
    S: Into<String>,
{
    crate::bootstrap::cli::run_process(args, interactive_text_output).await
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

#[cfg(test)]
use service_cli::ensure_web_remote_bind_allowed;

#[cfg(test)]
#[path = "tests/naming.rs"]
mod cli_naming_tests;

#[cfg(test)]
#[path = "tests/general.rs"]
mod cli_tests;

#[cfg(test)]
#[path = "tests/parse.rs"]
mod cli_parse_tests;

#[cfg(test)]
#[path = "tests/remote.rs"]
mod remote_cli_tests;

#[cfg(test)]
#[path = "tests/map.rs"]
mod cli_map_tests;

#[cfg(test)]
#[path = "tests/service.rs"]
mod cli_service_tests;

#[cfg(test)]
#[path = "tests/version.rs"]
mod cli_version_tests;
