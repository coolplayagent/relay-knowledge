//! CLI process bootstrap entry points.
//!
//! These functions are the documented process-level facade for CLI execution.
//! They own process inputs such as cwd and delegate command behavior to the
//! interface layer after process-local contracts are resolved.

use crate::{
    application::KnowledgeMapService,
    interfaces::cli::{CliAction, CliCommand, OutputFormat},
    paths::discover_repository_root,
    project::KNOWLEDGE_MAP_RELATIVE_PATH,
};

pub use crate::interfaces::cli::{CliError, CliProcessOutput};

/// Runs a CLI process invocation through the outer bootstrap layer.
///
/// `args` are the command-line arguments after the binary name. The
/// `interactive_text_output` flag captures terminal capability detection from
/// the binary entry point so output-only tests can run without reading process
/// terminal state.
pub async fn run_process<I, S>(
    args: I,
    interactive_text_output: bool,
) -> Result<CliProcessOutput, CliError>
where
    I: IntoIterator<Item = S>,
    S: Into<String>,
{
    let command = CliCommand::parse(args)?;
    let stdout = run_command(command, interactive_text_output).await?;

    Ok(CliProcessOutput {
        stdout,
        stderr: String::new(),
    })
}

/// Renders best-effort process notices after primary CLI output is emitted.
///
/// The notice lifecycle is process-level behavior because it depends on the
/// final command, terminal mode, and post-command update checks. It is exposed
/// here so `main.rs` no longer calls the interface adapter directly.
pub async fn process_update_notice<I, S>(args: I, interactive_text_output: bool) -> Option<String>
where
    I: IntoIterator<Item = S>,
    S: Into<String>,
{
    crate::interfaces::cli::update_notice_for_process(args, interactive_text_output).await
}

async fn run_command(
    command: CliCommand,
    _interactive_text_output: bool,
) -> Result<String, CliError> {
    let service = match &command.action {
        CliAction::Map(map_command) if map_command.needs_repository_root() => {
            Some(knowledge_map_service(command.format)?)
        }
        _ => None,
    };
    crate::interfaces::cli::run_command(command, service.as_ref()).await
}

fn knowledge_map_service(format: OutputFormat) -> Result<KnowledgeMapService, CliError> {
    let current = std::env::current_dir().map_err(|error| {
        CliError::invalid_api_argument(
            format!("failed to resolve current directory: {error}"),
            format,
        )
    })?;
    let root = discover_repository_root(&current)
        .map_err(|error| CliError::invalid_api_argument(error.to_string(), format))?
        .ok_or_else(|| {
            CliError::invalid_api_argument(
                format!("failed to find repository root for {KNOWLEDGE_MAP_RELATIVE_PATH}"),
                format,
            )
        })?;

    Ok(KnowledgeMapService::new(root))
}

#[cfg(test)]
#[allow(deprecated)]
#[path = "cli_tests.rs"]
mod tests;
