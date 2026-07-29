    #[test]
    fn judge_defaults_to_opencode_cli_agent() {
        let settings = judge_settings(&BTreeMap::new());
        assert!(settings.enabled);
        assert_eq!(settings.backend, JudgeBackend::Cli);
        assert!(settings.command.starts_with("opencode run "));
        assert!(settings.missing.is_empty());
    }
