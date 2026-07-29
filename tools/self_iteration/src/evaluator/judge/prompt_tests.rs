    #[test]
    fn judge_prompt_truncates_diff_on_char_boundary_and_includes_targets() {
        let suite = serde_json::json!({
            "max_diff_chars": 1,
            "max_doc_chars": 1,
            "competitive_feature_targets": ["repo-set"],
            "implementation_guardrails": ["no fixture special casing"],
            "rubric_dimensions": ["research_alignment", "competitive_advantage"],
            "min_dimension_score": 0.7
        });

        let prompt = build_judge_prompt(JudgePromptInput {
            workspace: std::path::Path::new("."),
            suite: &suite,
            generated_diff: true,
            candidate_diff: "汉字",
            gates: &[],
            cases: &[],
            metrics: &[],
            repo_reports: &[],
        });

        assert!(prompt.contains("汉\n...diff truncated..."));
        assert!(prompt.contains("competitive_feature_targets"));
        assert!(prompt.contains("repo-set"));
        assert!(prompt.contains("implementation_guardrails"));
        assert!(prompt.contains("capability_delta"));
        assert!(prompt.contains("research_gaps"));
        assert!(prompt.contains("min_dimension_score"));
        assert!(prompt.contains("objective_scores"));
        assert!(prompt.contains("metric_budget_failures"));
        assert!(prompt.contains("competitive_advantage"));
    }
