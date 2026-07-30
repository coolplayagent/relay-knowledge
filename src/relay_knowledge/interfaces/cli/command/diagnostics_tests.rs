use super::*;

#[test]
fn usage_and_runtime_failures_map_to_distinct_exit_codes() {
    assert_eq!(CliError::MissingValue("--query").exit_code(), 2);
    assert_eq!(
        CliError::RuntimeConfigFailed("invalid config".to_owned()).exit_code(),
        1
    );
}

#[test]
fn api_failures_preserve_structured_machine_readable_stderr() {
    let error = CliError::api_failed(
        ApiError::invalid_argument("invalid repository"),
        OutputFormat::Json,
    );
    let rendered: serde_json::Value =
        serde_json::from_str(&error.render_stderr()).expect("API stderr should be JSON");

    assert_eq!(rendered["error_kind"], "invalid_argument");
    assert_eq!(rendered["message"], "invalid repository");
}

#[test]
fn parse_diagnostics_render_context_in_text_and_json_formats() {
    let text = CliError::Diagnostic(Box::new(CliDiagnostic::new(
        "unknown command".to_owned(),
        Some("relay-knowledge help".to_owned()),
        Some("relay-knowledge status".to_owned()),
        vec!["repo".to_owned()],
        Some("stats".to_owned()),
        vec!["status".to_owned()],
        OutputFormat::Text,
    )));
    let json = CliError::Diagnostic(Box::new(CliDiagnostic::new(
        "unknown command".to_owned(),
        Some("relay-knowledge help".to_owned()),
        None,
        vec!["repo".to_owned()],
        Some("stats".to_owned()),
        vec!["status".to_owned()],
        OutputFormat::Json,
    )));

    assert_eq!(
        text.render_stderr(),
        "unknown command\nTry: relay-knowledge status\nUsage: relay-knowledge help"
    );
    let rendered: serde_json::Value =
        serde_json::from_str(&json.render_stderr()).expect("diagnostic stderr should be JSON");
    assert_eq!(rendered["matched_path"], serde_json::json!(["repo"]));
    assert_eq!(rendered["unexpected_token"], "stats");
    assert_eq!(rendered["expected"], serde_json::json!(["status"]));
}
