use serde_json::json;

use super::*;

#[test]
fn adoption_requires_a_committed_revision() {
    assert!(adopted(&json!({"committed": true})));
    assert!(adopted(&json!({"commit": "abc123"})));
    assert!(!adopted(&json!({
        "accepted": true,
        "score_accepted": true,
        "committed": false,
    })));
}

#[test]
fn manual_evaluation_state_is_not_an_automated_baseline() {
    let run = json!({"run_id": "manual-evaluate-123", "generated_diff": true});

    assert!(is_evaluate_run(&run));
    assert!(!automated_baseline_run(&run));
    assert_eq!(run_mode(&run), "evaluate");
}

#[test]
fn adoption_status_distinguishes_score_acceptance_from_commit() {
    assert_eq!(adoption_status(true, true), "committed");
    assert_eq!(adoption_status(false, true), "would_accept");
    assert_eq!(adoption_status(false, false), "rejected");
}
