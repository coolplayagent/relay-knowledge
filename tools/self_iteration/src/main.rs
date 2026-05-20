mod cases;
mod codex;
mod command;
mod config;
mod evaluator;
mod git_ops;
mod history;
mod history_synthesis;
mod memory;
mod scoring;
mod unattended;

use std::time::{SystemTime, UNIX_EPOCH};

use config::{Config, Mode, Strategy};
use git_ops::PatchSnapshot;
use scoring::{EvaluationObservation, GateObservation};

fn main() {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    let exit_code = match Config::parse(args).and_then(run) {
        Ok(code) => code,
        Err(error) => {
            eprintln!("[self-iterate] {error}");
            1
        }
    };
    std::process::exit(exit_code);
}

fn run(mut config: Config) -> Result<i32, String> {
    config.workspace = config
        .workspace
        .canonicalize()
        .map_err(|error| format!("invalid workspace {}: {error}", config.workspace.display()))?;
    let paths = history::HistoryPaths::new(&config.workspace);
    paths.ensure()?;
    match config.mode {
        Mode::Chart => {
            let (csv, svg) = history::export_history(&paths)?;
            println!("score csv: {}", csv.display());
            println!("score svg: {}", svg.display());
            Ok(0)
        }
        Mode::Evaluate => run_evaluate(&config, &paths),
        Mode::Once => {
            config.max_iterations = Some(1);
            run_loop(&config, &paths)
        }
        Mode::Loop => run_loop(&config, &paths),
    }
}

fn run_loop(config: &Config, paths: &history::HistoryPaths) -> Result<i32, String> {
    if config.strategy == Strategy::UnattendedLayered {
        return unattended::run_unattended_layered_loop(config, paths);
    }
    if config.max_iterations == Some(0) || config.stop_after_accepted == Some(0) {
        return Ok(0);
    }
    if !config.use_current_candidate {
        git_ops::ensure_clean_worktree(&config.workspace)?;
    }
    let mut iteration = 0usize;
    let mut accepted_count = 0usize;
    loop {
        if config.max_iterations.is_some_and(|max| iteration >= max) {
            return Ok(0);
        }
        if config
            .stop_after_accepted
            .is_some_and(|max| accepted_count >= max)
        {
            return Ok(0);
        }
        iteration += 1;
        println!("[self-iterate] iteration {iteration} starting");
        match run_generation_iteration(config, paths) {
            Ok(true) => accepted_count += 1,
            Ok(false) => {}
            Err(error) if config.fail_fast => return Err(error),
            Err(error) => {
                eprintln!("[self-iterate] iteration failed: {error}");
                if config.max_iterations.is_some_and(|max| iteration >= max) {
                    return Ok(1);
                }
            }
        }
        git_ops::sleep_seconds(config.sleep_seconds);
    }
}

fn run_evaluate(config: &Config, paths: &history::HistoryPaths) -> Result<i32, String> {
    let run_id = new_manual_evaluate_run_id();
    let patch = git_ops::capture_patch(&config.workspace, paths, &run_id, "HEAD")?;
    let evaluation = evaluate_candidate_for_patch(config, paths, &run_id, &patch)?;
    let record = persist_scored_run(config, paths, &run_id, &patch, None, &evaluation, None)?;
    print_score(&record);
    Ok(if record["score"].as_f64().unwrap_or(0.0) > 0.0 {
        0
    } else {
        1
    })
}

fn run_generation_iteration(
    config: &Config,
    paths: &history::HistoryPaths,
) -> Result<bool, String> {
    let run_id = new_run_id();
    if !config.use_current_candidate {
        git_ops::ensure_clean_worktree(&config.workspace)?;
    }
    let base_ref = git_ops::current_head(&config.workspace)?;
    let codex_result = if config.use_current_candidate {
        println!("[self-iterate] using current working tree as candidate");
        None
    } else {
        let prompt = codex::build_prompt(
            paths,
            &config.workspace,
            &run_id,
            &config.profile,
            config.categories.as_ref(),
        );
        let result = codex::run_codex(config, &prompt);
        println!(
            "[self-iterate] codex exit={} duration_ms={}",
            result.exit_code, result.duration_ms
        );
        Some(result)
    };
    let patch = git_ops::capture_patch(&config.workspace, paths, &run_id, &base_ref)?;
    if codex_result
        .as_ref()
        .is_some_and(|result| !result.succeeded())
    {
        let observation = EvaluationObservation {
            gates: vec![GateObservation {
                name: "codex_generation".to_owned(),
                passed: false,
                duration_ms: codex_result
                    .as_ref()
                    .map(|result| result.duration_ms)
                    .unwrap_or(0),
                message: codex_result
                    .as_ref()
                    .map(|result| command::last_output_line(&result.stdout, &result.stderr))
                    .unwrap_or_default(),
            }],
            cases: Vec::new(),
            metrics: Vec::new(),
            generated_diff: patch.has_diff(),
        };
        let evaluation = evaluator::EvaluationRun {
            observation,
            report: serde_json::json!({"generated_diff": patch.has_diff()}),
        };
        let record = persist_scored_run(
            config,
            paths,
            &run_id,
            &patch,
            codex_result.as_ref(),
            &evaluation,
            None,
        )?;
        git_ops::reject_candidate(&config.workspace, &patch, !config.use_current_candidate)?;
        print_score(&record);
        return Ok(false);
    }
    if !patch.has_diff() {
        let evaluation = evaluator::EvaluationRun {
            observation: EvaluationObservation::empty(false),
            report: serde_json::json!({"generated_diff": false}),
        };
        let record = persist_scored_run(
            config,
            paths,
            &run_id,
            &patch,
            codex_result.as_ref(),
            &evaluation,
            None,
        )?;
        print_score(&record);
        return Ok(false);
    }
    println!("[self-iterate] candidate patch: {}", patch.path.display());
    let mut evaluation = evaluate_candidate_for_patch(config, paths, &run_id, &patch)?;
    apply_candidate_documentation_gate(&mut evaluation, &patch);
    let category_focus = config.category_focus_key();
    let previous_run = history::previous_scored_run_for_workload(
        paths,
        &config.profile,
        category_focus.as_deref(),
    )?;
    let score = scoring::score_evaluation(&evaluation.observation, previous_run.as_ref());
    let commit = if score.accepted {
        write_adopted_optimization_document(
            &config.workspace,
            &run_id,
            &patch,
            &score,
            &evaluation,
        )?;
        Some(git_ops::commit_candidate(
            &config.workspace,
            config.commit_message.as_deref(),
            score.score,
            &base_ref,
        )?)
    } else {
        None
    };
    let record = persist_scored_run_with_score(PersistInput {
        config,
        paths,
        run_id: &run_id,
        patch: &patch,
        codex: codex_result.as_ref(),
        evaluation: &evaluation,
        commit: commit.as_deref(),
        score: &score,
        previous_run: previous_run.as_ref(),
        metadata: None,
    })?;
    if record["accepted"].as_bool().unwrap_or(false) {
        println!(
            "[self-iterate] accepted commit={}",
            commit.unwrap_or_default()
        );
        print_score(&record);
        Ok(true)
    } else {
        git_ops::reject_candidate(&config.workspace, &patch, !config.use_current_candidate)?;
        println!("[self-iterate] rejected candidate and restored working tree");
        print_score(&record);
        Ok(false)
    }
}

pub(crate) fn evaluate_candidate_for_patch(
    config: &Config,
    paths: &history::HistoryPaths,
    run_id: &str,
    patch: &PatchSnapshot,
) -> Result<evaluator::EvaluationRun, String> {
    let cases_config =
        cases::load_cases(&config.workspace.join("tools/self_iteration/cases.json"))?;
    evaluator::evaluate_candidate(
        config,
        paths,
        run_id,
        &cases_config,
        patch.has_diff(),
        &patch.diff,
    )
}

pub(crate) fn apply_candidate_documentation_gate(
    evaluation: &mut evaluator::EvaluationRun,
    patch: &PatchSnapshot,
) {
    let changed_paths = git_ops::changed_paths_from_diff(&patch.diff);
    let requires_docs = changed_paths
        .iter()
        .any(|path| !path.starts_with("docs/") && !path.ends_with(".md"));
    let documented = changed_paths
        .iter()
        .any(|path| path == "docs/zh/05-benchmarks/04-self-iteration-accepted-optimizations.md");
    let gate = GateObservation {
        name: "self_iteration_algorithm_documentation".to_owned(),
        passed: !requires_docs || documented,
        duration_ms: 0,
        message: if !requires_docs {
            "documentation not required for documentation-only candidate".to_owned()
        } else if documented {
            "accepted optimization document updated".to_owned()
        } else {
            "missing candidate algorithm and architecture notes".to_owned()
        },
    };
    evaluation.observation.gates.push(gate.clone());
    if let Some(gates) = evaluation
        .report
        .get_mut("gates")
        .and_then(serde_json::Value::as_array_mut)
    {
        gates.push(serde_json::to_value(gate).expect("gate should serialize"));
    }
}

fn persist_scored_run(
    config: &Config,
    paths: &history::HistoryPaths,
    run_id: &str,
    patch: &PatchSnapshot,
    codex: Option<&codex::CodexResult>,
    evaluation: &evaluator::EvaluationRun,
    commit: Option<&str>,
) -> Result<serde_json::Value, String> {
    let category_focus = config.category_focus_key();
    let previous = history::previous_scored_run_for_workload(
        paths,
        &config.profile,
        category_focus.as_deref(),
    )?;
    let score = scoring::score_evaluation(&evaluation.observation, previous.as_ref());
    persist_scored_run_with_score(PersistInput {
        config,
        paths,
        run_id,
        patch,
        codex,
        evaluation,
        commit,
        score: &score,
        previous_run: previous.as_ref(),
        metadata: None,
    })
}

pub(crate) struct PersistInput<'a> {
    pub(crate) config: &'a Config,
    pub(crate) paths: &'a history::HistoryPaths,
    pub(crate) run_id: &'a str,
    pub(crate) patch: &'a PatchSnapshot,
    pub(crate) codex: Option<&'a codex::CodexResult>,
    pub(crate) evaluation: &'a evaluator::EvaluationRun,
    pub(crate) commit: Option<&'a str>,
    pub(crate) score: &'a scoring::ScoreBreakdown,
    pub(crate) previous_run: Option<&'a serde_json::Value>,
    pub(crate) metadata: Option<&'a serde_json::Value>,
}

pub(crate) fn persist_scored_run_with_score(
    input: PersistInput<'_>,
) -> Result<serde_json::Value, String> {
    let timestamp = unix_timestamp_string();
    let patch = patch_metadata(input.patch);
    let optimization_plan = optimization_plan(input.patch, input.score, input.codex);
    let category_focus = input.config.category_focus_key();
    let selected_categories = input.config.selected_category_labels();
    let selected_categories_report = selected_categories_value(&selected_categories);
    let comparison_baseline = comparison_baseline(
        input.paths,
        &input.config.profile,
        category_focus.as_deref(),
        input.previous_run,
    )?;
    let report = serde_json::json!({
        "run_id": input.run_id,
        "profile": input.config.profile,
        "strategy": input.config.strategy.label(),
        "category_focus": category_focus.as_deref(),
        "selected_categories": selected_categories_report,
        "unattended": input.metadata,
        "workspace": input.config.workspace.display().to_string(),
        "patch": patch,
        "optimization_plan": optimization_plan,
        "comparison_baseline": comparison_baseline,
        "score_accepted": input.score.accepted,
        "committed": input.commit.is_some(),
        "adoption_status": if input.commit.is_some() {
            "committed"
        } else if input.score.accepted {
            "would_accept"
        } else {
            "rejected"
        },
        "codex": input.codex.map(codex::CodexResult::serializable),
        "evaluation": input.evaluation.report,
        "score": input.score,
        "degradations": input.score.degradations,
        "improvements": input.score.improvements,
    });
    let report_path = history::write_report(input.paths, input.run_id, &report)?;
    let record = history::make_run_record(history::RunRecordInput {
        run_id: input.run_id,
        timestamp: &timestamp,
        profile: &input.config.profile,
        category_focus: category_focus.as_deref(),
        selected_categories: &selected_categories,
        report_path: &report_path,
        commit: input.commit,
        score: input.score,
        observation: &input.evaluation.observation,
    });
    let mut record = record;
    if let Some(object) = record.as_object_mut() {
        object.insert("patch".to_owned(), patch);
        object.insert("optimization_plan".to_owned(), optimization_plan);
        object.insert("comparison_baseline".to_owned(), comparison_baseline);
        object.insert(
            "strategy".to_owned(),
            serde_json::json!(input.config.strategy.label()),
        );
        if let Some(metadata) = input.metadata.and_then(serde_json::Value::as_object) {
            for key in [
                "layer",
                "parent_run_id",
                "promoted_from_run_id",
                "macro_trigger",
                "promotion_decision",
                "wall_clock_started_at",
                "wall_clock_elapsed_seconds",
            ] {
                if let Some(value) = metadata.get(key) {
                    object.insert(key.to_owned(), value.clone());
                }
            }
        }
    }
    if !history::is_evaluate_run(&record) {
        memory::write_run_memory(input.paths, &record)?;
    }
    history::append_run(input.paths, &record)?;
    history::export_history(input.paths)?;
    Ok(record)
}

fn selected_categories_value(selected_categories: &[&str]) -> serde_json::Value {
    if selected_categories.is_empty() {
        serde_json::Value::Null
    } else {
        serde_json::json!(selected_categories)
    }
}

fn patch_metadata(patch: &PatchSnapshot) -> serde_json::Value {
    serde_json::json!({
        "path": patch.path.display().to_string(),
        "sha256": patch.sha256,
        "bytes": patch.diff.len(),
        "has_diff": patch.has_diff(),
        "base_ref": patch.base_ref,
    })
}

fn optimization_plan(
    patch: &PatchSnapshot,
    score: &scoring::ScoreBreakdown,
    codex: Option<&codex::CodexResult>,
) -> serde_json::Value {
    let codex_notes = codex.map(|result| {
        memory::compact_prompt_text(&format!("{}\n{}", result.stdout, result.stderr), 1200)
    });
    serde_json::json!({
        "changed_paths": git_ops::changed_paths_from_diff(&patch.diff),
        "key_improvements": memory::compact_score_changes(&score.improvements, 8),
        "known_degradations": memory::compact_score_changes(&score.degradations, 8),
        "reject_reasons": score.reject_reasons,
        "codex_notes": codex_notes,
    })
}

fn comparison_baseline(
    paths: &history::HistoryPaths,
    profile: &str,
    category_focus: Option<&str>,
    previous_run: Option<&serde_json::Value>,
) -> Result<serde_json::Value, String> {
    let best_accepted = history::best_accepted_run_for_workload(paths, profile, category_focus)?;
    Ok(serde_json::json!({
        "comparison_kind": "latest_scored_workload_run",
        "profile": profile,
        "category_focus": category_focus,
        "latest_run_id": previous_run.and_then(|run| run.get("run_id")).and_then(serde_json::Value::as_str),
        "latest_score": previous_run.and_then(|run| run.get("score")).and_then(serde_json::Value::as_f64),
        "latest_accepted": previous_run.map(history::adopted),
        "best_accepted_run_id": best_accepted.as_ref().and_then(|run| run.get("run_id")).and_then(serde_json::Value::as_str),
        "best_accepted_score": best_accepted.as_ref().and_then(|run| run.get("score")).and_then(serde_json::Value::as_f64),
    }))
}

pub(crate) fn write_adopted_optimization_document(
    workspace: &std::path::Path,
    run_id: &str,
    patch: &PatchSnapshot,
    score: &scoring::ScoreBreakdown,
    evaluation: &evaluator::EvaluationRun,
) -> Result<(), String> {
    let path = workspace.join("docs/zh/05-benchmarks/04-self-iteration-accepted-optimizations.md");
    let case_count = evaluation.observation.cases.len();
    let passed_cases = evaluation
        .observation
        .cases
        .iter()
        .filter(|case| case.passed)
        .count();
    let metrics = evaluation
        .observation
        .metrics
        .iter()
        .filter(|metric| metric.name.ends_with("_ms"))
        .take(8)
        .map(|metric| format!("{}={:.0}ms", metric.name, metric.value))
        .collect::<Vec<_>>()
        .join("; ");
    let entry = format!(
        "\n## {run_id}\n\n- patch: `{}`\n- score: {:.6} (foundational={:.6}, competitive={:.6}, accuracy={:.6}, semantic_vector={:.6}, research_judge={}, performance={:.6}, stability={:.6})\n- cases: {passed_cases}/{case_count} passed\n- changed paths: {}\n- key improvements: {}\n- known degradations: {}\n- latency metrics: {}\n\nAdopted optimization notes:\n\nRust self-iteration v2 accepted this candidate through the independent tools/self_iteration harness. The candidate is expected to improve the general retrieval, indexing, evaluation, or harness behavior described by the changed paths and recorded metrics.\n\n",
        patch.path.display(),
        score.score,
        score.foundational_capability,
        score.competitive_capability,
        score.accuracy,
        score.semantic_vector,
        score
            .research_judge
            .map(|value| format!("{value:.6}"))
            .unwrap_or_else(|| "n/a".to_owned()),
        score.performance,
        score.stability,
        git_ops::changed_paths_from_diff(&patch.diff)
            .into_iter()
            .map(|path| format!("`{path}`"))
            .collect::<Vec<_>>()
            .join(", "),
        compact_changes(&score.improvements),
        compact_changes(&score.degradations),
        if metrics.is_empty() {
            "none recorded"
        } else {
            &metrics
        },
    );
    std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .and_then(|mut file| {
            use std::io::Write;
            file.write_all(entry.as_bytes())
        })
        .map_err(|error| format!("failed to append {}: {error}", path.display()))
}

fn compact_changes(changes: &[serde_json::Value]) -> String {
    let text = changes
        .iter()
        .take(8)
        .map(|item| {
            format!(
                "{}:{} {}->{}",
                item.get("kind")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("change"),
                item.get("name")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("unknown"),
                item.get("previous")
                    .map(ToString::to_string)
                    .unwrap_or_default(),
                item.get("current")
                    .map(ToString::to_string)
                    .unwrap_or_default()
            )
        })
        .collect::<Vec<_>>()
        .join("; ");
    if text.is_empty() {
        "none recorded".to_owned()
    } else {
        text
    }
}

pub(crate) fn print_score(record: &serde_json::Value) {
    let score_accepted = record
        .get("score_accepted")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or_else(|| record["accepted"].as_bool().unwrap_or(false));
    let status = if record["accepted"].as_bool().unwrap_or(false) {
        "accepted"
    } else if score_accepted {
        "would_accept"
    } else {
        "rejected"
    };
    println!(
        "[self-iterate] {status} score={:.6} foundational={:.6} competitive={:.6} accuracy={:.6} semantic_vector={:.6} research_judge={} performance={:.6} stability={:.6}",
        number(record, "score"),
        number(record, "foundational_capability"),
        number(record, "competitive_capability"),
        number(record, "accuracy"),
        number(record, "semantic_vector"),
        record
            .get("research_judge")
            .and_then(serde_json::Value::as_f64)
            .map(|value| format!("{value:.6}"))
            .unwrap_or_else(|| "n/a".to_owned()),
        number(record, "performance"),
        number(record, "stability"),
    );
    let reasons = record
        .get("reject_reasons")
        .and_then(serde_json::Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(serde_json::Value::as_str)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    if !reasons.is_empty() {
        println!("[self-iterate] reasons: {}", reasons.join("; "));
    } else if status == "would_accept" {
        println!(
            "[self-iterate] reasons: score passed, but this mode does not create an accepted git commit"
        );
    }
    if let Some(baseline) = record.get("comparison_baseline") {
        let latest = baseline
            .get("latest_run_id")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("none");
        let latest_score = baseline
            .get("latest_score")
            .and_then(serde_json::Value::as_f64)
            .map(|score| format!("{score:.6}"))
            .unwrap_or_else(|| "n/a".to_owned());
        let best = baseline
            .get("best_accepted_run_id")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("none");
        let best_score = baseline
            .get("best_accepted_score")
            .and_then(serde_json::Value::as_f64)
            .map(|score| format!("{score:.6}"))
            .unwrap_or_else(|| "n/a".to_owned());
        println!(
            "[self-iterate] comparison baseline latest={latest} score={latest_score}; best_accepted={best} score={best_score}"
        );
    }
    println!(
        "[self-iterate] report: {}",
        record["report"].as_str().unwrap_or("")
    );
}

pub(crate) fn number(record: &serde_json::Value, name: &str) -> f64 {
    record
        .get(name)
        .and_then(serde_json::Value::as_f64)
        .unwrap_or(0.0)
}

fn new_run_id() -> String {
    format!("run-{}", unix_timestamp_string())
}

pub(crate) fn new_layer_run_id(layer: &str) -> String {
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos().to_string())
        .unwrap_or_else(|_| "0".to_owned());
    format!("run-{suffix}-{layer}")
}

fn new_manual_evaluate_run_id() -> String {
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos().to_string())
        .unwrap_or_else(|_| "0".to_owned());
    format!("manual-evaluate-{suffix}")
}

pub(crate) fn unix_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

fn unix_timestamp_string() -> String {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs().to_string())
        .unwrap_or_else(|_| "0".to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manual_evaluate_run_id_uses_unique_patch_namespace() {
        let run_id = new_manual_evaluate_run_id();

        assert!(run_id.starts_with("manual-evaluate-"));
        assert!(run_id.len() > "manual-evaluate-".len());
    }
}
