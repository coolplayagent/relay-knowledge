mod tests {
    use super::*;

    #[test]
    fn shell_split_keeps_quoted_argument() {
        assert_eq!(
            shell_split("tool run \"hello world\" --file {prompt_file}").expect("split"),
            vec!["tool", "run", "hello world", "--file", "{prompt_file}"]
        );
    }

    #[test]
    fn judge_defaults_to_opencode_cli_agent() {
        let settings = judge_settings(&BTreeMap::new());
        assert!(settings.enabled);
        assert_eq!(settings.backend, JudgeBackend::Cli);
        assert!(settings.command.starts_with("opencode run "));
        assert!(settings.missing.is_empty());
    }

    #[test]
    fn judge_uses_openai_compatible_http_when_configured() {
        let env = BTreeMap::from([
            (
                "RELAY_KNOWLEDGE_JUDGE_BASE_URL".to_owned(),
                "http://localhost:11434/v1".to_owned(),
            ),
            ("RELAY_KNOWLEDGE_JUDGE_API_KEY".to_owned(), "token".to_owned()),
            (
                "RELAY_KNOWLEDGE_JUDGE_MODEL".to_owned(),
                "judge-model".to_owned(),
            ),
        ]);
        let settings = judge_settings(&env);
        assert_eq!(settings.backend, JudgeBackend::Http);
        assert!(settings.missing.is_empty());
        assert_eq!(
            normalize_judge_chat_url(&settings.http_base_url),
            "http://localhost:11434/v1/chat/completions"
        );
        let (command, body) = judge_http_command(&settings, "judge prompt").expect("http command");
        assert!(!command.join(" ").contains("token"));
        assert!(body.contains("judge-model"));
        assert!(body.contains("judge prompt"));
    }

    #[test]
    fn judge_backend_http_env_selects_http_runner() {
        let env = BTreeMap::from([
            (
                "RELAY_KNOWLEDGE_JUDGE_BACKEND".to_owned(),
                "http".to_owned(),
            ),
            (
                "RELAY_KNOWLEDGE_JUDGE_BASE_URL".to_owned(),
                "http://localhost:11434".to_owned(),
            ),
            ("RELAY_KNOWLEDGE_JUDGE_API_KEY".to_owned(), "token".to_owned()),
            (
                "RELAY_KNOWLEDGE_JUDGE_MODEL".to_owned(),
                "judge-model".to_owned(),
            ),
        ]);
        let settings = judge_settings(&env);
        assert_eq!(settings.backend, JudgeBackend::Http);
        assert_eq!(settings_summary(&settings)["backend"], "http");
    }

    #[test]
    fn judge_rejects_unsupported_backend() {
        let env = BTreeMap::from([(
            "RELAY_KNOWLEDGE_JUDGE_BACKEND".to_owned(),
            "httpp".to_owned(),
        )]);

        let settings = judge_settings(&env);

        assert!(settings.configuration_error.is_some());
        assert!(!settings_summary(&settings)["configured"]
            .as_bool()
            .expect("configured should be boolean"));
    }

    #[test]
    fn explicit_cli_judge_command_wins_over_stray_http_env() {
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
        assert!(observation.message.contains("truncated=false expected=true"));
        assert!(observation.message.contains("missing=budget"));
    }

    #[test]
    fn malformed_json_fails_file_case() {
        let case = serde_json::json!({"id": "negative", "expect_empty": true});
        let result = CommandResult {
            name: "files_query".to_owned(),
            command: vec!["relay-knowledge".to_owned()],
            exit_code: 0,
            duration_ms: 1,
            stdout: "not json".to_owned(),
            stderr: String::new(),
        };

        let observation = score_file_case("fixture", &case, &result);

        assert!(!observation.passed);
        assert!(observation.message.contains("invalid JSON"));
    }

    #[test]
    fn percentile_selects_expected_rank() {
        assert_eq!(percentile(&[10, 20, 30, 40], 50), 20);
        assert_eq!(percentile(&[10, 20, 30, 40], 95), 30);
    }
}
