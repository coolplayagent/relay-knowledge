use crate::{
    application::{RelayKnowledgeService, RuntimeConfiguration, update_notice},
    project::PROJECT_NAME,
};

use super::{CliAction, CliCommand, CliError, OutputFormat, cli_render::serialize_line};

pub(super) fn render_version(format: OutputFormat) -> Result<String, CliError> {
    match format {
        OutputFormat::Text => Ok(format!("{} {}\n", PROJECT_NAME, env!("CARGO_PKG_VERSION"))),
        OutputFormat::Json => serialize_line(&serde_json::json!({
            "project_name": PROJECT_NAME,
            "version": env!("CARGO_PKG_VERSION"),
        })),
        OutputFormat::Markdown => Ok(format!("{} {}\n", PROJECT_NAME, env!("CARGO_PKG_VERSION"))),
        OutputFormat::StreamingJson => Err(CliError::UnsupportedVersionFormat(format)),
    }
}

pub(super) async fn run_version_check(
    service: &RelayKnowledgeService,
    format: OutputFormat,
) -> Result<String, CliError> {
    if format == OutputFormat::StreamingJson {
        return Err(CliError::UnsupportedVersionFormat(format));
    }
    let response = service.check_for_updates(true).await;
    match format {
        OutputFormat::Text | OutputFormat::Markdown => Ok(render_version_check_text(&response)),
        OutputFormat::Json => serialize_line(&response),
        OutputFormat::StreamingJson => unreachable!("streaming-json was rejected above"),
    }
}

pub(super) async fn update_notice_for_process(
    command: &CliCommand,
    interactive_text_output: bool,
) -> Option<String> {
    if !interactive_text_output || !should_check_after_command(command) {
        return None;
    }
    let runtime = RuntimeConfiguration::from_process_environment()
        .await
        .ok()?;
    update_notice(&runtime.paths, &runtime.network, &runtime.updates).await
}

fn should_check_after_command(command: &CliCommand) -> bool {
    matches!(command.format, OutputFormat::Text | OutputFormat::Markdown)
        && !matches!(
            command.action,
            CliAction::Help { .. }
                | CliAction::Version
                | CliAction::VersionCheck
                | CliAction::ServiceRun { .. }
        )
}

fn render_version_check_text(response: &crate::application::VersionCheckResponse) -> String {
    match (
        response.update_available,
        response.latest_version.as_deref(),
        response.source.as_deref(),
    ) {
        (true, Some(latest), Some(source)) => format!(
            "{} update available: current={} latest={} source={}\n",
            response.project_name, response.current_version, latest, source
        ),
        (true, Some(latest), None) => format!(
            "{} update available: current={} latest={}\n",
            response.project_name, response.current_version, latest
        ),
        (false, Some(latest), Some(source)) => format!(
            "{} is current: current={} latest={} source={}\n",
            response.project_name, response.current_version, latest, source
        ),
        _ => format!(
            "{} latest version unavailable: current={} diagnostics={}\n",
            response.project_name,
            response.current_version,
            response.diagnostics.len()
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_notice_skips_machine_readable_and_version_commands() {
        assert!(!should_check_after_command(&CliCommand {
            action: CliAction::Version,
            format: OutputFormat::Text,
            help: false,
        }));
        assert!(!should_check_after_command(&CliCommand {
            action: CliAction::Status,
            format: OutputFormat::Json,
            help: false,
        }));
        assert!(should_check_after_command(&CliCommand {
            action: CliAction::Status,
            format: OutputFormat::Text,
            help: false,
        }));
    }
}
