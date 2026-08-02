use super::*;

#[test]
fn endpoint_children_preserve_root_and_trim_trailing_separators() {
    assert_eq!(endpoint_child("/", "metrics"), "/metrics");
    assert_eq!(endpoint_child("/mcp/", "metrics"), "/mcp/metrics");
}

#[test]
fn duration_projection_saturates_values_beyond_wire_capacity() {
    assert_eq!(duration_millis(Duration::MAX), u64::MAX);
    assert_eq!(duration_millis(Duration::from_millis(42)), 42);
}
