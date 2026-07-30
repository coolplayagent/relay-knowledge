use serde_json::json;

use super::*;

#[test]
fn changed_paths_prefers_structured_optimization_plan() {
    let record = json!({
        "optimization_plan": {
            "changed_paths": ["src/query.rs", "src/index.rs"],
        },
        "patch": {
            "path": "/missing/patch",
        },
    });

    assert_eq!(
        changed_paths(&record),
        vec!["src/query.rs".to_owned(), "src/index.rs".to_owned()]
    );
}

#[test]
fn memory_identifiers_are_bounded_and_filesystem_safe() {
    let value = format!("  name with spaces/and?punctuation-{}  ", "x".repeat(180));
    let id = safe_id(&value);

    assert_eq!(id.len(), 160);
    assert!(!id.contains([' ', '/', '?']));
    assert_eq!(safe_id("///"), "memory");
}

#[test]
fn failed_gate_metadata_only_reports_failed_gates() {
    let record = json!({
        "gates": [
            {"name": "format", "passed": true},
            {"name": "clippy", "passed": false},
            {"passed": false},
        ],
    });

    assert_eq!(failed_gate_names(&record), vec!["clippy"]);
}
