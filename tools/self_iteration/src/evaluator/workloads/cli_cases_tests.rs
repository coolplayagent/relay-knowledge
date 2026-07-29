use serde_json::Value;

use super::{
    register_command, score_cli_contract_case, score_registration_case,
    select_cli_contract_cases_for_profile, select_registration_cases_for_profile,
};
use crate::{cases::string_or, command::CommandResult};

#[test]
fn registration_case_scores_expected_failure_message() {
    let case = serde_json::json!({
        "id": "reject_register_language",
        "expect_failure": true,
        "stderr_contains": "registration language filters are not supported",
        "guardrail": true
    });
    let result = CommandResult {
        name: "register".to_owned(),
        command: vec!["relay-knowledge".to_owned()],
        exit_code: 1,
        duration_ms: 1,
        stdout: String::new(),
        stderr: "registration language filters are not supported; use query-time --language"
            .to_owned(),
    };

    let observation = score_registration_case("fixture", &case, &result);

    assert!(observation.passed);
    assert!(observation.guardrail);
    assert_eq!(observation.score_override, Some(1.0));
}

#[test]
fn cli_contract_case_scores_idle_index_worker_json() {
    let case = serde_json::json!({
        "id": "repo_index_worker_idle_json_reports_no_claim",
        "guardrail": true,
        "json_expect": {
            "claimed": false,
            "task": null
        }
    });
    let result = CommandResult {
        name: "index_worker".to_owned(),
        command: vec!["relay-knowledge".to_owned()],
        exit_code: 0,
        duration_ms: 1,
        stdout: "{\"claimed\":false,\"task\":null}\n".to_owned(),
        stderr: String::new(),
    };

    let observation = score_cli_contract_case(&case, &result);

    assert!(observation.passed, "{}", observation.message);
    assert!(observation.guardrail);
    assert_eq!(observation.repository, "cli_contract");
}

#[test]
fn cli_contract_case_scores_idle_index_worker_stream() {
    let case = serde_json::json!({
        "id": "repo_index_worker_idle_streaming_json_reports_events",
        "guardrail": true,
        "json_lines_expect": [
            {"event": "started", "operation": "code.repo.index_worker"},
            {
                "event": "item",
                "operation": "code.repo.index_worker",
                "payload": {
                    "claimed": false,
                    "task": null
                }
            },
            {"event": "completed", "operation": "code.repo.index_worker"}
        ]
    });
    let result = CommandResult {
            name: "index_worker_stream".to_owned(),
            command: vec!["relay-knowledge".to_owned()],
            exit_code: 0,
            duration_ms: 1,
            stdout: concat!(
                "{\"event\":\"started\",\"operation\":\"code.repo.index_worker\"}\n",
                "{\"event\":\"item\",\"operation\":\"code.repo.index_worker\",\"payload\":{\"claimed\":false,\"task\":null}}\n",
                "{\"event\":\"completed\",\"operation\":\"code.repo.index_worker\"}\n"
            )
            .to_owned(),
            stderr: String::new(),
        };

    let observation = score_cli_contract_case(&case, &result);

    assert!(observation.passed, "{}", observation.message);
    assert!(observation.guardrail);
}

#[test]
fn cli_contract_cases_are_preserved_for_fast() {
    let cases = vec![
        serde_json::json!({"id": "regular"}),
        serde_json::json!({
            "id": "repo_index_worker_idle_json_reports_no_claim",
            "guardrail": true
        }),
    ];

    let selected = select_cli_contract_cases_for_profile("fast", None, cases);

    assert!(selected.iter().any(|case| {
        string_or(case, "id", "") == "repo_index_worker_idle_json_reports_no_claim"
            && case.get("guardrail").and_then(Value::as_bool) == Some(true)
    }));
}

#[test]
fn register_command_can_omit_alias_for_default_project_name() {
    let binary = std::path::Path::new("relay-knowledge");
    let root = std::path::Path::new("/work/project");

    assert_eq!(
        register_command(binary, root, None),
        vec![
            "relay-knowledge",
            "repo",
            "register",
            "/work/project",
            "--format",
            "json"
        ]
    );
    assert_eq!(
        register_command(binary, root, Some("stable")),
        vec![
            "relay-knowledge",
            "repo",
            "register",
            "/work/project",
            "--alias",
            "stable",
            "--format",
            "json"
        ]
    );
}

#[test]
fn registration_guardrail_cases_are_preserved_for_fast() {
    let cases = vec![
        serde_json::json!({"id": "regular", "repository": "fixture"}),
        serde_json::json!({
            "id": "reject_register_language",
            "repository": "fixture",
            "expect_failure": true,
            "language_filters": ["cpp"],
            "guardrail": true
        }),
    ];

    let selected = select_registration_cases_for_profile("fast", None, cases);

    assert!(selected.iter().any(|case| {
        string_or(case, "id", "") == "reject_register_language"
            && case.get("guardrail").and_then(Value::as_bool) == Some(true)
    }));
}
