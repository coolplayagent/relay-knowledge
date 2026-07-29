    #[test]
    fn parses_evaluate_with_jobs() {
        let config = Config::parse(vec![
            "evaluate".to_owned(),
            "--profile".to_owned(),
            "smoke".to_owned(),
            "--jobs=2".to_owned(),
            "--repo-jobs".to_owned(),
            "1".to_owned(),
        ])
        .expect("config should parse");

        assert_eq!(config.mode, Mode::Evaluate);
        assert_eq!(config.profile, "smoke");
        assert_eq!(config.model.as_deref(), Some(DEFAULT_CODEX_MODEL));
        assert_eq!(
            config.codex_reasoning_effort,
            DEFAULT_CODEX_REASONING_EFFORT
        );
        assert_eq!(config.jobs, Jobs::Fixed(2));
        assert_eq!(config.repo_jobs, Jobs::Fixed(1));
    }

    #[test]
    fn parses_codex_generation_overrides() {
        let config = Config::parse(vec![
            "once".to_owned(),
            "--model=o3".to_owned(),
            "--codex-reasoning-effort".to_owned(),
            "high".to_owned(),
        ])
        .expect("config should parse");

        assert_eq!(config.model.as_deref(), Some("o3"));
        assert_eq!(config.codex_reasoning_effort, "high");
    }

    #[test]
    fn parses_research_plan_options() {
        let config = Config::parse(vec![
            "research-plan".to_owned(),
            "--research-topic".to_owned(),
            "2026 graph database research".to_owned(),
            "--research-slug=graph-database-research".to_owned(),
            "--research-date".to_owned(),
            "2026-06-05".to_owned(),
        ])
        .expect("config should parse");

        assert_eq!(config.mode, Mode::ResearchPlan);
        assert_eq!(config.research_topic, "2026 graph database research");
        assert_eq!(config.research_slug, "graph-database-research");
        assert_eq!(config.research_date, "2026-06-05");
    }

    #[test]
    fn rejects_invalid_research_plan_metadata() {
        let invalid_slug = Config::parse(vec![
            "research-plan".to_owned(),
            "--research-slug=Graph DB".to_owned(),
        ])
        .expect_err("invalid slug should fail");
        let invalid_date = Config::parse(vec![
            "research-plan".to_owned(),
            "--research-date".to_owned(),
            "20260605".to_owned(),
        ])
        .expect_err("invalid date should fail");

        assert!(invalid_slug.contains("research-slug"));
        assert!(invalid_date.contains("YYYY-MM-DD"));
    }

    #[test]
    fn rejects_invalid_codex_reasoning_effort() {
        let error = Config::parse(vec![
            "once".to_owned(),
            "--codex-reasoning-effort".to_owned(),
            "extreme".to_owned(),
        ])
        .expect_err("invalid effort should fail");

        assert!(error.contains("invalid codex reasoning effort"));
    }
