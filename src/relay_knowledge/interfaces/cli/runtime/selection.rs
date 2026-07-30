use super::super::{CliCommand, remote_cli};

#[cfg(test)]
#[path = "selection_tests.rs"]
mod tests;

pub(super) fn select_remote_base_url(
    command: &CliCommand,
    environment_base_url: Option<String>,
) -> Option<String> {
    if let Some(base_url) = command.remote_base_url.clone() {
        return Some(base_url);
    }
    if remote_cli::supports(&command.action) || remote_cli::blocks_local_fallback(&command.action) {
        return environment_base_url;
    }

    None
}

pub(super) fn remote_environment_needed(command: &CliCommand) -> bool {
    command.remote_base_url.is_some()
        || remote_cli::supports(&command.action)
        || remote_cli::blocks_local_fallback(&command.action)
}
