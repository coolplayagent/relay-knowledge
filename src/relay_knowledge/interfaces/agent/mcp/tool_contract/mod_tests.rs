//! Direct contracts for MCP tool argument and result mapping.

use serde_json::json;

use super::{
    AgentAdapterError, AgentAdapterErrorKind, FreshnessPolicy, parse_freshness, tool_error_result,
    tool_success_result,
};

#[test]
fn freshness_labels_map_to_the_domain_contract() {
    assert_eq!(
        parse_freshness(None).expect("default freshness"),
        FreshnessPolicy::AllowStale
    );
    assert_eq!(
        parse_freshness(Some("wait-until-fresh")).expect("wait freshness"),
        FreshnessPolicy::WaitUntilFresh
    );
    assert_eq!(
        parse_freshness(Some("graph-only")).expect("graph freshness"),
        FreshnessPolicy::GraphOnly
    );

    let error = parse_freshness(Some("eventually")).expect_err("unknown freshness");
    assert_eq!(error.kind, AgentAdapterErrorKind::InvalidArgument);
    assert_eq!(error.message, "invalid freshness 'eventually'");
}

#[test]
fn tool_results_keep_text_and_structured_error_contracts_aligned() {
    let success = tool_success_result("loaded", json!({"count": 2}));
    assert_eq!(success["content"][0]["text"], "loaded");
    assert_eq!(success["structuredContent"]["count"], 2);
    assert_eq!(success["isError"], false);

    let error = tool_error_result(AgentAdapterError::new(
        AgentAdapterErrorKind::Timeout,
        "deadline reached",
    ));
    assert_eq!(error["content"][0]["text"], "timeout: deadline reached");
    assert_eq!(error["structuredContent"]["error_kind"], "timeout");
    assert_eq!(error["structuredContent"]["message"], "deadline reached");
    assert_eq!(error["isError"], true);
}
