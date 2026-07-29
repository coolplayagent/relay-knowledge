use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodexResult {
    pub command: Vec<String>,
    pub exit_code: i32,
    pub duration_ms: u64,
    pub stdout: String,
    pub stderr: String,
}

impl CodexResult {
    pub fn succeeded(&self) -> bool {
        self.exit_code == 0
    }

    pub fn serializable(&self) -> serde_json::Value {
        serde_json::json!({
            "command": self.command,
            "exit_code": self.exit_code,
            "duration_ms": self.duration_ms,
            "stdout_tail": crate::command::tail(&self.stdout, 4000),
            "stderr_tail": crate::command::tail(&self.stderr, 4000),
        })
    }
}

mod command;
mod execution;
mod history_context;
mod prompt;
mod result_mapping;
mod unattended_prompt;

pub use execution::run_codex;
pub use prompt::build_prompt;
pub use unattended_prompt::build_unattended_prompt;

#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;
