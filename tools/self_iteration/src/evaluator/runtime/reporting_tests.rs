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
