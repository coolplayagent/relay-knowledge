use std::path::Path;

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
            retain_index_only_cold_index_result,
        },
    },
};
use super::{
    cli_cases::{
        framework_query_command, incremental_update_command, query_command, register_command,
        software_query_command,
    },
    repository_scoring::{score_framework_case, score_query_case, score_software_case},
    selection::guardrail_gate_from_case,
};

mod expectation;
mod isolation;

use expectation::{
    IndexExpectation, cold_index_completion_validation, incremental_index_completion_validation,
    observed_git_file_count, scope_preview_command,
};
use isolation::RepositoryIsolation;

fn scoped_register_command(
    binary: &Path,
    path: &Path,
    alias: Option<&str>,
    repo_config: &Value,
) -> Vec<String> {
    let mut command = register_command(binary, path, alias);
    let format = command.split_off(command.len().saturating_sub(2));
    for path_filter in string_vec(repo_config, "registration_path_filters") {
        command.extend(["--path".to_owned(), path_filter]);
    }
    command.extend(format);
    command
}

pub(in crate::evaluator) fn evaluate_repository(
    runtime: &EvalRuntime,
    run_home: &Path,
    repo_name: &str,
    repo_config: &Value,
    repo_cases: Vec<Value>,
    software_cases: Vec<Value>,
) -> Result<RepoReport, String> {
    let isolation = RepositoryIsolation::prepare(runtime, run_home, repo_name, repo_config)?;
    let result = evaluate_repository_in_runtime(
        &isolation.runtime,
        run_home,
        repo_name,
        repo_config,
        repo_cases,
        software_cases,
    );
    let mut report = isolation.complete(result)?;
    retain_index_only_cold_index_result(
        &mut report,
        repo_config
            .get("index_only_performance_target")
            .and_then(Value::as_bool)
            .unwrap_or(false),
    );
    Ok(report)
}

fn evaluate_repository_in_runtime(
    runtime: &EvalRuntime,
    run_home: &Path,
    repo_name: &str,
    repo_config: &Value,
    repo_cases: Vec<Value>,
    software_cases: Vec<Value>,
) -> Result<RepoReport, String> {
    let alias = string_or(repo_config, "alias", repo_name);
    let ref_selector = string_or(repo_config, "ref", "HEAD");
    let scope = string_or(repo_config, "scope", "all").to_owned();
    let mut commands = Vec::new();
    let mut cases = Vec::new();
    let mut guardrail_gates = Vec::new();
    let mut metrics = Vec::new();
    let (path, setup_commands) =
        prepare_repository_path(runtime, run_home, repo_name, repo_config)?;
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
    let (observed_repo_config, observed_git_count) =
        observed_git_file_count(runtime, repo_name, repo_config, &path, ref_selector);
    let observed_git_count_passed = observed_git_count.passed();
    commands.push(observed_git_count);
    if !observed_git_count_passed {
        return Ok(repo_report(
            repo_name,
            scope,
            commands,
            cases,
            metrics,
            Value::Null,
        ));
    }
    let elastic_repo_config = if elastic_budget_enabled(&observed_repo_config)
        && observed_repo_config.get("expected_file_count").is_none()
    {
        let mut effective = observed_repo_config.clone();
        if let (Some(object), Some(observed)) = (
            effective.as_object_mut(),
            observed_repo_config
                .get("observed_git_file_count")
                .and_then(Value::as_u64),
        ) {
            object.insert("expected_file_count".to_owned(), Value::from(observed));
        }
        effective
    } else {
        observed_repo_config.clone()
    };
    let register = run_writer_limited(
        runtime,
        CommandSpec::new(
            format!("{repo_name}_register"),
            scoped_register_command(
                &runtime.binary,
                &path,
                (!repo_config
                    .get("register_without_alias")
                    .and_then(Value::as_bool)
                    .unwrap_or(false))
                    .then_some(alias),
                repo_config,
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
                scoped_register_command(
                    &runtime.binary,
                    &path,
                    Some(&additional_alias),
                    repo_config,
                ),
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
    let preview = run_limited(
        &runtime.limiter,
        CommandSpec::new(
            format!("{repo_name}_scope_preview"),
            scope_preview_command(&runtime.binary, alias, ref_selector),
            &runtime.workspace,
            Some(runtime.env.clone()),
            runtime.timeout,
        ),
    );
    let preview_json = parse_json_output(&preview.stdout);
    let preview_passed = preview.passed();
    commands.push(preview);
    if !preview_passed {
        index_json = serde_json::json!({"index": index_json, "scope_preview": preview_json});
        return Ok(repo_report(
            repo_name, scope, commands, cases, metrics, index_json,
        ));
    }
    let preview_expectation = match IndexExpectation::from_preview(
        repo_name,
        &observed_repo_config,
        ref_selector,
        &preview_json,
    ) {
        Ok(expectation) => expectation,
        Err(validation) => {
            commands.push(validation);
            index_json = serde_json::json!({"index": index_json, "scope_preview": preview_json});
            return Ok(repo_report(
                repo_name, scope, commands, cases, metrics, index_json,
            ));
        }
    };
    commands.push(preview_expectation.validation_command(repo_name));
    let validation =
        cold_index_completion_validation(repo_name, repo_config, &preview_expectation, &index_json);
    let passed = validation.passed();
    commands.push(validation);
    if !passed {
        return Ok(repo_report(
            repo_name, scope, commands, cases, metrics, index_json,
        ));
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
            historical_reuse_index_command(&runtime.binary, alias, &incremental.head_ref)
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
        let incremental_preview = run_limited(
            &runtime.limiter,
            CommandSpec::new(
                format!("{repo_name}_incremental_scope_preview"),
                scope_preview_command(&runtime.binary, alias, &incremental.head_ref),
                &runtime.workspace,
                Some(runtime.env.clone()),
                runtime.timeout,
            ),
        );
        let incremental_preview_json = parse_json_output(&incremental_preview.stdout);
        let incremental_preview_passed = incremental_preview.passed();
        commands.push(incremental_preview);
        if !incremental_preview_passed {
            index_json = serde_json::json!({
                "cold": index_json,
                "incremental": update_json,
                "incremental_preview": incremental_preview_json,
            });
            return Ok(repo_report(
                repo_name, scope, commands, cases, metrics, index_json,
            ));
        }
        let (incremental_observed_config, incremental_git_count) = observed_git_file_count(
            runtime,
            &format!("{repo_name}_incremental"),
            repo_config,
            &path,
            &incremental.head_ref,
        );
        let incremental_git_count_passed = incremental_git_count.passed();
        commands.push(incremental_git_count);
        if !incremental_git_count_passed {
            index_json = serde_json::json!({
                "cold": index_json,
                "incremental": update_json,
                "incremental_preview": incremental_preview_json,
            });
            return Ok(repo_report(
                repo_name, scope, commands, cases, metrics, index_json,
            ));
        }
        let incremental_expectation = match IndexExpectation::from_preview(
            repo_name,
            &incremental_observed_config,
            &incremental.head_ref,
            &incremental_preview_json,
        ) {
            Ok(expectation) => expectation,
            Err(validation) => {
                commands.push(validation);
                index_json = serde_json::json!({
                    "cold": index_json,
                    "incremental": update_json,
                    "incremental_preview": incremental_preview_json,
                });
                return Ok(repo_report(
                    repo_name, scope, commands, cases, metrics, index_json,
                ));
            }
        };
        commands.push(incremental_expectation.validation_command(repo_name));
        let validation = incremental_index_completion_validation(
            repo_name,
            repo_config,
            incremental.changed_path_count,
            &incremental.base_ref,
            &incremental_expectation,
            &update_json,
        );
        let validation_passed = validation.passed();
        commands.push(validation);
        index_json = serde_json::json!({
            "cold": index_json,
            "incremental": update_json,
            "incremental_preview": incremental_preview_json,
        });
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
            let framework_surface = string_or(&case, "surface", "query") == "framework";
            let command = if framework_surface {
                framework_query_command(&runtime.binary, &query_alias, &ref_selector, &case)
            } else {
                query_command(&runtime.binary, &query_alias, &ref_selector, &case)
            };
            let query = run_limited(
                &runtime.limiter,
                CommandSpec::new(
                    format!("{}_{}", repo_name, string_or(&case, "id", "case")),
                    command,
                    &runtime.workspace,
                    Some(runtime.env.clone()),
                    runtime.timeout,
                ),
            );
            let duration_ms = query.duration_ms;
            let observation = if framework_surface {
                score_framework_case(&repo_name, &case, &query)
            } else {
                score_query_case(&repo_name, &case, &query)
            };
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

fn historical_reuse_index_command(binary: &Path, alias: &str, head_ref: &str) -> Vec<String> {
    vec![
        binary.display().to_string(),
        "repo".to_owned(),
        "index".to_owned(),
        alias.to_owned(),
        "--ref".to_owned(),
        head_ref.to_owned(),
        "--reuse-historical".to_owned(),
        "--format".to_owned(),
        "json".to_owned(),
    ]
}

fn elastic_timeout_seconds(default_seconds: u64, config: &Value, budget_name: &str) -> u64 {
    let Some(budget_ms) = budget(config, budget_name) else {
        return default_seconds;
    };
    let budget_seconds = (budget_ms / 1_000.0).ceil() as u64;
    default_seconds.max(budget_seconds.saturating_add(30))
}

#[cfg(test)]
#[path = "repository_tests.rs"]
mod tests;
