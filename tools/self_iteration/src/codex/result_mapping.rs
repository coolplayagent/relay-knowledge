use crate::command::CommandResult;

use super::CodexResult;

pub(super) fn from_command(result: CommandResult) -> CodexResult {
    CodexResult {
        command: result.command,
        exit_code: result.exit_code,
        duration_ms: result.duration_ms,
        stdout: result.stdout,
        stderr: result.stderr,
    }
}

#[cfg(test)]
#[path = "result_mapping_tests.rs"]
mod result_mapping_tests;
