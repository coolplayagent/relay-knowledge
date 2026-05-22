use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    command::{CommandResult, CommandSpec, run_command},
    config::{CategorySet, Config, DEFAULT_CODEX_MODEL, EvaluationCategory},
    history::{
        HistoryPaths, adopted, best_accepted_run_for_profile, best_accepted_run_for_workload,
        is_evaluate_run, load_runs,
    },
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
    if let Some(profile) = &config.codex_profile {
        command.extend(["-p".to_owned(), profile.clone()]);
    }
    let model = config.model.as_deref().unwrap_or(DEFAULT_CODEX_MODEL);
    command.extend(["-m".to_owned(), model.to_owned()]);
    command.extend([
        "-c".to_owned(),
        format!(
            "model_reasoning_effort=\"{}\"",
            config.codex_reasoning_effort
        ),
    ]);
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
    let category_focus_key = categories.map(CategorySet::focus_key);
    let best = best_accepted_run_for_workload(paths, profile, category_focus_key.as_deref())
        .ok()
        .flatten();
    let profile_best = best_accepted_run_for_profile(paths, profile).ok().flatten();
    let best_summary = best
        .as_ref()
        .map(run_brief)
        .unwrap_or_else(|| "none for this profile/category".to_owned());
    let profile_best_summary = profile_best
        .as_ref()
        .map(run_brief)
        .unwrap_or_else(|| "none for this profile".to_owned());
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
- Any implementation candidate must update docs/zh/05-benchmarks/04-self-iteration-accepted-optimizations.md with algorithm, architecture, invariants, expected impact, and risks. Evaluation-set-only candidates may instead update the matching benchmark specification document, such as docs/zh/05-benchmarks/06-c-cpp-syntax-self-iteration-evaluation.md or docs/zh/05-benchmarks/07-multilingual-syntax-self-iteration-evaluation.md.

Constraints:
- Follow AGENTS.md and hard architecture constraints.
- Keep this self-iteration harness independent under tools/self_iteration.
- Do not create commits yourself; the harness owns accepted commits.
- Code graph import hits whose targets are external or otherwise unresolved may
  use the product's internal grep fallback over the current indexed repository
  source. Treat `text_fallback` results and the external dependency diagnostic
  as source-text evidence, not as proof that the dependency library itself is
  indexed in the code graph.

Workspace: {workspace}
Evaluation profile: {profile}
Evaluation category focus: {category_focus}
Historical context:
- Best accepted for this profile/category: {best_summary}
- Best accepted for this profile: {profile_best_summary}
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

pub fn build_unattended_prompt(
    paths: &HistoryPaths,
    workspace: &Path,
    run_id: &str,
    profile: &str,
    category: EvaluationCategory,
    macro_explore: bool,
    cases_config: &Value,
) -> String {
    let categories = CategorySet::single(category);
    let category_focus_key = categories.focus_key();
    let best = best_accepted_run_for_workload(paths, profile, Some(&category_focus_key))
        .ok()
        .flatten();
    let profile_best = best_accepted_run_for_profile(paths, profile).ok().flatten();
    let latest =
        crate::history::previous_scored_run_for_workload(paths, profile, Some(&category_focus_key))
            .ok()
            .flatten();
    let feature_targets = if macro_explore {
        competitive_feature_targets(cases_config, 6)
    } else {
        "Macro targets omitted for short explore; use the current category and recent rejection summary."
            .to_owned()
    };
    let guardrails = if macro_explore {
        implementation_guardrails(cases_config, 5)
    } else {
        "Do not enumerate known queries, paths, repositories, symbols, or fixture strings."
            .to_owned()
    };
    let exploration_mode = if macro_explore {
        "macro_explore"
    } else {
        "explore"
    };
    let expected_change = if macro_explore {
        "Make a larger, general competitive-capability improvement in ranking, indexing, relationship extraction, query planning, context construction, or retrieval evidence. Prefer a coherent algorithmic change over a local tweak."
    } else {
        "Make one narrow, concrete candidate improvement for the current category."
    };
    format!(
        r#"You are running relay-knowledge unattended self-iteration run {run_id}.

Mode: {exploration_mode}
Workspace: {workspace}
Screen profile: {profile}
Category focus: {category_focus}

Goal:
- {expected_change}
- Preserve foundational capability, semantic/vector retrieval, stability, and existing competitive behavior.
- Update docs/zh/05-benchmarks/04-self-iteration-accepted-optimizations.md when code, tests, benchmark behavior, or harness policy changes.
- Do not create commits; the harness owns accepted commits.
- When code graph import targets are external or unresolved, relay-knowledge may
  use internal grep over the current indexed repository source and report
  `text_fallback` plus an external dependency diagnostic. Use that as local
  source-text evidence only; do not infer that the external dependency library
  has been indexed.

Baseline:
- Latest scored run: {latest_summary}
- Best accepted run for this profile/category: {best_summary}
- Best accepted run for this profile: {profile_best_summary}

Recent rejected attempts:
{rejected}

Relevant memory index:
{memory}

Competitive feature targets:
{feature_targets}

Implementation guardrails:
{guardrails}

Before editing, inspect only the files needed for this category. In your final notes, state the strategy used and why it should improve the category without fixture specialization.
"#,
        workspace = workspace.display(),
        category_focus = category.label(),
        latest_summary = latest
            .as_ref()
            .map(run_brief)
            .unwrap_or_else(|| "none for this profile/category".to_owned()),
        best_summary = best
            .as_ref()
            .map(run_brief)
            .unwrap_or_else(|| "none for this profile/category".to_owned()),
        profile_best_summary = profile_best
            .as_ref()
            .map(run_brief)
            .unwrap_or_else(|| "none for this profile".to_owned()),
        rejected = recent_rejections(paths),
        memory = progressive_memory_index(paths, if macro_explore { 5 } else { 3 }),
    )
}

fn run_brief(run: &Value) -> String {
    format!(
        "run_id={} score={} competitive={} reasons={}",
        value(run, "run_id"),
        value(run, "score"),
        value(run, "competitive_capability"),
        run.get("reject_reasons")
            .and_then(Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .filter_map(Value::as_str)
                    .collect::<Vec<_>>()
                    .join("; ")
            })
            .unwrap_or_default()
    )
}

fn competitive_feature_targets(cases_config: &Value, limit: usize) -> String {
    suite_strings(cases_config, "competitive_feature_targets", limit)
}

fn implementation_guardrails(cases_config: &Value, limit: usize) -> String {
    suite_strings(cases_config, "implementation_guardrails", limit)
}

fn suite_strings(cases_config: &Value, field: &str, limit: usize) -> String {
    let items = cases_config
        .get("research_judge_suite")
        .and_then(|suite| suite.get(field))
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .take(limit)
                .map(|item| format!("- {item}"))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    if items.is_empty() {
        "No research judge targets configured.".to_owned()
    } else {
        items.join("\n")
    }
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
    fn codex_command_defaults_to_gpt55_xhigh() {
        let config = Config::parse(vec![
            "once".to_owned(),
            "--workspace".to_owned(),
            "/tmp/relay-knowledge".to_owned(),
        ])
        .expect("config should parse");

        let command = build_codex_command(&config);

        assert_eq!(
            command,
            vec![
                "codex",
                "exec",
                "-C",
                "/tmp/relay-knowledge",
                "-m",
                "gpt-5.5",
                "-c",
                "model_reasoning_effort=\"xhigh\"",
                "-"
            ]
        );
    }

    #[test]
    fn codex_command_keeps_explicit_generation_overrides() {
        let config = Config::parse(vec![
            "once".to_owned(),
            "--workspace".to_owned(),
            "/tmp/relay-knowledge".to_owned(),
            "--yolo".to_owned(),
            "--codex-path".to_owned(),
            "/usr/local/bin/codex".to_owned(),
            "--model".to_owned(),
            "o3".to_owned(),
            "--codex-reasoning-effort=high".to_owned(),
            "--codex-profile".to_owned(),
            "self-iteration".to_owned(),
        ])
        .expect("config should parse");

        let command = build_codex_command(&config);

        assert_eq!(
            command,
            vec![
                "/usr/local/bin/codex",
                "-a",
                "never",
                "exec",
                "-C",
                "/tmp/relay-knowledge",
                "--dangerously-bypass-approvals-and-sandbox",
                "-s",
                "danger-full-access",
                "-p",
                "self-iteration",
                "-m",
                "o3",
                "-c",
                "model_reasoning_effort=\"high\"",
                "-"
            ]
        );
    }

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
        assert!(prompt.contains("external dependency diagnostic"));
        assert!(prompt.contains("source-text evidence"));
    }

    #[test]
    fn unattended_prompt_explains_external_import_grep_fallback() {
        let workspace = temp_workspace("codex-unattended-prompt");
        let paths = HistoryPaths::new(&workspace);
        paths.ensure().expect("history paths");

        let prompt = build_unattended_prompt(
            &paths,
            &workspace,
            "run-test",
            "fast",
            EvaluationCategory::Competitive,
            false,
            &json!({}),
        );

        assert!(prompt.contains("external dependency diagnostic"));
        assert!(prompt.contains("external dependency library"));
        assert!(prompt.contains("text_fallback"));
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
