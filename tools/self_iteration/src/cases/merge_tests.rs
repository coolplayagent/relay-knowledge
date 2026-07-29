use super::*;

#[test]
fn merge_appends_arrays_and_merges_maps() {
    let mut target = serde_json::json!({
        "query_cases": [{"id": "one"}],
        "repositories": {"a": {"path": "/a"}}
    });
    let included = serde_json::json!({
        "query_cases": [{"id": "two"}],
        "repositories": {"b": {"path": "/b"}}
    });

    merge_case_config(&mut target, included).expect("merge should succeed");

    assert_eq!(array_field(&target, "query_cases").len(), 2);
    assert!(
        object_field(&target, "repositories")
            .expect("repositories should exist")
            .contains_key("b")
    );
}
