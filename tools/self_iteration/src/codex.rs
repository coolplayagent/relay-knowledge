use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::{
    command::{CommandResult, CommandSpec, run_command},
    config::Config,
    history::{HistoryPaths, best_accepted_run, load_runs},
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

pub fn build_codex_command(config: &Config) -> Vec<String> {
    let codex = config
        .codex_path
        .clone()
        .unwrap_or_else(|| "codex".to_owned());
    let mut command = vec![codex];
    if config.yolo {
        command.extend(["-a".to_owned(), "never".to_owned()]);
    }
    command.extend([
        "exec".to_owned(),
        "-C".to_owned(),
        config.workspace.display().to_string(),
    ]);
    if config.yolo {
        command.extend([
            "--dangerously-bypass-approvals-and-sandbox".to_owned(),
            "-s".to_owned(),
            "danger-full-access".to_owned(),
        ]);
    }
    if let Some(model) = &config.model {
        command.extend(["-m".to_owned(), model.clone()]);
    }
    if let Some(profile) = &config.codex_profile {
        command.extend(["-p".to_owned(), profile.clone()]);
    }
    command.push("-".to_owned());
    command
}

pub fn build_prompt(paths: &HistoryPaths, workspace: &Path, run_id: &str) -> String {
    let best = best_accepted_run(paths).ok().flatten();
    let best_summary = best
        .as_ref()
        .map(|run| {
            format!(
                "Best accepted score={} commit={}",
                value(run, "score"),
                value(run, "commit")
            )
        })
        .unwrap_or_else(|| "No accepted v2 historical run yet.".to_owned());
    let rejected = recent_rejections(paths);
    format!(
        r#"You are running inside relay-knowledge self-iteration run {run_id}.

Goal:
- Preserve foundational capability, competitive capability, semantic/vector retrieval, and stability as protected floors.
- Improve multi-repository code retrieval, indexing throughput, semantic/vector retrieval, research alignment, and measured performance.
- Treat tools/self_iteration/cases.json as the target workload. Improve general parser, graph, retrieval, indexing, ranking, and service behavior instead of enumerating fixture strings.
- Any implementation candidate must update docs/zh/05-benchmarks/04-self-iteration-accepted-optimizations.md with algorithm, architecture, invariants, expected impact, and risks.

Constraints:
- Follow AGENTS.md and hard architecture constraints.
- Keep this self-iteration harness independent under tools/self_iteration.
- Do not create commits yourself; the harness owns accepted commits.

Workspace: {workspace}
Historical context: {best_summary}
Recent rejected v2 attempts:
{rejected}

Make one concrete candidate code change now.
"#,
        workspace = workspace.display(),
    )
}

fn recent_rejections(paths: &HistoryPaths) -> String {
    let Ok(runs) = load_runs(paths) else {
        return "No rejected v2 historical run with reasons yet.".to_owned();
    };
    let lines = runs
        .iter()
        .rev()
        .filter(|run| {
            !run.get("accepted")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false)
        })
        .take(3)
        .map(|run| {
            format!(
                "- run_id={} score={} reasons={}",
                value(run, "run_id"),
                value(run, "score"),
                run.get("reject_reasons")
                    .and_then(serde_json::Value::as_array)
                    .map(|items| {
                        items
                            .iter()
                            .filter_map(serde_json::Value::as_str)
                            .collect::<Vec<_>>()
                            .join("; ")
                    })
                    .unwrap_or_default()
            )
        })
        .collect::<Vec<_>>();
    if lines.is_empty() {
        "No rejected v2 historical run with reasons yet.".to_owned()
    } else {
        lines.join("\n")
    }
}

fn value(run: &serde_json::Value, name: &str) -> String {
    let value = run.get(name).unwrap_or(&serde_json::Value::Null);
    value
        .as_str()
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| value.to_string())
}

fn from_command(result: CommandResult) -> CodexResult {
    CodexResult {
        command: result.command,
        exit_code: result.exit_code,
        duration_ms: result.duration_ms,
        stdout: result.stdout,
        stderr: result.stderr,
    }
}
