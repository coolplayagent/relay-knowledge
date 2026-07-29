use std::path::Path;

use crate::command::{CommandResult, CommandSpec, run_command};

pub(super) fn git_dynamic(
    workspace: &Path,
    args: &[String],
    timeout_seconds: u64,
    check: bool,
) -> Result<CommandResult, String> {
    let mut command = vec!["git".to_owned()];
    command.extend(args.iter().cloned());
    let result = run_command(&CommandSpec::new(
        "git",
        command,
        workspace,
        None,
        timeout_seconds,
    ));
    if check && !result.passed() {
        Err(result.gate_message())
    } else {
        Ok(result)
    }
}

#[cfg(test)]
#[path = "dynamic_command_tests.rs"]
mod dynamic_command_tests;
