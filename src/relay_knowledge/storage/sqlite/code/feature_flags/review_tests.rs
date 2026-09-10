//! Regression cases from the configuration registry code review.
use super::*;
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
