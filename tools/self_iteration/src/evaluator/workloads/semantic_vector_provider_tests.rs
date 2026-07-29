    #[test]
    fn provider_probe_ok_false_fails_gate() {
        let mut result = CommandResult {
            name: "semantic_vector_provider_probe".to_owned(),
            command: vec!["relay-knowledge".to_owned()],
            exit_code: 0,
            duration_ms: 1,
            stdout: serde_json::json!({"ok": false, "error_code": "auth_failed"}).to_string(),
            stderr: String::new(),
        };

        assert!(!validate_provider_probe(&mut result));
        assert_eq!(result.exit_code, 1);
        assert_eq!(result.stderr, "auth_failed");
    }
