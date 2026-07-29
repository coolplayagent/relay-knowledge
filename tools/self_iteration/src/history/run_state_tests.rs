    #[test]
    fn automated_baseline_ignores_manual_evaluations() {
        let workspace = temp_workspace("history-baseline");
        let paths = HistoryPaths::new(&workspace);
        paths.ensure().expect("history paths");
        let runs = [
            json!({
                "run_id": "run-1",
                "timestamp": "1",
                "profile": "fast",
                "accepted": true,
                "score_accepted": true,
                "committed": true,
                "score": 0.8,
                "commit": "abc1234"
            }),
            json!({
                "run_id": "manual-evaluate-2",
                "timestamp": "2",
                "profile": "fast",
                "accepted": false,
                "score_accepted": true,
                "committed": false,
                "score": 0.99
            }),
            json!({
                "run_id": "run-no-diff-3",
                "timestamp": "3",
                "profile": "fast",
                "accepted": false,
                "score_accepted": false,
                "committed": false,
                "generated_diff": false,
                "score": 0.98
            }),
            json!({
                "run_id": "run-legacy-no-diff-4",
                "timestamp": "4",
                "profile": "fast",
                "accepted": false,
                "score_accepted": false,
                "committed": false,
                "reject_reasons": ["codex produced no candidate diff"],
                "score": 0.97
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

        let previous = previous_scored_run_for_workload(&paths, "fast", None)
            .expect("history")
            .expect("previous run");

        assert_eq!(
            previous.get("run_id").and_then(Value::as_str),
            Some("run-1")
        );
    }
