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
