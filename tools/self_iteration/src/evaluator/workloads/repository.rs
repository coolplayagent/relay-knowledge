use std::{fs, path::Path, process::Command};

use serde_json::Value;

use crate::{
    cases::{string_or, string_vec},
    command::{CommandResult, CommandSpec},
    scoring::MetricObservation,
};

use super::super::{
    fixtures::{prepare_incremental_repository_change, prepare_repository_path},
    runtime::{
        concurrency::{parallel_map, run_limited, run_writer_limited},
        contracts::{EvalRuntime, RepoReport},
        reporting::{
            budget, elastic_budget_enabled, parse_json_output, push_latency_metrics, repo_report,
        },
    },
};
use super::{
    cli_cases::{
        incremental_update_command, query_command, register_command, software_query_command,
    },
    repository_scoring::{score_query_case, score_software_case},
    selection::guardrail_gate_from_case,
};

pub(in crate::evaluator) fn evaluate_repository(
    runtime: &EvalRuntime,
    run_home: &Path,
    repo_name: &str,
    repo_config: &Value,
    repo_cases: Vec<Value>,
    software_cases: Vec<Value>,
) -> Result<RepoReport, String> {
    let owned_runtime = isolated_repository_runtime(runtime, run_home, repo_name, repo_config)?;
    let runtime = &owned_runtime;
    let alias = string_or(repo_config, "alias", repo_name);
    let ref_selector = string_or(repo_config, "ref", "HEAD");
    let scope = string_or(repo_config, "scope", "all").to_owned();
    let mut commands = Vec::new();
    let mut cases = Vec::new();
    let mut guardrail_gates = Vec::new();
    let mut metrics = Vec::new();
    let (path, setup_commands) =
        prepare_repository_path(runtime, run_home, repo_name, repo_config)?;
    let elastic_repo_config = with_observed_file_count(repo_config, &path);
    let setup_passed = setup_commands.iter().all(CommandResult::passed);
    commands.extend(setup_commands);
    eprintln!(
        "[self-iterate] repository start name={} alias={} path={} scope={} query_cases={} software_query_cases={}",
        repo_name,
        alias,
        path.display(),
        scope,
        repo_cases.len(),
        software_cases.len()
    );
    if !setup_passed {
        return Ok(repo_report(
            repo_name,
            scope,
            commands,
            cases,
            metrics,
            Value::Null,
        ));
    }
    if !path.exists() {
        commands.push(CommandResult {
            name: format!("{repo_name}_repository_exists"),
            command: vec![
                "test".to_owned(),
                "-d".to_owned(),
                path.display().to_string(),
            ],
            exit_code: 1,
            duration_ms: 0,
            stdout: String::new(),
            stderr: format!("repository path is missing: {}", path.display()),
        });
        return Ok(repo_report(
            repo_name,
            scope,
            commands,
            cases,
            metrics,
            Value::Null,
        ));
    }
    if scope != "all" {
        commands.push(CommandResult {
            name: format!("{repo_name}_scope_is_all"),
            command: vec!["validate".to_owned(), "scope".to_owned(), scope.clone()],
            exit_code: 1,
            duration_ms: 0,
            stdout: String::new(),
            stderr: format!("self-iteration repositories must use full scope=all, got: {scope}"),
        });
        return Ok(repo_report(
            repo_name,
            scope,
            commands,
            cases,
            metrics,
            Value::Null,
        ));
    }
    let register = run_writer_limited(
        runtime,
        CommandSpec::new(
            format!("{repo_name}_register"),
            register_command(
                &runtime.binary,
                &path,
                (!repo_config
                    .get("register_without_alias")
                    .and_then(Value::as_bool)
                    .unwrap_or(false))
                .then_some(alias),
            ),
            &runtime.workspace,
            Some(runtime.env.clone()),
            elastic_timeout_seconds(
                runtime.timeout,
                &elastic_repo_config,
                "register_index_budget_ms",
            ),
        ),
    );
    commands.push(register.clone());
    if !register.passed() {
        return Ok(repo_report(
            repo_name,
            scope,
            commands,
            cases,
            metrics,
            Value::Null,
        ));
    }
    for additional_alias in string_vec(repo_config, "additional_aliases") {
        let additional_register = run_writer_limited(
            runtime,
            CommandSpec::new(
                format!("{repo_name}_register_alias_{additional_alias}"),
                register_command(&runtime.binary, &path, Some(&additional_alias)),
                &runtime.workspace,
                Some(runtime.env.clone()),
                runtime.timeout,
            ),
        );
        commands.push(additional_register.clone());
        if !additional_register.passed() {
            return Ok(repo_report(
                repo_name,
                scope,
                commands,
                cases,
                metrics,
                Value::Null,
            ));
        }
    }
    let index = run_writer_limited(
        runtime,
        CommandSpec::new(
            format!("{repo_name}_index"),
            vec![
                runtime.binary.display().to_string(),
                "repo".to_owned(),
                "index".to_owned(),
                alias.to_owned(),
                "--ref".to_owned(),
                ref_selector.to_owned(),
                "--format".to_owned(),
                "json".to_owned(),
            ],
            &runtime.workspace,
            Some(runtime.env.clone()),
            elastic_timeout_seconds(runtime.timeout, &elastic_repo_config, "index_budget_ms"),
        ),
    );
    let mut index_json = parse_json_output(&index.stdout);
    metrics.push(MetricObservation {
        name: format!("{repo_name}_cold_index_ms"),
        value: index.duration_ms as f64,
        budget: budget(&elastic_repo_config, "index_budget_ms"),
        lower_is_better: true,
        key: true,
    });
    metrics.push(MetricObservation {
        name: format!("{repo_name}_cold_register_index_ms"),
        value: (register.duration_ms + index.duration_ms) as f64,
        budget: budget(&elastic_repo_config, "register_index_budget_ms"),
        lower_is_better: true,
        key: true,
    });
    commands.push(index.clone());
    if !index.passed() {
        return Ok(repo_report(
            repo_name, scope, commands, cases, metrics, index_json,
        ));
    }
    if let Some(validation) = cold_index_completion_validation(repo_name, repo_config, &index_json)
    {
        let passed = validation.passed();
        commands.push(validation);
        if !passed {
            return Ok(repo_report(
                repo_name, scope, commands, cases, metrics, index_json,
            ));
        }
    }

    if let Some(incremental) =
        prepare_incremental_repository_change(runtime, repo_name, &path, repo_config)?
    {
        let setup_passed = incremental.commands.iter().all(CommandResult::passed)
            && !incremental.base_ref.is_empty()
            && !incremental.head_ref.is_empty();
        commands.extend(incremental.commands);
        if !setup_passed {
            return Ok(repo_report(
                repo_name, scope, commands, cases, metrics, index_json,
            ));
        }
        let incremental_via_full_index = repo_config
            .get("incremental_via_full_index")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let incremental_command = if incremental_via_full_index {
            vec![
                runtime.binary.display().to_string(),
                "repo".to_owned(),
                "index".to_owned(),
                alias.to_owned(),
                "--ref".to_owned(),
                incremental.head_ref.clone(),
                "--format".to_owned(),
                "json".to_owned(),
            ]
        } else {
            incremental_update_command(
                &runtime.binary,
                alias,
                &incremental.base_ref,
                &incremental.head_ref,
            )
        };
        let metric_suffix = if incremental_via_full_index {
            "initialization_incremental_index_ms"
        } else {
            "incremental_index_ms"
        };
        let budget_name = if incremental_via_full_index {
            "initialization_incremental_index_budget_ms"
        } else {
            "incremental_index_budget_ms"
        };
        let update = run_writer_limited(
            runtime,
            CommandSpec::new(
                format!("{repo_name}_incremental_index"),
                incremental_command,
                &runtime.workspace,
                Some(runtime.env.clone()),
                runtime.timeout,
            ),
        );
        let update_json = parse_json_output(&update.stdout);
        metrics.push(MetricObservation {
            name: format!("{repo_name}_{metric_suffix}"),
            value: update.duration_ms as f64,
            budget: budget(&elastic_repo_config, budget_name),
            lower_is_better: true,
            key: true,
        });
        let update_passed = update.passed();
        commands.push(update);
        if !update_passed {
            index_json = serde_json::json!({"cold": index_json, "incremental": update_json});
            return Ok(repo_report(
                repo_name, scope, commands, cases, metrics, index_json,
            ));
        }
        let validation = incremental_index_completion_validation(
            repo_name,
            repo_config,
            incremental.changed_path_count,
            &incremental.head_ref,
            &update_json,
        );
        let validation_passed = validation.passed();
        commands.push(validation);
        index_json = serde_json::json!({"cold": index_json, "incremental": update_json});
        if !validation_passed {
            return Ok(repo_report(
                repo_name, scope, commands, cases, metrics, index_json,
            ));
        }
    }

    let query_results = parallel_map(repo_cases, runtime.query_jobs.max(1), {
        let runtime = runtime.clone();
        let alias = alias.to_owned();
        let ref_selector = ref_selector.to_owned();
        let repo_name = repo_name.to_owned();
        move |case| {
            let query_alias = string_or(&case, "repository_alias", &alias).to_owned();
            let query = run_limited(
                &runtime.limiter,
                CommandSpec::new(
                    format!("{}_{}", repo_name, string_or(&case, "id", "case")),
                    query_command(&runtime.binary, &query_alias, &ref_selector, &case),
                    &runtime.workspace,
                    Some(runtime.env.clone()),
                    runtime.timeout,
                ),
            );
            let duration_ms = query.duration_ms;
            let observation = score_query_case(&repo_name, &case, &query);
            let guardrail_gate = guardrail_gate_from_case(&observation, duration_ms);
            (query, observation, guardrail_gate)
        }
    });
    let query_durations = query_results
        .iter()
        .map(|(command, _, _)| command.duration_ms)
        .collect::<Vec<_>>();
    for (command, observation, guardrail_gate) in query_results {
        commands.push(command);
        cases.push(observation);
        if let Some(gate) = guardrail_gate {
            guardrail_gates.push(gate);
        }
    }
    push_latency_metrics(
        &mut metrics,
        repo_config,
        &format!("{repo_name}_query"),
        &query_durations,
    );
    let software_results = parallel_map(software_cases, runtime.query_jobs.max(1), {
        let runtime = runtime.clone();
        let alias = alias.to_owned();
        let ref_selector = ref_selector.to_owned();
        let repo_name = repo_name.to_owned();
        move |case| {
            let query_alias = string_or(&case, "repository_alias", &alias).to_owned();
            let query = run_limited(
                &runtime.limiter,
                CommandSpec::new(
                    format!("{}_{}", repo_name, string_or(&case, "id", "software_case")),
                    software_query_command(&runtime.binary, &query_alias, &ref_selector, &case),
                    &runtime.workspace,
                    Some(runtime.env.clone()),
                    runtime.timeout,
                ),
            );
            let duration_ms = query.duration_ms;
            let observation = score_software_case(&repo_name, &case, &query);
            let guardrail_gate = guardrail_gate_from_case(&observation, duration_ms);
            (query, observation, guardrail_gate)
        }
    });
    let software_durations = software_results
        .iter()
        .map(|(command, _, _)| command.duration_ms)
        .collect::<Vec<_>>();
    for (command, observation, guardrail_gate) in software_results {
        commands.push(command);
        cases.push(observation);
        if let Some(gate) = guardrail_gate {
            guardrail_gates.push(gate);
        }
    }
    push_latency_metrics(
        &mut metrics,
        repo_config,
        &format!("{repo_name}_software_query"),
        &software_durations,
    );
    let mut report = repo_report(repo_name, scope, commands, cases, metrics, index_json);
    report.gates = guardrail_gates;
    Ok(report)
}

fn isolated_repository_runtime(
    runtime: &EvalRuntime,
    run_home: &Path,
    repo_name: &str,
    repo_config: &Value,
) -> Result<EvalRuntime, String> {
    if !repo_config
        .get("isolated_index_home")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        return Ok(runtime.clone());
    }
    if repo_name.is_empty()
        || !repo_name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
    {
        return Err(format!(
            "isolated repository name must be a safe path component: {repo_name:?}"
        ));
    }
    let home = run_home.join("isolated-index-homes").join(repo_name);
    if home.exists() {
        fs::remove_dir_all(&home)
            .map_err(|error| format!("failed to remove {}: {error}", home.display()))?;
    }
    fs::create_dir_all(&home)
        .map_err(|error| format!("failed to create {}: {error}", home.display()))?;
    let mut isolated = runtime.clone();
    isolated.env.insert(
        "RELAY_KNOWLEDGE_HOME".to_owned(),
        home.display().to_string(),
    );
    eprintln!(
        "[self-iterate] repository isolated index home name={} home={}",
        repo_name,
        home.display()
    );
    Ok(isolated)
}

fn cold_index_completion_validation(
    repo_name: &str,
    repo_config: &Value,
    payload: &Value,
) -> Option<CommandResult> {
    let minimum_files = repo_config
        .get("cold_index_min_file_count")
        .and_then(Value::as_u64)?;
    let indexed_files = payload
        .pointer("/status/indexed_file_count")
        .and_then(Value::as_u64)
        .unwrap_or_default();
    let parsed_files = payload
        .pointer("/summary/progress/parsed_file_count")
        .and_then(Value::as_u64)
        .unwrap_or_default();
    let task_succeeded =
        payload.pointer("/task/state").and_then(Value::as_str) == Some("succeeded");
    let passed =
        indexed_files >= minimum_files && (task_succeeded || parsed_files >= minimum_files);
    let evidence = serde_json::json!({
        "minimum_files": minimum_files,
        "indexed_files": indexed_files,
        "parsed_files": parsed_files,
        "task_succeeded": task_succeeded,
    });
    Some(CommandResult {
        name: format!("{repo_name}_cold_index_completion"),
        command: vec!["validate".to_owned(), "cold-index-completion".to_owned()],
        exit_code: i32::from(!passed),
        duration_ms: 0,
        stdout: if passed {
            evidence.to_string()
        } else {
            String::new()
        },
        stderr: if passed {
            String::new()
        } else {
            format!("cold index completion evidence failed: {evidence}")
        },
    })
}

fn with_observed_file_count(config: &Value, repository_path: &Path) -> Value {
    if !elastic_budget_enabled(config) {
        return config.clone();
    }
    let Ok(output) = Command::new("git")
        .args([
            "-C",
            &repository_path.display().to_string(),
            "ls-files",
            "-z",
        ])
        .output()
    else {
        return config.clone();
    };
    if !output.status.success() {
        return config.clone();
    }
    let observed = output.stdout.iter().filter(|byte| **byte == 0).count();
    if observed == 0 {
        return config.clone();
    }
    let mut effective = config.clone();
    if let Some(object) = effective.as_object_mut() {
        object.insert(
            "expected_file_count".to_owned(),
            Value::from(observed as u64),
        );
    }
    effective
}

fn elastic_timeout_seconds(default_seconds: u64, config: &Value, budget_name: &str) -> u64 {
    let Some(budget_ms) = budget(config, budget_name) else {
        return default_seconds;
    };
    let budget_seconds = (budget_ms / 1_000.0).ceil() as u64;
    default_seconds.max(budget_seconds.saturating_add(30))
}

fn incremental_index_completion_validation(
    repo_name: &str,
    repo_config: &Value,
    expected_changed_paths: usize,
    expected_head_ref: &str,
    payload: &Value,
) -> CommandResult {
    let changed_paths = payload
        .pointer("/summary/changed_path_count")
        .and_then(Value::as_u64)
        .unwrap_or_default();
    let blob_reads = payload
        .pointer("/summary/progress/blob_read_count")
        .and_then(Value::as_u64)
        .unwrap_or(u64::MAX);
    let parsed_files = payload
        .pointer("/summary/progress/parsed_file_count")
        .and_then(Value::as_u64)
        .unwrap_or(u64::MAX);
    let resolved_head = payload
        .pointer("/summary/resolved_commit_sha")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let max_blob_reads = repo_config
        .get("incremental_max_blob_reads")
        .and_then(Value::as_u64)
        .unwrap_or(expected_changed_paths as u64);
    let max_parsed_files = repo_config
        .get("incremental_max_parsed_files")
        .and_then(Value::as_u64)
        .unwrap_or(expected_changed_paths as u64);
    let passed = changed_paths == expected_changed_paths as u64
        && blob_reads <= max_blob_reads
        && parsed_files <= max_parsed_files
        && resolved_head == expected_head_ref;
    let evidence = serde_json::json!({
        "expected_changed_paths": expected_changed_paths,
        "changed_paths": changed_paths,
        "blob_reads": blob_reads,
        "max_blob_reads": max_blob_reads,
        "parsed_files": parsed_files,
        "max_parsed_files": max_parsed_files,
        "expected_head_ref": expected_head_ref,
        "resolved_head": resolved_head,
    });
    CommandResult {
        name: format!("{repo_name}_incremental_index_completion"),
        command: vec![
            "validate".to_owned(),
            "incremental-index-completion".to_owned(),
        ],
        exit_code: i32::from(!passed),
        duration_ms: 0,
        stdout: if passed {
            evidence.to_string()
        } else {
            String::new()
        },
        stderr: if passed {
            String::new()
        } else {
            format!("incremental index completion evidence failed: {evidence}")
        },
    }
}

#[cfg(test)]
#[path = "repository_tests.rs"]
mod tests;
