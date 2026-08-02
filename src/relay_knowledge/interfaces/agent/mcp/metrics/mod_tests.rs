use super::{metrics_endpoint, push_metric};

#[test]
fn metrics_endpoint_is_a_normalized_child_of_the_mcp_endpoint() {
    assert_eq!(metrics_endpoint("/"), "/metrics");
    assert_eq!(metrics_endpoint("/mcp"), "/mcp/metrics");
    assert_eq!(metrics_endpoint("/mcp/"), "/mcp/metrics");
}

#[test]
fn metric_rendering_keeps_help_type_and_sample_together() {
    let mut output = String::new();

    push_metric(&mut output, "relay_test_total", "Test count.", 7);

    assert_eq!(
        output,
        "# HELP relay_test_total Test count.\n# TYPE relay_test_total gauge\nrelay_test_total 7\n"
    );
}
