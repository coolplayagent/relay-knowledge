use std::collections::BTreeMap;

use serde_json::Value;

use crate::{
    command::CommandResult,
    scoring::{CaseObservation, MetricObservation},
};

use super::contracts::RepoReport;

pub(in crate::evaluator) fn parse_json_output(stdout: &str) -> Value {
    parse_json_output_value(stdout).unwrap_or(Value::Null)
}

pub(in crate::evaluator) fn parse_json_output_value(stdout: &str) -> Option<Value> {
    stdout
        .lines()
        .rev()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .find_map(|line| serde_json::from_str(line).ok())
}

pub(in crate::evaluator) fn push_latency_metrics(
    metrics: &mut Vec<MetricObservation>,
    config: &Value,
    prefix: &str,
    durations: &[u64],
) {
    if durations.is_empty() {
        return;
    }
    metrics.push(MetricObservation {
        name: format!("{prefix}_p50_ms"),
        value: percentile(durations, 50) as f64,
        budget: budget(config, "query_p50_budget_ms"),
        lower_is_better: true,
        key: false,
    });
    metrics.push(MetricObservation {
        name: format!("{prefix}_p95_ms"),
        value: percentile(durations, 95) as f64,
        budget: budget(config, "query_p95_budget_ms"),
        lower_is_better: true,
        key: true,
    });
    if let Some(max_budget) = budget(config, "query_max_budget_ms") {
        metrics.push(MetricObservation {
            name: format!("{prefix}_max_ms"),
            value: durations.iter().copied().max().unwrap_or(0) as f64,
            budget: Some(max_budget),
            lower_is_better: true,
            key: true,
        });
    }
}

fn percentile(values: &[u64], percentile_value: u64) -> u64 {
    let mut ordered = values.to_vec();
    ordered.sort_unstable();
    let index = ((ordered.len() - 1) as u64 * percentile_value / 100) as usize;
    ordered[index]
}

pub(in crate::evaluator) fn budget(value: &Value, name: &str) -> Option<f64> {
    if elastic_budget_enabled(value)
        && value
            .get("baseline_file_count")
            .and_then(Value::as_f64)
            .is_some()
        && (name == "index_budget_ms" || name == "register_index_budget_ms")
    {
        let baseline_files = value
            .get("baseline_file_count")
            .and_then(Value::as_f64)
            .filter(|count| *count > 0.0)?;
        let expected_files = value
            .get("expected_file_count")
            .and_then(Value::as_f64)
            .filter(|count| *count > 0.0)
            .unwrap_or(baseline_files);
        let baseline_budget = value
            .get("baseline_index_budget_ms")
            .and_then(Value::as_f64)
            .filter(|budget| *budget > 0.0)?;
        let throughput_budget = value
            .get("baseline_files_per_second")
            .and_then(Value::as_f64)
            .filter(|throughput| *throughput > 0.0)
            .map(|throughput| expected_files / throughput * 1_000.0);
        let register_overhead = if name == "register_index_budget_ms" {
            value
                .get("register_overhead_budget_ms")
                .and_then(Value::as_f64)
                .unwrap_or(1_000.0)
        } else {
            0.0
        };
        let scaled = throughput_budget
            .unwrap_or_else(|| baseline_budget * (expected_files / baseline_files))
            + register_overhead;
        let maximum = value
            .get("max_index_budget_ms")
            .and_then(Value::as_f64)
            .filter(|budget| *budget > 0.0)
            .unwrap_or(f64::INFINITY);
        return Some(scaled.min(maximum));
    }
    value
        .get(name)
        .and_then(Value::as_f64)
        .filter(|value| *value > 0.0)
}

pub(in crate::evaluator) fn elastic_budget_enabled(value: &Value) -> bool {
    value
        .get("index_budget_mode")
        .and_then(Value::as_str)
        .unwrap_or("elastic")
        == "elastic"
}

pub(in crate::evaluator) fn normalized_env(
    env: &BTreeMap<String, String>,
    name: &str,
    default: &str,
) -> String {
    env.get(name)
        .map(|value| value.trim().to_ascii_lowercase())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| default.to_owned())
}

pub(in crate::evaluator) fn repo_report(
    repo_name: &str,
    scope: String,
    commands: Vec<CommandResult>,
    cases: Vec<CaseObservation>,
    metrics: Vec<MetricObservation>,
    index_summary: Value,
) -> RepoReport {
    let passed_commands = commands.iter().filter(|command| command.passed()).count();
    let passed_cases = cases.iter().filter(|case| case.passed).count();
    let command_duration_ms = commands
        .iter()
        .map(|command| command.duration_ms)
        .sum::<u64>();
    eprintln!(
        "[self-iterate] report done name={} commands={}/{} cases={}/{} metrics={} command_duration_ms={}",
        repo_name,
        passed_commands,
        commands.len(),
        passed_cases,
        cases.len(),
        metrics.len(),
        command_duration_ms
    );
    RepoReport {
        repository: repo_name.to_owned(),
        scope,
        commands,
        gates: Vec::new(),
        cases,
        metrics,
        index_summary,
        cold_index_result: None,
    }
}

pub(in crate::evaluator) fn retain_index_only_cold_index_result(
    report: &mut RepoReport,
    index_only_performance_target: bool,
) {
    if !index_only_performance_target {
        return;
    }
    let completion_name = format!("{}_cold_index_completion", report.repository);
    if !report
        .commands
        .iter()
        .any(|command| command.name == completion_name && command.passed())
    {
        return;
    }
    report.cold_index_result = Some(
        report
            .index_summary
            .get("cold")
            .cloned()
            .unwrap_or_else(|| report.index_summary.clone()),
    );
}

pub(super) fn serializable_repo_report(report: &RepoReport) -> Value {
    let mut serialized = serde_json::json!({
        "repository": report.repository,
        "scope": report.scope,
        "commands": report.commands.iter().map(CommandResult::serializable).collect::<Vec<_>>(),
        "gates": report.gates,
        "cases": report.cases,
        "metrics": report.metrics,
        "index_summary": report.index_summary.get("summary").cloned().unwrap_or_else(|| report.index_summary.clone()),
    });
    if let (Some(object), Some(cold_index_result)) =
        (serialized.as_object_mut(), &report.cold_index_result)
    {
        object.insert("cold_index_result".to_owned(), cold_index_result.clone());
    }
    serialized
}

#[cfg(test)]
#[path = "reporting_tests.rs"]
mod tests;
