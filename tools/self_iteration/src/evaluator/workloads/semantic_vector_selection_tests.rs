    #[test]
    fn semantic_vector_selection_uses_guardrail_for_fast_default() {
        let suite = serde_json::json!({
            "query_cases": [
                {"id": "guardrail", "guardrail": true},
                {"id": "full"}
            ]
        });

        let selected = semantic_vector_suite_for_selection(&suite, "fast", None);
        let cases = array_field(&selected, "query_cases");

        assert_eq!(cases.len(), 1);
        assert_eq!(string_or(&cases[0], "id", ""), "guardrail");
    }

    #[test]
    fn semantic_vector_focus_runs_full_suite() {
        let categories = CategorySet::parse("semantic_vector").expect("categories should parse");
        let suite = serde_json::json!({
            "query_cases": [
                {"id": "guardrail", "guardrail": true},
                {"id": "full"}
            ]
        });

        let selected = semantic_vector_suite_for_selection(&suite, "fast", Some(&categories));

        assert_eq!(array_field(&selected, "query_cases").len(), 2);
    }
