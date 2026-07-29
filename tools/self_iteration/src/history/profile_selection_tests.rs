    #[test]
    fn profile_best_accepted_ignores_category_focus() {
        let workspace = temp_workspace("history-profile-best");
        let paths = HistoryPaths::new(&workspace);
        paths.ensure().expect("history paths");
        let runs = [
            json!({
                "run_id": "run-competitive",
                "timestamp": "1",
                "profile": "fast",
                "category_focus": "competitive",
                "accepted": true,
                "committed": true,
                "commit": "competitive123",
                "score": 0.84
            }),
            json!({
                "run_id": "run-semantic",
                "timestamp": "2",
                "profile": "fast",
                "category_focus": "semantic_vector",
                "accepted": true,
                "committed": true,
                "commit": "semantic123",
                "score": 0.95
            }),
            json!({
                "run_id": "run-performance",
                "timestamp": "3",
                "profile": "fast",
                "category_focus": "performance",
                "accepted": false,
                "committed": false,
                "score": 0.99
            }),
            json!({
                "run_id": "run-full",
                "timestamp": "4",
                "profile": "full",
                "category_focus": "competitive",
                "accepted": true,
                "committed": true,
                "commit": "full123",
                "score": 0.98
            }),
        ];
        fs::write(
            &paths.runs_jsonl,
            runs.iter()
                .map(Value::to_string)
                .collect::<Vec<_>>()
                .join("\n"),
        )
        .expect("runs");

        let best = best_accepted_run_for_profile(&paths, "fast")
            .expect("history")
            .expect("profile best");

        assert_eq!(
            best.get("run_id").and_then(Value::as_str),
            Some("run-semantic")
        );
    }
