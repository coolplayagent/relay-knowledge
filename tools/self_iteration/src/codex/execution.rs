use crate::{
    command::{CommandSpec, run_command},
    config::Config,
};

use super::{CodexResult, command::build_codex_command, result_mapping::from_command};

pub fn run_codex(config: &Config, prompt: &str) -> CodexResult {
    let command = build_codex_command(config);
    if config.dry_run_codex {
        return CodexResult {
            command,
            exit_code: 0,
            duration_ms: 0,
            stdout: "dry-run: codex was not invoked\n".to_owned(),
            stderr: String::new(),
        };
    }
    let result = run_command(
        &CommandSpec::new(
            "codex_generation",
            command,
            &config.workspace,
            None,
            config.codex_timeout_seconds,
        )
        .with_stdin(prompt.to_owned()),
    );
    from_command(result)
}

#[cfg(test)]
#[path = "execution_tests.rs"]
mod execution_tests;
