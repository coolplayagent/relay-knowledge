use serde_json::json;

use super::*;

#[test]
fn parses_view_payload_variants() {
    let request = code_view_request(&json!({
        "alias": "relay",
        "ref": "main",
        "kind": "dependency-tour",
        "freshness": "wait-until-fresh",
        "limit": 7,
        "changed_paths": ["src/lib.rs"]
    }))
    .expect("view request");

    assert_eq!(request.view_kind, CodebaseViewKind::DependencyTour);
    assert_eq!(request.changed_paths, ["src/lib.rs"]);
}

#[test]
fn rejects_unsupported_view_kinds() {
    let error = code_view_request(&json!({
        "alias": "relay",
        "ref": "main",
        "kind": "unknown",
        "freshness": "allow-stale",
        "limit": 7
    }))
    .expect_err("unsupported view kind");

    assert!(error.message.contains("unsupported codebase view kind"));
}
