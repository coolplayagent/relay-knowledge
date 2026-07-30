use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    time::Duration,
};

use serde::{Deserialize, Serialize};

pub(super) const COMMAND_PROGRESS_INTERVAL: Duration = Duration::from_secs(15);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandResult {
    pub name: String,
    pub command: Vec<String>,
    pub exit_code: i32,
    pub duration_ms: u64,
    pub stdout: String,
    pub stderr: String,
}

impl CommandResult {
    pub fn passed(&self) -> bool {
        self.exit_code == 0
    }

    pub fn gate_message(&self) -> String {
        output::last_output_line(&self.stdout, &self.stderr)
    }

    pub fn serializable(&self) -> serde_json::Value {
        serde_json::json!({
            "name": self.name,
            "command": self.command,
            "exit_code": self.exit_code,
            "duration_ms": self.duration_ms,
            "stdout_tail": output::tail(&self.stdout, 4000),
            "stderr_tail": output::tail(&self.stderr, 4000),
        })
    }
}

#[derive(Debug, Clone)]
pub struct CommandSpec {
    pub name: String,
    pub command: Vec<String>,
    pub cwd: PathBuf,
    pub env: Option<BTreeMap<String, String>>,
    pub timeout_seconds: u64,
    pub stdin: Option<String>,
}

impl CommandSpec {
    pub fn new<N: Into<String>>(
        name: N,
        command: Vec<String>,
        cwd: &Path,
        env: Option<BTreeMap<String, String>>,
        timeout_seconds: u64,
    ) -> Self {
        Self {
            name: name.into(),
            command,
            cwd: cwd.to_path_buf(),
            env,
            timeout_seconds,
            stdin: None,
        }
    }

    pub fn with_stdin(mut self, stdin: String) -> Self {
        self.stdin = Some(stdin);
        self
    }
}

pub fn inherited_env() -> BTreeMap<String, String> {
    std::env::vars().collect()
}

mod execution;
mod failure;
mod logging;
mod output;
mod pipes;

pub use execution::run_command;
pub use output::{last_output_line, tail};

#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;
