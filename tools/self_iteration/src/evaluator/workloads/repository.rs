use std::path::Path;

use serde_json::Value;

use crate::{
    cases::{string_or, string_vec},
    command::{CommandResult, CommandSpec},
    scoring::MetricObservation,
};

use super::super::{
    fixtures::prepare_repository_path,
    runtime::{
        concurrency::{parallel_map, run_limited, run_writer_limited},
        contracts::{EvalRuntime, RepoReport},
        reporting::{budget, parse_json_output, push_latency_metrics, repo_report},
    },
};
use super::{
    cli_cases::{query_command, register_command, software_query_command},
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
            runtime.timeout,
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
            runtime.timeout,
        ),
    );
    let index_json = parse_json_output(&index.stdout);
    metrics.push(MetricObservation {
        name: format!("{repo_name}_index_ms"),
        value: index.duration_ms as f64,
        budget: budget(repo_config, "index_budget_ms"),
        lower_is_better: true,
        key: true,
    });
    metrics.push(MetricObservation {
        name: format!("{repo_name}_register_index_ms"),
        value: (register.duration_ms + index.duration_ms) as f64,
        budget: budget(repo_config, "register_index_budget_ms"),
        lower_is_better: true,
        key: true,
    });
    commands.push(index.clone());
    if !index.passed() {
        return Ok(repo_report(
            repo_name, scope, commands, cases, metrics, index_json,
        ));
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

#[cfg(test)]
#[path = "repository_tests.rs"]
mod tests;
