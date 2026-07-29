use std::{collections::BTreeMap, fs, path::Path};

use serde_json::Value;

use crate::{
    cases::{array_field, number_or, string_field, string_or, string_vec},
    command::{CommandResult, CommandSpec},
    config::CategorySet,
    scoring::CaseObservation,
};

use super::super::{
    CliContractReport, EvalRuntime, RegistrationCaseReport, parse_json_output_value,
    prepare_repository_path, run_writer_limited,
};
use super::{
    repository_scoring::repository_case_objective,
    selection::{focused_repository_case, guardrail_gate_from_case, is_guardrail_case},
};

pub(in crate::evaluator) fn evaluate_registration_cases(
    runtime: &EvalRuntime,
    run_home: &Path,
    repository_configs: &BTreeMap<String, Value>,
    cases_config: &Value,
    profile: &str,
    categories: Option<&CategorySet>,
) -> Result<RegistrationCaseReport, String> {
    let selected_cases = select_registration_cases_for_profile(
        profile,
        categories,
        array_field(cases_config, "registration_cases").to_vec(),
    );
    let mut commands = Vec::new();
    let mut cases = Vec::new();
    let mut gates = Vec::new();
    if selected_cases.is_empty() {
        return Ok(RegistrationCaseReport {
            commands,
            cases,
            gates,
        });
    }
    eprintln!(
        "[self-iterate] registration guardrail workload start cases={}",
        selected_cases.len()
    );
    for case in selected_cases {
        let repo_name = string_or(&case, "repository", "");
        let Some(repo_config) = repository_configs.get(repo_name) else {
            let command = CommandResult {
                name: format!(
                    "registration_case_{}_repository_config",
                    string_or(&case, "id", "case")
                ),
                command: vec!["validate".to_owned(), "repository_config".to_owned()],
                exit_code: 1,
                duration_ms: 0,
                stdout: String::new(),
                stderr: format!("registration case repository is not configured: {repo_name}"),
            };
            let observation = score_registration_case(repo_name, &case, &command);
            if let Some(gate) = guardrail_gate_from_case(&observation, command.duration_ms) {
                gates.push(gate);
            }
            commands.push(command);
            cases.push(observation);
            continue;
        };
        let (path, setup_commands) =
            prepare_repository_path(runtime, run_home, repo_name, repo_config)?;
        let setup_passed = setup_commands.iter().all(CommandResult::passed);
        commands.extend(setup_commands);
        if !setup_passed {
            let failed = CommandResult {
                name: format!("registration_case_{}_setup", string_or(&case, "id", "case")),
                command: vec!["prepare".to_owned(), repo_name.to_owned()],
                exit_code: 1,
                duration_ms: 0,
                stdout: String::new(),
                stderr: "registration case fixture setup failed".to_owned(),
            };
            let observation = score_registration_case(repo_name, &case, &failed);
            if let Some(gate) = guardrail_gate_from_case(&observation, failed.duration_ms) {
                gates.push(gate);
            }
            commands.push(failed);
            cases.push(observation);
            continue;
        }
        let alias = registration_case_alias(repo_config, &case);
        let register = run_writer_limited(
            runtime,
            CommandSpec::new(
                format!("registration_case_{}", string_or(&case, "id", "case")),
                registration_case_command(&runtime.binary, &path, &alias, &case),
                &runtime.workspace,
                Some(runtime.env.clone()),
                runtime.timeout,
            ),
        );
        let observation = score_registration_case(repo_name, &case, &register);
        if let Some(gate) = guardrail_gate_from_case(&observation, register.duration_ms) {
            gates.push(gate);
        }
        commands.push(register);
        cases.push(observation);
    }

    Ok(RegistrationCaseReport {
        commands,
        cases,
        gates,
    })
}

pub(super) fn select_registration_cases_for_profile(
    profile: &str,
    categories: Option<&CategorySet>,
    cases: Vec<Value>,
) -> Vec<Value> {
    cases
        .into_iter()
        .filter(|case| {
            string_field(case, "profile") != Some("exhaustive") || profile == "exhaustive"
        })
        .filter(|case| {
            categories
                .map(|items| focused_repository_case(items, case))
                .unwrap_or(true)
        })
        .collect()
}

pub(in crate::evaluator) fn evaluate_cli_contract_cases(
    runtime: &EvalRuntime,
    run_home: &Path,
    cases_config: &Value,
    profile: &str,
    categories: Option<&CategorySet>,
) -> CliContractReport {
    let selected_cases = select_cli_contract_cases_for_profile(
        profile,
        categories,
        array_field(cases_config, "cli_contract_cases").to_vec(),
    );
    let mut commands = Vec::new();
    let mut cases = Vec::new();
    let mut gates = Vec::new();
    if selected_cases.is_empty() {
        return CliContractReport {
            commands,
            cases,
            gates,
        };
    }
    eprintln!(
        "[self-iterate] cli contract workload start cases={}",
        selected_cases.len()
    );
    for case in selected_cases {
        let command = cli_contract_command(&runtime.binary, &case);
        let env = match cli_contract_env(&runtime.env, run_home, &case) {
            Ok(env) => env,
            Err(error) => {
                let result = CommandResult {
                    name: format!("cli_contract_{}", string_or(&case, "id", "case")),
                    command,
                    exit_code: 1,
                    duration_ms: 0,
                    stdout: String::new(),
                    stderr: error,
                };
                let observation = score_cli_contract_case(&case, &result);
                if let Some(gate) = guardrail_gate_from_case(&observation, result.duration_ms) {
                    gates.push(gate);
                }
                commands.push(result);
                cases.push(observation);
                continue;
            }
        };
        let result = run_writer_limited(
            runtime,
            CommandSpec::new(
                format!("cli_contract_{}", string_or(&case, "id", "case")),
                command,
                &runtime.workspace,
                Some(env),
                number_or(&case, "timeout_seconds", runtime.timeout),
            ),
        );
        let observation = score_cli_contract_case(&case, &result);
        if let Some(gate) = guardrail_gate_from_case(&observation, result.duration_ms) {
            gates.push(gate);
        }
        commands.push(result);
        cases.push(observation);
    }

    CliContractReport {
        commands,
        cases,
        gates,
    }
}

fn select_cli_contract_cases_for_profile(
    profile: &str,
    categories: Option<&CategorySet>,
    cases: Vec<Value>,
) -> Vec<Value> {
    cases
        .into_iter()
        .filter(|case| {
            string_field(case, "profile") != Some("exhaustive") || profile == "exhaustive"
        })
        .filter(|case| {
            categories
                .map(|items| focused_repository_case(items, case))
                .unwrap_or(true)
        })
        .collect()
}

fn cli_contract_command(binary: &Path, case: &Value) -> Vec<String> {
    let mut command = vec![binary.display().to_string()];
    command.extend(string_vec(case, "command"));
    command
}

fn cli_contract_env(
    base: &BTreeMap<String, String>,
    run_home: &Path,
    case: &Value,
) -> Result<BTreeMap<String, String>, String> {
    let mut env = base.clone();
    if !case
        .get("isolated_home")
        .and_then(Value::as_bool)
        .unwrap_or(true)
    {
        return Ok(env);
    }
    let home = run_home
        .join("cli-contracts")
        .join(safe_path_component(string_or(case, "id", "case")));
    if home.exists() {
        fs::remove_dir_all(&home)
            .map_err(|error| format!("failed to remove {}: {error}", home.display()))?;
    }
    fs::create_dir_all(&home)
        .map_err(|error| format!("failed to create {}: {error}", home.display()))?;
    env.insert(
        "RELAY_KNOWLEDGE_HOME".to_owned(),
        home.display().to_string(),
    );
    Ok(env)
}

fn safe_path_component(value: &str) -> String {
    value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
                ch
            } else {
                '_'
            }
        })
        .collect::<String>()
}

fn registration_case_alias(repo_config: &Value, case: &Value) -> String {
    string_field(case, "alias")
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| {
            format!(
                "{}-{}",
                string_or(repo_config, "alias", "registration-case"),
                string_or(case, "id", "case")
            )
        })
}

fn registration_case_command(binary: &Path, path: &Path, alias: &str, case: &Value) -> Vec<String> {
    let mut command = vec![
        binary.display().to_string(),
        "repo".to_owned(),
        "register".to_owned(),
        path.display().to_string(),
        "--alias".to_owned(),
        alias.to_owned(),
    ];
    for path_filter in string_vec(case, "path_filters") {
        command.extend(["--path".to_owned(), path_filter]);
    }
    for language in string_vec(case, "language_filters") {
        command.extend(["--language".to_owned(), language]);
    }
    command.extend(["--format".to_owned(), "json".to_owned()]);
    command
}

pub(super) fn register_command(binary: &Path, path: &Path, alias: Option<&str>) -> Vec<String> {
    let mut command = vec![
        binary.display().to_string(),
        "repo".to_owned(),
        "register".to_owned(),
        path.display().to_string(),
    ];
    if let Some(alias) = alias {
        command.extend(["--alias".to_owned(), alias.to_owned()]);
    }
    command.extend(["--format".to_owned(), "json".to_owned()]);
    command
}

pub(super) fn query_command(
    binary: &Path,
    alias: &str,
    ref_selector: &str,
    case: &Value,
) -> Vec<String> {
    let mut command = vec![
        binary.display().to_string(),
        "repo".to_owned(),
        "query".to_owned(),
        alias.to_owned(),
        "--query".to_owned(),
        string_or(case, "query", "").to_owned(),
        "--kind".to_owned(),
        string_or(case, "kind", "hybrid").to_owned(),
        "--ref".to_owned(),
        string_or(case, "ref", ref_selector).to_owned(),
        "--freshness".to_owned(),
        "wait-until-fresh".to_owned(),
        "--limit".to_owned(),
        number_or(case, "limit", 10).to_string(),
    ];
    for path in string_vec(case, "path_filters") {
        command.extend(["--path".to_owned(), path]);
    }
    for language in string_vec(case, "language_filters") {
        command.extend(["--language".to_owned(), language]);
    }
    command.extend(["--format".to_owned(), "json".to_owned()]);
    command
}

pub(super) fn software_query_command(
    binary: &Path,
    alias: &str,
    ref_selector: &str,
    case: &Value,
) -> Vec<String> {
    vec![
        binary.display().to_string(),
        "repo".to_owned(),
        "software".to_owned(),
        alias.to_owned(),
        "--kind".to_owned(),
        string_or(case, "kind", "all").to_owned(),
        "--ref".to_owned(),
        string_or(case, "ref", ref_selector).to_owned(),
        "--freshness".to_owned(),
        "wait-until-fresh".to_owned(),
        "--limit".to_owned(),
        number_or(case, "limit", 20).to_string(),
        "--format".to_owned(),
        "json".to_owned(),
    ]
}

fn score_registration_case(
    repo_name: &str,
    case: &Value,
    result: &CommandResult,
) -> CaseObservation {
    let objective = repository_case_objective(case);
    let expect_failure = case
        .get("expect_failure")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let output = format!("{}\n{}", result.stdout, result.stderr);
    let required_output =
        string_field(case, "output_contains").or_else(|| string_field(case, "stderr_contains"));
    let output_matches = required_output.is_none_or(|expected| output.contains(expected));
    let passed = if expect_failure {
        !result.passed() && output_matches
    } else {
        result.passed() && output_matches
    };
    let message = if passed {
        format!(
            "exit_code={} expected_failure={} output_contains={:?}",
            result.exit_code, expect_failure, required_output
        )
    } else if let Some(expected) = required_output {
        format!(
            "exit_code={} expected_failure={} missing_output={expected:?} tail={}",
            result.exit_code,
            expect_failure,
            result.gate_message()
        )
    } else {
        format!(
            "exit_code={} expected_failure={} tail={}",
            result.exit_code,
            expect_failure,
            result.gate_message()
        )
    };

    CaseObservation {
        case_id: string_or(case, "id", "case").to_owned(),
        repository: repo_name.to_owned(),
        passed,
        guardrail: is_guardrail_case(case),
        rank: passed.then_some(0),
        max_rank: 1,
        false_positive_count: 0,
        message,
        objective,
        score_override: Some(if passed { 1.0 } else { 0.0 }),
    }
}

fn score_cli_contract_case(case: &Value, result: &CommandResult) -> CaseObservation {
    let objective = repository_case_objective(case);
    let expect_failure = case
        .get("expect_failure")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let mut failures = Vec::new();
    if expect_failure == result.passed() {
        failures.push(format!(
            "exit_code={} expected_failure={expect_failure}",
            result.exit_code
        ));
    }
    let output = format!("{}\n{}", result.stdout, result.stderr);
    for expected in output_contains_values(case, "output_contains") {
        if !output.contains(&expected) {
            failures.push(format!("missing_output={expected:?}"));
        }
    }
    for expected in output_contains_values(case, "stdout_contains") {
        if !result.stdout.contains(&expected) {
            failures.push(format!("missing_stdout={expected:?}"));
        }
    }
    for expected in output_contains_values(case, "stderr_contains") {
        if !result.stderr.contains(&expected) {
            failures.push(format!("missing_stderr={expected:?}"));
        }
    }
    if let Some(expected) = case.get("json_expect") {
        match parse_json_output_value(&result.stdout) {
            Some(actual) => failures.extend(json_subset_failures("$", expected, &actual)),
            None => failures.push("stdout did not contain a JSON object".to_owned()),
        }
    }
    if let Some(expected_lines) = case.get("json_lines_expect").and_then(Value::as_array) {
        failures.extend(json_line_subset_failures(expected_lines, &result.stdout));
    }
    let passed = failures.is_empty();
    CaseObservation {
        case_id: string_or(case, "id", "case").to_owned(),
        repository: "cli_contract".to_owned(),
        passed,
        guardrail: is_guardrail_case(case),
        rank: passed.then_some(0),
        max_rank: 1,
        false_positive_count: 0,
        message: if passed {
            format!(
                "exit_code={} expected_failure={} contract assertions passed",
                result.exit_code, expect_failure
            )
        } else {
            format!(
                "exit_code={} failures={} tail={}",
                result.exit_code,
                failures.join("; "),
                result.gate_message()
            )
        },
        objective,
        score_override: Some(if passed { 1.0 } else { 0.0 }),
    }
}

fn output_contains_values(case: &Value, field: &str) -> Vec<String> {
    match case.get(field) {
        Some(Value::String(value)) => vec![value.to_owned()],
        Some(Value::Array(values)) => values
            .iter()
            .filter_map(Value::as_str)
            .map(ToOwned::to_owned)
            .collect(),
        _ => Vec::new(),
    }
}

fn json_line_subset_failures(expected_lines: &[Value], stdout: &str) -> Vec<String> {
    let mut failures = Vec::new();
    let observed_lines = stdout
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>();
    if observed_lines.len() != expected_lines.len() {
        failures.push(format!(
            "json_line_count={} expected={}",
            observed_lines.len(),
            expected_lines.len()
        ));
    }
    for (index, expected) in expected_lines.iter().enumerate() {
        let Some(line) = observed_lines.get(index) else {
            continue;
        };
        match serde_json::from_str::<Value>(line) {
            Ok(actual) => failures.extend(json_subset_failures(
                &format!("$[{index}]"),
                expected,
                &actual,
            )),
            Err(error) => failures.push(format!("json_line_{index}_parse={error}")),
        }
    }
    failures
}

fn json_subset_failures(path: &str, expected: &Value, actual: &Value) -> Vec<String> {
    let mut failures = Vec::new();
    match expected {
        Value::Object(expected_object) => {
            let Some(actual_object) = actual.as_object() else {
                failures.push(format!("{path} expected object got {}", json_type(actual)));
                return failures;
            };
            for (key, expected_value) in expected_object {
                match actual_object.get(key) {
                    Some(actual_value) => failures.extend(json_subset_failures(
                        &format!("{path}.{key}"),
                        expected_value,
                        actual_value,
                    )),
                    None => failures.push(format!("{path}.{key} missing")),
                }
            }
        }
        Value::Array(expected_items) => {
            let Some(actual_items) = actual.as_array() else {
                failures.push(format!("{path} expected array got {}", json_type(actual)));
                return failures;
            };
            if actual_items.len() < expected_items.len() {
                failures.push(format!(
                    "{path} length={} expected_at_least={}",
                    actual_items.len(),
                    expected_items.len()
                ));
            }
            for (index, expected_value) in expected_items.iter().enumerate() {
                if let Some(actual_value) = actual_items.get(index) {
                    failures.extend(json_subset_failures(
                        &format!("{path}[{index}]"),
                        expected_value,
                        actual_value,
                    ));
                }
            }
        }
        _ if actual != expected => {
            failures.push(format!("{path} expected={expected} actual={actual}"));
        }
        _ => {}
    }
    failures
}

fn json_type(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "bool",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

#[cfg(test)]
#[path = "cli_cases_tests.rs"]
mod tests;
