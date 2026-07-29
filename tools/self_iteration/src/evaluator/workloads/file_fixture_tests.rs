use super::score_file_case;
use crate::command::CommandResult;

#[test]
fn file_case_enforces_payload_constraints() {
    let case = serde_json::json!({
        "id": "file_constraints",
        "max_results": 1,
        "truncated": true,
        "degraded_reason_contains": "budget",
        "expected": [{"relative_path": "a.md"}]
    });
    let result = CommandResult {
        name: "files_query".to_owned(),
        command: vec!["relay-knowledge".to_owned()],
        exit_code: 0,
        duration_ms: 1,
        stdout: serde_json::json!({
            "results": [{"relative_path": "a.md"}, {"relative_path": "b.md"}],
            "truncated": false,
            "degraded_reason": "stale"
        })
        .to_string(),
        stderr: String::new(),
    };

    let observation = score_file_case("fixture", &case, &result);

    assert!(!observation.passed);
    assert!(observation.message.contains("max_results=1"));
    assert!(
        observation
            .message
            .contains("truncated=false expected=true")
    );
    assert!(observation.message.contains("missing=budget"));
}

#[test]
fn malformed_json_fails_file_case() {
    let case = serde_json::json!({
        "id": "malformed_file_query",
        "expected": [{"path": "report.md"}]
    });
    let result = CommandResult {
        name: "file_query".to_owned(),
        command: vec!["relay-knowledge".to_owned()],
        exit_code: 0,
        duration_ms: 1,
        stdout: "{not-json".to_owned(),
        stderr: String::new(),
    };

    let observation = score_file_case("fixture", &case, &result);

    assert!(!observation.passed);
    assert!(observation.message.contains("invalid JSON output"));
}
