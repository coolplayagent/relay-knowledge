use crate::command::CommandResult;
use serde_json::Value;

use super::{
    percentile, push_latency_metrics, repo_report, retain_index_only_cold_index_result,
    serializable_repo_report,
};

#[test]
fn percentile_selects_expected_rank() {
    assert_eq!(percentile(&[10, 20, 30, 40], 50), 20);
    assert_eq!(percentile(&[10, 20, 30, 40], 95), 30);
}

#[test]
fn query_max_budget_records_tail_latency_metric() {
    let mut metrics = Vec::new();
    let config = serde_json::json!({
        "query_p50_budget_ms": 100,
        "query_p95_budget_ms": 200,
        "query_max_budget_ms": 1000
    });

    push_latency_metrics(&mut metrics, &config, "wide_repo_query", &[40, 81, 926]);

    let max_metric = metrics
        .iter()
        .find(|metric| metric.name == "wide_repo_query_max_ms")
        .expect("max latency metric should be recorded");
    assert_eq!(max_metric.value, 926.0);
    assert_eq!(max_metric.budget, Some(1000.0));
    assert!(max_metric.key);
}

#[test]
fn index_only_serialization_preserves_validated_cold_terminal_evidence() {
    let cold_index_result = serde_json::json!({
        "scope": {
            "scope_id": "scope-1",
            "repository_id": "repository-1",
            "alias": "linux",
            "requested_ref": "v1",
            "resolved_commit_sha": "commit-1",
            "tree_hash": "tree-1",
            "indexed_file_count": 2,
            "path_filters": [],
            "language_filters": []
        },
        "task": {
            "state": "succeeded",
            "mode": "full",
            "repository_id": "repository-1",
            "alias": "linux",
            "ref_selector": "v1",
            "resolved_commit_sha": "commit-1",
            "tree_hash": "tree-1",
            "source_scope": "scope-1",
            "path_filters": [],
            "language_filters": []
        },
        "summary": {
            "repository_id": "repository-1",
            "source_scope": "scope-1",
            "resolved_commit_sha": "commit-1",
            "tree_hash": "tree-1",
            "indexed_file_count": 2
        },
        "checkpoint": {
            "state": "completed",
            "repository_id": "repository-1",
            "source_scope": "scope-1",
            "committed_file_count": 2,
            "total_path_count": 2
        },
        "status": {
            "state": "fresh",
            "stale": false,
            "repository_id": "repository-1",
            "alias": "linux",
            "last_indexed_scope_id": "scope-1",
            "last_indexed_commit": "commit-1",
            "tree_hash": "tree-1",
            "indexed_file_count": 2,
            "path_filters": [],
            "language_filters": []
        }
    });
    let mut report = repo_report(
        "linux_full",
        "all".to_owned(),
        vec![completion_command("linux_full", 0)],
        Vec::new(),
        Vec::new(),
        cold_index_result.clone(),
    );

    retain_index_only_cold_index_result(&mut report, true);
    let serialized = serializable_repo_report(&report);

    assert_eq!(serialized["index_summary"], cold_index_result["summary"]);
    assert_eq!(serialized["cold_index_result"], cold_index_result);
    assert_eq!(
        serialized
            .pointer("/cold_index_result/task/state")
            .and_then(Value::as_str),
        Some("succeeded")
    );
    assert_eq!(
        serialized
            .pointer("/cold_index_result/checkpoint/state")
            .and_then(Value::as_str),
        Some("completed")
    );
    assert_eq!(
        serialized
            .pointer("/cold_index_result/status/state")
            .and_then(Value::as_str),
        Some("fresh")
    );
    assert_eq!(
        serialized.pointer("/cold_index_result/scope/scope_id"),
        serialized.pointer("/cold_index_result/task/source_scope")
    );
}

#[test]
fn ordinary_and_unvalidated_reports_omit_cold_index_result() {
    let raw_result = serde_json::json!({
        "summary": {"indexed_file_count": 2},
        "task": {"state": "succeeded"}
    });
    let mut ordinary = repo_report(
        "ordinary",
        "all".to_owned(),
        vec![completion_command("ordinary", 0)],
        Vec::new(),
        Vec::new(),
        raw_result.clone(),
    );
    retain_index_only_cold_index_result(&mut ordinary, false);
    let ordinary_json = serializable_repo_report(&ordinary);
    assert_eq!(ordinary_json["index_summary"], raw_result["summary"]);
    assert!(ordinary_json.get("cold_index_result").is_none());

    let mut unvalidated = repo_report(
        "linux_full",
        "all".to_owned(),
        vec![completion_command("linux_full", 1)],
        Vec::new(),
        Vec::new(),
        raw_result,
    );
    retain_index_only_cold_index_result(&mut unvalidated, true);
    assert!(
        serializable_repo_report(&unvalidated)
            .get("cold_index_result")
            .is_none()
    );
}

fn completion_command(repository: &str, exit_code: i32) -> CommandResult {
    CommandResult {
        name: format!("{repository}_cold_index_completion"),
        command: vec!["validate".to_owned(), "cold-index-completion".to_owned()],
        exit_code,
        duration_ms: 0,
        stdout: String::new(),
        stderr: String::new(),
    }
}
use super::{budget, elastic_budget_enabled};
use serde_json::json;

#[test]
fn elastic_index_budget_scales_from_baseline_and_is_capped() {
    let config = json!({
        "index_budget_mode": "elastic",
        "baseline_file_count": 100.0,
        "expected_file_count": 250.0,
        "baseline_index_budget_ms": 10_000.0,
        "register_overhead_budget_ms": 1_000.0,
        "max_index_budget_ms": 20_000.0
    });
    assert_eq!(budget(&config, "index_budget_ms"), Some(20_000.0));
    assert_eq!(budget(&config, "register_index_budget_ms"), Some(20_000.0));
}

#[test]
fn non_elastic_budget_keeps_explicit_contract() {
    let config = json!({"index_budget_ms": 12_345});
    assert_eq!(budget(&config, "index_budget_ms"), Some(12_345.0));
}

#[test]
fn elastic_budget_prefers_observed_throughput_baseline() {
    let config = serde_json::json!({
        "index_budget_mode": "elastic",
        "baseline_file_count": 100,
        "expected_file_count": 800,
        "baseline_index_budget_ms": 10_000,
        "baseline_files_per_second": 80,
        "max_index_budget_ms": 20_000
    });
    assert_eq!(budget(&config, "index_budget_ms"), Some(10_000.0));
}

#[test]
fn elastic_budget_is_the_default_mode() {
    let config = json!({
        "baseline_file_count": 100,
        "expected_file_count": 200,
        "baseline_index_budget_ms": 10_000,
        "max_index_budget_ms": 30_000
    });
    assert!(elastic_budget_enabled(&config));
    assert_eq!(budget(&config, "index_budget_ms"), Some(20_000.0));
}
