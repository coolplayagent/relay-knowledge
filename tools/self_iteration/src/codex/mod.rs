use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    command::{CommandResult, CommandSpec, run_command},
    config::{CategorySet, Config, DEFAULT_CODEX_MODEL, EvaluationCategory},
    history::{
        HistoryPaths, adopted, best_accepted_run_for_profile, best_accepted_run_for_workload,
        is_evaluate_run, load_runs,
        memory::{
            historical_patch_memory_index, progressive_memory_index,
            rejection_recovery_memory_review,
        },
        synthesis::synthesize_history,
    },
};

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

include!("execution.rs");
include!("command.rs");
include!("prompt.rs");
include!("unattended_prompt.rs");
include!("history_context.rs");
include!("result_mapping.rs");

include!("command_tests.rs");
include!("prompt_tests.rs");
