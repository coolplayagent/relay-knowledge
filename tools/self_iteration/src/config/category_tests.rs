    #[test]
    fn parses_focus_categories() {
        let config = Config::parse(vec![
            "once".to_owned(),
            "--categories".to_owned(),
            "semantic_vector,competitive".to_owned(),
        ])
        .expect("config should parse");
        let labels = config
            .categories
            .as_ref()
            .expect("categories should be set")
            .labels();

        assert_eq!(labels, vec!["competitive", "semantic_vector"]);
        assert_eq!(
            config.category_focus_key().as_deref(),
            Some("competitive,semantic_vector")
        );
    }

    #[test]
    fn parses_all_focus_categories() {
        let config = Config::parse(vec!["evaluate".to_owned(), "--categories=all".to_owned()])
            .expect("config should parse");
        let labels = config
            .categories
            .as_ref()
            .expect("categories should be set")
            .labels();

        assert_eq!(
            labels,
            vec![
                "foundational",
                "competitive",
                "semantic_vector",
                "file_fixtures",
                "repository_sets",
                "agent_workflows",
                "research_judge",
                "performance"
            ]
        );
    }

    #[test]
    fn excludes_categories_after_all_expansion() {
        let config = Config::parse(vec![
            "evaluate".to_owned(),
            "--categories=all".to_owned(),
            "--exclude-categories=research_judge".to_owned(),
        ])
        .expect("config should parse");
        let labels = config
            .categories
            .as_ref()
            .expect("categories should be set")
            .labels();

        assert_eq!(
            labels,
            vec![
                "foundational",
                "competitive",
                "semantic_vector",
                "file_fixtures",
                "repository_sets",
                "agent_workflows",
                "performance"
            ]
        );
        assert_eq!(
            config.category_focus_key().as_deref(),
            Some(
                "foundational,competitive,semantic_vector,file_fixtures,repository_sets,agent_workflows,performance"
            )
        );
    }

    #[test]
    fn exclude_categories_without_focus_selects_all_remaining_categories() {
        let config = Config::parse(vec![
            "evaluate".to_owned(),
            "--exclude-categories".to_owned(),
            "judge".to_owned(),
        ])
        .expect("config should parse");
        let labels = config
            .categories
            .as_ref()
            .expect("categories should be set")
            .labels();

        assert_eq!(
            labels,
            vec![
                "foundational",
                "competitive",
                "semantic_vector",
                "file_fixtures",
                "repository_sets",
                "agent_workflows",
                "performance"
            ]
        );
    }

    #[test]
    fn rejects_excluding_all_selected_categories() {
        let error = Config::parse(vec![
            "evaluate".to_owned(),
            "--categories".to_owned(),
            "research_judge".to_owned(),
            "--exclude-categories".to_owned(),
            "research_judge".to_owned(),
        ])
        .expect_err("empty selected categories should fail");

        assert!(error.contains("removed all selected categories"));
    }

    #[test]
    fn rejects_invalid_focus_category() {
        let error = Config::parse(vec![
            "evaluate".to_owned(),
            "--categories".to_owned(),
            "semantic_vector,nope".to_owned(),
        ])
        .expect_err("invalid category should fail");

        assert!(error.contains("invalid evaluation category"));
    }

    #[test]
    fn rejects_invalid_excluded_category() {
        let error = Config::parse(vec![
            "evaluate".to_owned(),
            "--exclude-categories".to_owned(),
            "nope".to_owned(),
        ])
        .expect_err("invalid category should fail");

        assert!(error.contains("invalid evaluation category"));
    }
