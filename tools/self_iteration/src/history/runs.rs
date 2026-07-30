use std::fs;

use serde_json::Value;

use super::{
    HistoryPaths,
    run_state::{adopted, automated_baseline_run},
};

pub fn load_runs(paths: &HistoryPaths) -> Result<Vec<Value>, String> {
    if !paths.runs_jsonl.exists() {
        return Ok(Vec::new());
    }
    let text = fs::read_to_string(&paths.runs_jsonl)
        .map_err(|error| format!("failed to read {}: {error}", paths.runs_jsonl.display()))?;
    text.lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| {
            serde_json::from_str(line).map_err(|error| format!("invalid run record: {error}"))
        })
        .collect()
}

pub fn previous_scored_run(paths: &HistoryPaths) -> Result<Option<Value>, String> {
    let runs = load_runs(paths)?;
    Ok(latest_scored_run(
        runs.into_iter().filter(automated_baseline_run),
    ))
}

pub fn previous_scored_run_for_workload(
    paths: &HistoryPaths,
    profile: &str,
    category_focus: Option<&str>,
) -> Result<Option<Value>, String> {
    let runs = load_runs(paths)?;
    Ok(latest_scored_run(runs.into_iter().filter(|run| {
        run_profile(run) == profile
            && run_category_focus(run) == category_focus
            && automated_baseline_run(run)
    })))
}

fn latest_scored_run<I>(runs: I) -> Option<Value>
where
    I: Iterator<Item = Value>,
{
    runs.filter(|run| run.get("score").is_some())
        .max_by_key(|run| {
            run.get("timestamp")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_owned()
        })
}

fn run_profile(run: &Value) -> &str {
    run.get("profile").and_then(Value::as_str).unwrap_or("full")
}

fn run_category_focus(run: &Value) -> Option<&str> {
    run.get("category_focus").and_then(Value::as_str)
}

pub fn best_accepted_run_for_workload(
    paths: &HistoryPaths,
    profile: &str,
    category_focus: Option<&str>,
) -> Result<Option<Value>, String> {
    let runs = load_runs(paths)?;
    Ok(best_accepted_run(runs.into_iter().filter(|run| {
        run_profile(run) == profile && run_category_focus(run) == category_focus
    })))
}

pub fn best_accepted_run_for_profile(
    paths: &HistoryPaths,
    profile: &str,
) -> Result<Option<Value>, String> {
    let runs = load_runs(paths)?;
    Ok(best_accepted_run(
        runs.into_iter().filter(|run| run_profile(run) == profile),
    ))
}

fn best_accepted_run<I>(runs: I) -> Option<Value>
where
    I: Iterator<Item = Value>,
{
    runs.filter(adopted).max_by(|left, right| {
        let left_score = left.get("score").and_then(Value::as_f64).unwrap_or(0.0);
        let right_score = right.get("score").and_then(Value::as_f64).unwrap_or(0.0);
        left_score
            .partial_cmp(&right_score)
            .unwrap_or(std::cmp::Ordering::Equal)
    })
}

#[cfg(test)]
#[path = "baseline_selection_tests.rs"]
mod baseline_selection_tests;

#[cfg(test)]
#[path = "profile_selection_tests.rs"]
mod profile_selection_tests;

#[cfg(test)]
#[path = "workload_selection_tests.rs"]
mod workload_selection_tests;
