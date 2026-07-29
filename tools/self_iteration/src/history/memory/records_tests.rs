use serde_json::json;

use super::*;

#[test]
fn regression_memory_classifies_semantic_vector_degradation() {
    let record = json!({
        "run_id": "semantic-regression",
        "timestamp": "2026-07-30T00:00:00Z",
        "accepted": false,
        "score": 0.7,
        "degradations": [{
            "objective": "semantic_vector",
            "name": "semantic recall",
        }],
    });

    let memory = regression_memory(&record).expect("regression memory");

    assert_eq!(memory["kind"], "semantic_vector_regression");
    assert_eq!(
        memory["id"],
        "semantic-regression-semantic_vector_regression-semantic-recall"
    );
    assert!(
        memory["summary"]
            .as_str()
            .expect("summary")
            .contains("semantic vector regression")
    );
}

#[test]
fn regression_memory_skips_records_without_degradations() {
    assert!(regression_memory(&json!({"degradations": []})).is_none());
    assert!(regression_memory(&json!({})).is_none());
}
