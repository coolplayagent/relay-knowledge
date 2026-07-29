    #[test]
    fn workload_baseline_matches_category_focus() {
        let workspace = temp_workspace("history-workload-baseline");
        let paths = HistoryPaths::new(&workspace);
        paths.ensure().expect("history paths");
        let runs = [
            json!({
                "run_id": "run-default",
                "timestamp": "1",
                "profile": "fast",
                "accepted": true,
                "score": 0.8
            }),
            json!({
                "run_id": "run-semantic",
                "timestamp": "2",
                "profile": "fast",
                "category_focus": "semantic_vector",
                "selected_categories": ["semantic_vector"],
                "accepted": true,
                "committed": true,
                "commit": "semantic123",
                "score": 0.9
            }),
            json!({
                "run_id": "run-competitive",
                "timestamp": "3",
                "profile": "fast",
                "category_focus": "competitive",
                "selected_categories": ["competitive"],
                "accepted": true,
                "committed": true,
                "commit": "competitive123",
                "score": 0.95
            }),
            json!({
                "run_id": "manual-evaluate-semantic",
                "timestamp": "4",
                "profile": "fast",
                "category_focus": "semantic_vector",
                "selected_categories": ["semantic_vector"],
                "accepted": false,
                "score": 0.99
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

        let default_previous = previous_scored_run_for_workload(&paths, "fast", None)
            .expect("history")
            .expect("default previous run");
        let semantic_previous =
            previous_scored_run_for_workload(&paths, "fast", Some("semantic_vector"))
                .expect("history")
                .expect("semantic previous run");

        assert_eq!(
            default_previous.get("run_id").and_then(Value::as_str),
            Some("run-default")
        );
        assert_eq!(
            semantic_previous.get("run_id").and_then(Value::as_str),
            Some("run-semantic")
        );
    }
