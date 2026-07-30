use super::*;

#[test]
fn codex_result_reports_status_and_bounded_serialization() {
    let mut result = CodexResult {
        command: vec!["codex".to_owned(), "exec".to_owned()],
        exit_code: 0,
        duration_ms: 42,
        stdout: "o".repeat(4_100),
        stderr: "error".to_owned(),
    };

    assert!(result.succeeded());
    let value = result.serializable();
    assert_eq!(value["command"], serde_json::json!(["codex", "exec"]));
    assert_eq!(
        value["stdout_tail"]
            .as_str()
            .expect("stdout tail should be a string")
            .chars()
            .count(),
        4_000
    );

    result.exit_code = 1;
    assert!(!result.succeeded());
}
