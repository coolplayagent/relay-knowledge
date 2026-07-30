use std::collections::BTreeMap;

use super::{JudgeBackend, judge_settings, settings_summary, shell_split};

#[test]
fn judge_defaults_to_opencode_cli_agent() {
    let settings = judge_settings(&BTreeMap::new());

    assert!(settings.enabled);
    assert_eq!(settings.backend, JudgeBackend::Cli);
    assert!(settings.command.starts_with("opencode run "));
    assert!(settings.missing.is_empty());
}

#[test]
fn complete_http_environment_selects_http_backend() {
    let env = BTreeMap::from([
        (
            "RELAY_KNOWLEDGE_JUDGE_BASE_URL".to_owned(),
            "http://localhost:11434/v1".to_owned(),
        ),
        (
            "RELAY_KNOWLEDGE_JUDGE_API_KEY".to_owned(),
            "token".to_owned(),
        ),
        (
            "RELAY_KNOWLEDGE_JUDGE_MODEL".to_owned(),
            "judge-model".to_owned(),
        ),
    ]);

    let settings = judge_settings(&env);

    assert_eq!(settings.backend, JudgeBackend::Http);
    assert!(settings.missing.is_empty());
    assert_eq!(settings_summary(&settings)["backend"], "http");
}

#[test]
fn unsupported_backend_is_observable_as_misconfiguration() {
    let env = BTreeMap::from([(
        "RELAY_KNOWLEDGE_JUDGE_BACKEND".to_owned(),
        "httpp".to_owned(),
    )]);

    let settings = judge_settings(&env);

    assert!(settings.configuration_error.is_some());
    assert!(
        !settings_summary(&settings)["configured"]
            .as_bool()
            .expect("configured should be boolean")
    );
}

#[test]
fn explicit_cli_command_wins_over_partial_http_environment() {
    let env = BTreeMap::from([
        (
            "RELAY_KNOWLEDGE_JUDGE_BASE_URL".to_owned(),
            "http://localhost:11434".to_owned(),
        ),
        (
            "RELAY_KNOWLEDGE_JUDGE_COMMAND".to_owned(),
            "custom-judge --file {prompt_file}".to_owned(),
        ),
    ]);

    let settings = judge_settings(&env);

    assert_eq!(settings.backend, JudgeBackend::Cli);
    assert!(settings.missing.is_empty());
    assert_eq!(
        shell_split(&settings.command).expect("split").first(),
        Some(&"custom-judge".to_owned())
    );
}

#[test]
fn shell_split_keeps_quoted_argument_and_rejects_unterminated_quotes() {
    assert_eq!(
        shell_split("tool run \"hello world\" --file {prompt_file}").expect("split"),
        vec!["tool", "run", "hello world", "--file", "{prompt_file}"]
    );
    assert_eq!(
        shell_split("tool \"unterminated").expect_err("quote should fail"),
        "unterminated quote in command"
    );
}
