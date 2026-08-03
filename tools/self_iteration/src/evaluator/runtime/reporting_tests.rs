use super::{percentile, push_latency_metrics};

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
