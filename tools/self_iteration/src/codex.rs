use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::{
    command::{CommandResult, CommandSpec, run_command},
    config::{CategorySet, Config},
    history::{HistoryPaths, adopted, best_accepted_run_for_profile, is_evaluate_run, load_runs},
    history_synthesis::synthesize_history,
    memory::{
        historical_patch_memory_index, progressive_memory_index, rejection_recovery_memory_review,
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

pub fn build_prompt(
    paths: &HistoryPaths,
    workspace: &Path,
    run_id: &str,
    profile: &str,
    categories: Option<&CategorySet>,
) -> String {
    let best = best_accepted_run_for_profile(paths, profile).ok().flatten();
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
    let recovery_memory = rejection_recovery_memory_review(paths, 5);
    let progressive_memory = progressive_memory_index(paths, 12);
    let patch_memory = historical_patch_memory_index(paths, 12);
    let history_synthesis = synthesize_history(paths, profile);
    let category_focus = categories
        .map(|items| items.labels().join(", "))
        .unwrap_or_else(|| "profile default workload".to_owned());
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
Evaluation profile: {profile}
Evaluation category focus: {category_focus}
Historical context: {best_summary}
Historical synthesis:
{history_synthesis}

Recent rejected v2 attempts:
{rejected}

Rejected recovery memory:
{recovery_memory}

Progressive memory index:
{progressive_memory}

Historical patch memory index:
{patch_memory}

Make one concrete candidate code change now. Before editing, use the historical synthesis to decide whether this should be a broader algorithmic change rather than another small local tweak. In your final notes, state which accepted strategy or rejected pattern the candidate builds on or avoids.
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
        .filter(|run| !adopted(run) && !is_evaluate_run(run))
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

#[cfg(test)]
mod tests {
    use std::{
        fs,
        time::{SystemTime, UNIX_EPOCH},
    };

    use serde_json::{Value, json};

    use super::*;

    #[test]
    fn prompt_includes_direct_history_synthesis() {
        let workspace = temp_workspace("codex-prompt");
        let paths = HistoryPaths::new(&workspace);
        paths.ensure().expect("history paths");
        let runs = [
            json!({
                "run_id": "accepted",
                "timestamp": "1",
                "profile": "fast",
                "accepted": true,
                "score_accepted": true,
                "committed": true,
                "commit": "abc1234",
                "score": 0.8,
                "foundational_capability": 1.0,
                "competitive_capability": 0.8,
                "accuracy": 0.9,
                "semantic_vector": 0.0,
                "performance": 0.8,
                "stability": 1.0,
                "reject_reasons": [],
                "improvements": [{"kind": "score_component", "name": "score", "previous": 0.7, "current": 0.8}],
                "degradations": [],
                "optimization_plan": {"changed_paths": ["src/query.rs"]}
            }),
            json!({
                "run_id": "rejected",
                "timestamp": "2",
                "profile": "fast",
                "accepted": false,
                "score": 0.79,
                "foundational_capability": 1.0,
                "competitive_capability": 0.8,
                "accuracy": 0.9,
                "semantic_vector": 0.0,
                "performance": 0.7,
                "stability": 1.0,
                "reject_reasons": ["candidate did not improve score or tracked objectives beyond epsilon"],
                "improvements": [{"kind": "metric", "name": "relay_teams_query_p95_ms", "previous": 8000.0, "current": 7000.0}],
                "degradations": [{"kind": "score_component", "name": "score", "previous": 0.8, "current": 0.79}],
                "optimization_plan": {"changed_paths": ["src/query.rs"]}
            }),
        ];
        fs::write(
            &paths.runs_jsonl,
            runs.iter()
                .map(Value::to_string)
                .collect::<Vec<_>>()
                .join("\n"),
        )
        .expect("runs");

        let prompt = build_prompt(&paths, &workspace, "run-test", "fast", None);

        assert!(prompt.contains("Historical synthesis:"));
        assert!(prompt.contains("Latest scored baseline: rejected"));
        assert!(prompt.contains("Best accepted run: accepted"));
        assert!(prompt.contains("Local improvements that did not win"));
        assert!(prompt.contains("broader algorithmic change"));
    }

    fn temp_workspace(prefix: &str) -> std::path::PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time")
            .as_nanos();
        let workspace = std::env::temp_dir().join(format!("{prefix}-{unique}"));
        fs::create_dir_all(workspace.join(".git")).expect("workspace");
        workspace
    }
}
