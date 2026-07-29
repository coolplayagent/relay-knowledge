use super::{failed_case, parse_json_case_output, payload_constraint_failures};
use crate::command::CommandResult;

#[test]
fn payload_constraints_report_result_and_degradation_contract_failures() {
    let case = serde_json::json!({
        "max_results": 1,
        "truncated": false,
        "degraded_reason": null
    });
    let payload = serde_json::json!({
        "truncated": true,
        "degraded_reason": "stale index"
    });

    let failures = payload_constraint_failures(&case, &payload, 2);

    assert_eq!(failures.len(), 3);
    assert!(
        failures
            .iter()
            .any(|failure| failure.contains("max_results=1"))
    );
    assert!(
        failures
            .iter()
            .any(|failure| failure.contains("truncated=true"))
    );
    assert!(
        failures
            .iter()
            .any(|failure| failure.contains("degraded_reason=stale index"))
    );
}

#[test]
fn failed_case_preserves_guardrail_and_command_diagnostics() {
    let case = serde_json::json!({"id": "guard", "guardrail": true, "max_rank": 3});
    let result = CommandResult {
        name: "query".to_owned(),
        command: vec!["query".to_owned()],
        exit_code: 1,
        duration_ms: 2,
        stdout: String::new(),
        stderr: "query failed".to_owned(),
    };

    let observation = failed_case(&case, "repo", "objective", &result);

    assert!(!observation.passed);
    assert!(observation.guardrail);
    assert_eq!(observation.max_rank, 3);
    assert!(observation.message.contains("query failed"));
}

#[test]
fn malformed_json_maps_to_a_scored_case_failure() {
    let case = serde_json::json!({"id": "malformed", "guardrail": true});
    let result = CommandResult {
        name: "query".to_owned(),
        command: vec!["query".to_owned()],
        exit_code: 0,
        duration_ms: 1,
        stdout: "{invalid".to_owned(),
        stderr: String::new(),
    };

    let observation = parse_json_case_output(&case, "repo", "objective", &result)
        .expect_err("malformed JSON should fail");

    assert!(observation.guardrail);
    assert_eq!(observation.score_override, Some(0.0));
}
