//! Regression cases from the configuration registry code review.
use super::*;
#[test]
fn loading_stops_at_byte_budget_before_later_malformed_rows() {
    let db = fixture();
    let metadata = CodeConfigMetadata {
        flow_incomplete: Some("x".repeat(60_000)),
        ..Default::default()
    };
    for _ in 0..300 {
        add(&db, "flag", "config_key", "reads_config", metadata.clone());
    }
    add(
        &db,
        "invalid",
        "config_key",
        "reads_config",
        CodeConfigMetadata::default(),
    );
    db.execute("UPDATE code_repository_feature_flags SET metadata_json='invalid' WHERE source_key='invalid'", []).unwrap();
    let error = load(
        &db,
        &format!("SELECT {COLUMNS} FROM code_repository_feature_flags flag ORDER BY flag.rowid"),
        &[],
    )
    .map(|_| ())
    .expect_err("oversized stream must fail before the malformed tail");
    assert!(
        error.to_string().contains("16 MiB fact budget exceeded"),
        "{error}"
    );
}

#[test]
fn unicode_domain_filters_match_normalized_annotation_evidence() {
    let db = fixture();
    add(
        &db,
        "flag",
        "config_key",
        "defines_config",
        CodeConfigMetadata {
            domain: Some("über".into()),
            ..Default::default()
        },
    );
    let groups = search(
        &db,
        &status(),
        &request(
            None,
            CodeConfigFilter {
                domain: Some("ÜBER".into()),
                ..Default::default()
            },
        ),
    )
    .unwrap();
    assert_eq!(groups.len(), 1);
}
#[test]
fn path_queries_load_all_selected_key_evidence_before_consistency() {
    for symbolic in [false, true] {
        let db = fixture();
        add(
            &db,
            "feature_x",
            "config_key",
            "reads_config",
            CodeConfigMetadata {
                source_format: "java".into(),
                default_value: Some("false".into()),
                bindings: if symbolic {
                    vec!["Config.getX".into()]
                } else {
                    vec![]
                },
                ..Default::default()
            },
        );
        if symbolic {
            add(
                &db,
                "Config.getX",
                "config_symbol",
                "guards_code",
                CodeConfigMetadata {
                    reference: Some("Config.getX".into()),
                    ..Default::default()
                },
            );
        }
        db.execute("UPDATE code_repository_feature_flags SET path='src/Reader.java' WHERE rowid=(SELECT MAX(rowid) FROM code_repository_feature_flags)", []).unwrap();
        add(
            &db,
            "feature_x",
            "config_key",
            "defines_config",
            CodeConfigMetadata {
                source_format: "properties".into(),
                default_value: Some("true".into()),
                ..Default::default()
            },
        );
        let groups = search(
            &db,
            &status(),
            &request(
                Some("Reader"),
                CodeConfigFilter {
                    consistency: true,
                    ..Default::default()
                },
            ),
        )
        .unwrap();
        assert_eq!(groups.len(), 1);
        assert!(groups[0].analysis_complete);
        assert!(
            groups[0]
                .usages
                .iter()
                .any(|u| u.edge_kind == "defines_config")
        );
        assert_eq!(groups[0].conflicting_default_sources.len(), 2);
        assert!(
            !groups[0]
                .consistency_diagnostics
                .iter()
                .any(|d| d == "read_without_definition")
        );
    }
}

#[test]
fn getter_usage_paths_seed_queries_before_group_metadata_filters() {
    let db = fixture();
    add(
        &db,
        "feature_x",
        "config_key",
        "reads_config",
        CodeConfigMetadata {
            bindings: vec!["Config.getX".into()],
            domain: Some("business".into()),
            ..Default::default()
        },
    );
    add(
        &db,
        "Config.getX",
        "config_symbol",
        "guards_code",
        CodeConfigMetadata {
            reference: Some("Config.getX".into()),
            ..Default::default()
        },
    );
    db.execute("UPDATE code_repository_feature_flags SET path='src/Reader.java',excerpt='if (configuration.getX())' WHERE source_kind='config_symbol'", []).unwrap();
    for term in ["Reader", "configuration"] {
        let groups = search(
            &db,
            &status(),
            &request(
                Some(term),
                CodeConfigFilter {
                    domain: Some("business".into()),
                    ..Default::default()
                },
            ),
        )
        .unwrap();
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].source_key, "feature_x");
        assert_eq!(groups[0].usages.len(), 2);
    }
}

#[test]
fn referenced_constants_join_each_observed_namespace_without_a_synthetic_group() {
    for declaration in ["declares_config_key", "declares_string_constant"] {
        let db = fixture();
        add(
            &db,
            "HOME",
            "config_key",
            declaration,
            CodeConfigMetadata {
                bindings: vec!["Keys.HOME_KEY".into()],
                ..Default::default()
            },
        );
        add(
            &db,
            "Keys.HOME_KEY",
            "config_symbol",
            "reads_config",
            CodeConfigMetadata {
                reference: Some("Keys.HOME_KEY".into()),
                target_kind: Some("env_var".into()),
                bindings: vec!["Config.getHome".into()],
                ..Default::default()
            },
        );
        add(
            &db,
            "Config.getHome",
            "config_symbol",
            "guards_code",
            CodeConfigMetadata {
                reference: Some("Config.getHome".into()),
                ..Default::default()
            },
        );
        let mut query = request(None, CodeConfigFilter::default());
        query.limit = 10;
        let groups = search(&db, &status(), &query).unwrap();
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].source_kind, "env_var");
        assert_eq!(groups[0].usages.len(), 3);
        assert!(
            groups[0]
                .usages
                .iter()
                .any(|u| u.edge_kind == "declares_config_key")
        );
        add(
            &db,
            "Keys.HOME_KEY",
            "config_symbol",
            "reads_config",
            CodeConfigMetadata {
                reference: Some("Keys.HOME_KEY".into()),
                target_kind: Some("config_key".into()),
                ..Default::default()
            },
        );
        let groups = search(&db, &status(), &query).unwrap();
        assert_eq!(groups.len(), 2);
        assert!(groups.iter().all(|g| {
            g.analysis_complete
                && g.usages
                    .iter()
                    .any(|u| u.edge_kind == "declares_config_key")
        }));
    }
}

#[test]
fn repeated_interface_usages_reuse_one_resolution_and_evidence_entry() {
    let db = fixture();
    for _ in 0..2000 {
        add(
            &db,
            "feature_x",
            "config_key",
            "reads_config",
            CodeConfigMetadata {
                bindings: vec!["Config.getX".into()],
                ..Default::default()
            },
        );
    }
    add(
        &db,
        "Config.getX",
        "config_symbol",
        "guards_code",
        CodeConfigMetadata {
            reference: Some("Config.getX".into()),
            ..Default::default()
        },
    );
    let rows = load(
        &db,
        &format!("SELECT {COLUMNS} FROM code_repository_feature_flags flag"),
        &[],
    )
    .unwrap();
    let providers = HashMap::from([("Config.getX".into(), (0..2000).collect())]);
    let mut resolver = resolution::Resolver {
        rows: &rows,
        providers: &providers,
        targets: HashMap::new(),
        evidence: HashMap::new(),
    };
    for _ in 0..2000 {
        assert_eq!(
            resolver.resolve(&rows[2000], 0),
            Some(("config_key".into(), "feature_x".into()))
        );
        assert!(resolver.has_config_evidence("Config.getX", 0));
    }
    assert_eq!(resolver.targets.len(), 1);
    assert_eq!(resolver.evidence.len(), 1);
}

#[test]
fn ambiguous_bindings_only_suppress_connected_groups() {
    let db = fixture();
    for key in ["feature_a", "feature_b"] {
        add(
            &db,
            key,
            "config_key",
            "reads_config",
            CodeConfigMetadata {
                bindings: vec!["Config.getX".into()],
                ..Default::default()
            },
        );
    }
    add(
        &db,
        "Config.getX",
        "config_symbol",
        "guards_code",
        CodeConfigMetadata {
            reference: Some("Config.getX".into()),
            ..Default::default()
        },
    );
    add(
        &db,
        "feature_c",
        "config_key",
        "reads_config",
        CodeConfigMetadata::default(),
    );
    let mut query = request(
        None,
        CodeConfigFilter {
            consistency: true,
            ..Default::default()
        },
    );
    query.limit = 10;
    let groups = search(&db, &status(), &query).unwrap();
    for key in ["feature_a", "feature_b", "Config.getX"] {
        assert!(
            !groups
                .iter()
                .find(|g| g.source_key == key)
                .unwrap()
                .analysis_complete
        );
    }
    let independent = groups.iter().find(|g| g.source_key == "feature_c").unwrap();
    assert!(independent.analysis_complete);
    assert!(
        independent
            .consistency_diagnostics
            .iter()
            .any(|d| d.contains("read_without_definition"))
    );
}

#[test]
fn unicode_query_matches_uppercase_keys_before_sql_seed_selection() {
    let db = fixture();
    add(
        &db,
        "ÜBER_FLAG",
        "config_key",
        "defines_config",
        CodeConfigMetadata::default(),
    );
    for term in ["über", "ÜBER"] {
        let groups = search(
            &db,
            &status(),
            &request(Some(term), CodeConfigFilter::default()),
        )
        .unwrap();
        assert_eq!(groups[0].source_key, "ÜBER_FLAG");
    }
}

#[test]
fn final_expansion_round_rejects_new_outer_getter_bindings() {
    let db = fixture();
    add(
        &db,
        "feature_x",
        "config_key",
        "reads_config",
        CodeConfigMetadata {
            bindings: vec!["Layer0.getX".into()],
            ..Default::default()
        },
    );
    for index in 1..=5 {
        add(
            &db,
            &format!("Layer{}.getX", index - 1),
            "config_symbol",
            "reads_config",
            CodeConfigMetadata {
                reference: Some(format!("Layer{}.getX", index - 1)),
                bindings: vec![format!("Layer{index}.getX")],
                ..Default::default()
            },
        );
    }
    let error = search(
        &db,
        &status(),
        &request(Some("feature_x"), CodeConfigFilter::default()),
    )
    .unwrap_err();
    assert!(error.to_string().contains("symbol binding depth exceeded"));
}

#[test]
fn result_limit_counts_resolved_keys_instead_of_symbolic_seed_groups() {
    let db = fixture();
    for name in ["Keys.A", "Keys.B", "Keys.C"] {
        add(
            &db,
            "same_key",
            "config_key",
            "declares_config_key",
            CodeConfigMetadata {
                bindings: vec![name.into()],
                ..Default::default()
            },
        );
        add(
            &db,
            name,
            "config_symbol",
            "guards_code",
            CodeConfigMetadata {
                reference: Some(name.into()),
                target_kind: Some("config_key".into()),
                ..Default::default()
            },
        );
    }
    add(
        &db,
        "second_key",
        "config_key",
        "defines_config",
        CodeConfigMetadata::default(),
    );
    let mut query = request(None, CodeConfigFilter::default());
    query.limit = 2;
    let groups = search(&db, &status(), &query).unwrap();
    assert_eq!(groups.len(), 2);
    assert!(groups.iter().any(|g| g.source_key == "second_key"));
}
#[test]
fn expanded_and_consistency_usages_keep_containing_symbols() {
    let db = fixture();
    add(
        &db,
        "feature_x",
        "config_key",
        "reads_config",
        CodeConfigMetadata {
            source_format: "java".into(),
            bindings: vec!["Config.getX".into()],
            ..Default::default()
        },
    );
    add(
        &db,
        "Config.getX",
        "config_symbol",
        "guards_code",
        CodeConfigMetadata {
            source_format: "java".into(),
            reference: Some("Config.getX".into()),
            ..Default::default()
        },
    );
    db.execute_batch("INSERT INTO code_repository_symbols SELECT 'scope',path,0,999,'symbol:method','getX' FROM code_repository_feature_flags GROUP BY path;").unwrap();
    for consistency in [false, true] {
        let groups = search(
            &db,
            &status(),
            &request(
                Some("feature_x"),
                CodeConfigFilter {
                    consistency,
                    ..Default::default()
                },
            ),
        )
        .unwrap();
        assert_eq!(groups[0].usages.len(), 2);
        assert!(
            groups[0]
                .usages
                .iter()
                .all(
                    |u| u.related_symbol_snapshot_id.as_deref() == Some("symbol:method")
                        && u.related_symbol_name.as_deref() == Some("getX")
                )
        );
    }
}
#[test]
fn interrupted_sql_reports_actionable_incomplete_analysis() {
    let db = fixture();
    db.progress_handler(1, Some(|| true));
    let error = load(
        &db,
        &format!("SELECT {COLUMNS} FROM code_repository_feature_flags flag"),
        &[],
    )
    .map(|_| ())
    .expect_err("query should fail");
    assert!(
        matches!(error,StorageError::InvalidInput(ref message) if message.contains("incomplete") && message.contains("narrow")),
        "{error:?}"
    );
    db.progress_handler(0, None::<fn() -> bool>);
    let error = load(
        &db,
        "SELECT missing_column FROM code_repository_feature_flags",
        &[],
    )
    .map(|_| ())
    .expect_err("query should fail");
    assert!(matches!(error, StorageError::Sqlite(_)));
}

#[test]
fn exact_consistency_query_ignores_unrelated_rows_beyond_the_global_budget() {
    let db = fixture();
    db.execute_batch("WITH RECURSIVE n(x) AS (SELECT 1 UNION ALL SELECT x+1 FROM n WHERE x<10001)
      INSERT INTO code_repository_feature_flags SELECT 'unused:'||x,'usage:'||x,'file','src/unused','java','unrelated_'||x,'config_key','unrelated_'||x,'declares_string_constant',9000,'extracted',0,1,1,1,'unrelated','{}','scope' FROM n;").unwrap();
    add(
        &db,
        "selected_key",
        "config_key",
        "reads_config",
        CodeConfigMetadata {
            source_format: "java".into(),
            ..Default::default()
        },
    );
    db.execute_batch(
        "INSERT INTO code_repository_files VALUES ('scope','empty.ctmpl','gotemplate');",
    )
    .unwrap();
    let groups = search(
        &db,
        &status(),
        &request(
            Some("selected_key"),
            CodeConfigFilter {
                consistency: true,
                ..Default::default()
            },
        ),
    )
    .unwrap();
    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].source_key, "selected_key");
    assert!(
        groups[0]
            .consistency_diagnostics
            .contains(&"missing_from_format: ctmpl".into())
    );
}
