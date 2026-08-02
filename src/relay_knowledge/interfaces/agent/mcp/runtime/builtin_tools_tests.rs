//! Direct built-in tool request contract tests.

use serde_json::json;

use super::{InspectGraphArgs, RetrieveContextArgs};

#[test]
fn retrieve_context_arguments_preserve_optional_policy_inputs() {
    let args = serde_json::from_value::<RetrieveContextArgs>(json!({
        "query": "graph context",
        "source_scope": "docs",
        "limit": 3,
        "freshness": "graph-only"
    }))
    .expect("retrieve arguments should decode");

    assert_eq!(args.query, "graph context");
    assert_eq!(args.source_scope.as_deref(), Some("docs"));
    assert_eq!(args.limit, Some(3));
    assert_eq!(args.freshness.as_deref(), Some("graph-only"));
}

#[test]
fn inspect_graph_arguments_default_to_unspecified_scope() {
    let args = serde_json::from_value::<InspectGraphArgs>(json!({}))
        .expect("inspect arguments should decode");

    assert_eq!(args.source_scope, None);
}
