mod cases;
mod codex;
mod command;
mod config;
mod evaluator;
mod git_ops;
mod history;
mod scoring;

use std::time::{SystemTime, UNIX_EPOCH};

use config::{Config, Mode};
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
    let patch = git_ops::capture_patch(&config.workspace, paths, "manual-evaluate", "HEAD")?;
    let evaluation = evaluate_candidate_for_patch(config, paths, "manual-evaluate", &patch)?;
    let record = persist_scored_run(
        config,
        paths,
        "manual-evaluate",
        &patch,
        None,
        &evaluation,
        None,
    )?;
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
        let prompt = codex::build_prompt(paths, &config.workspace, &run_id);
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
    let previous_run = history::previous_scored_run(paths)?;
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

fn evaluate_candidate_for_patch(
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

fn apply_candidate_documentation_gate(
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
    let previous = history::previous_scored_run(paths)?;
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
    })
}

struct PersistInput<'a> {
    config: &'a Config,
    paths: &'a history::HistoryPaths,
    run_id: &'a str,
    patch: &'a PatchSnapshot,
    codex: Option<&'a codex::CodexResult>,
    evaluation: &'a evaluator::EvaluationRun,
    commit: Option<&'a str>,
    score: &'a scoring::ScoreBreakdown,
}

fn persist_scored_run_with_score(input: PersistInput<'_>) -> Result<serde_json::Value, String> {
    let timestamp = unix_timestamp_string();
    let report = serde_json::json!({
        "run_id": input.run_id,
        "workspace": input.config.workspace.display().to_string(),
        "patch": patch_metadata(input.patch),
        "codex": input.codex.map(codex::CodexResult::serializable),
        "evaluation": input.evaluation.report,
        "score": input.score,
        "degradations": input.score.degradations,
        "improvements": input.score.improvements,
    });
    let report_path = history::write_report(input.paths, input.run_id, &report)?;
    let record = history::make_run_record(
        input.run_id,
        &timestamp,
        &report_path,
        input.commit,
        input.score,
        &input.evaluation.observation,
    );
    history::append_run(input.paths, &record)?;
    history::export_history(input.paths)?;
    Ok(record)
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

fn write_adopted_optimization_document(
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

fn print_score(record: &serde_json::Value) {
    let status = if record["accepted"].as_bool().unwrap_or(false) {
        "accepted"
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
    }
    println!(
        "[self-iterate] report: {}",
        record["report"].as_str().unwrap_or("")
    );
}

fn number(record: &serde_json::Value, name: &str) -> f64 {
    record
        .get(name)
        .and_then(serde_json::Value::as_f64)
        .unwrap_or(0.0)
}

fn new_run_id() -> String {
    format!("run-{}", unix_timestamp_string())
}

fn unix_timestamp_string() -> String {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs().to_string())
        .unwrap_or_else(|_| "0".to_owned())
}
