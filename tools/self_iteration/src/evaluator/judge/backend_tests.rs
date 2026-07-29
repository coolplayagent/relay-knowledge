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
