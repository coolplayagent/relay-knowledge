use super::*;
#[test]
fn inherited_constants_stop_at_hidden_fields() {
    for hidden in [false, true] {
        let db = fixture();
        for (path, content) in [
            (
                "Base.java",
                r#"package app; class Base { public static final String CONFIG_KEY="base_flag"; }"#,
            ),
            (
                "Child.java",
                if hidden {
                    "package app; class Child extends Base { public static String CONFIG_KEY=other(); }"
                } else {
                    "package app; class Child extends Base {}"
                },
            ),
            (
                "Reader.java",
                "package app; class Reader { String read() { return System.getProperty(Child.CONFIG_KEY); } }",
            ),
        ] {
            for row in crate::code::feature_flags::extract_feature_flags(
                crate::code::feature_flags::FeatureFlagFileInput {
                    repository_id: "repo",
                    source_scope: "scope",
                    file_id: "file",
                    path,
                    language_id: "java",
                    content,
                    config_facts: &[],
                },
            )
            .unwrap()
            {
                add(
                    &db,
                    &row.source_key,
                    &row.source_kind,
                    &row.edge_kind,
                    row.metadata,
                );
                db.execute("UPDATE code_repository_feature_flags SET path=? WHERE rowid=last_insert_rowid()", [path]).unwrap();
            }
        }
        let mut req = request(None, CodeConfigFilter::default());
        req.repository.path_filters = vec!["Reader.java".into()];
        let groups = search(&db, &status(), &req).unwrap();
        assert_eq!(groups.len(), 1);
        assert_eq!(
            groups[0].source_kind,
            if hidden {
                "config_symbol"
            } else {
                "config_key"
            }
        );
        if !hidden {
            assert_eq!(groups[0].source_key, "base_flag");
        }
    }
}
#[test]
fn exact_platform_read_ignores_unrelated_large_type_inventory() {
    let db = fixture();
    db.execute_batch("WITH RECURSIVE numbers(n) AS (VALUES(1) UNION ALL SELECT n+1 FROM numbers WHERE n<10001) INSERT INTO code_repository_feature_flags SELECT 'type'||n,'type'||n,'file','src/Other.java','java','Other'||n,'config_symbol','other.Type'||n,'config_type_declaration',9000,'extracted',0,1,1,1,'type','{}','scope' FROM numbers;").unwrap();
    add(
        &db,
        "feature_x",
        "config_key",
        "reads_config",
        CodeConfigMetadata {
            implicit_platform_owner: Some("app.System".into()),
            ..Default::default()
        },
    );
    let groups = search(
        &db,
        &status(),
        &request(Some("feature_x"), CodeConfigFilter::default()),
    )
    .unwrap();
    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].source_key, "feature_x");
}
#[test]
fn helm_inventory_does_not_require_consul_keys() {
    let db = fixture();
    db.execute_batch("INSERT INTO code_repository_files VALUES ('scope','chart/templates/service.yaml','gotemplate');").unwrap();
    add(
        &db,
        "feature_x",
        "config_key",
        "defines_config",
        CodeConfigMetadata::default(),
    );
    let groups = search(
        &db,
        &status(),
        &request(
            None,
            CodeConfigFilter {
                consistency: true,
                ..Default::default()
            },
        ),
    )
    .unwrap();
    assert!(
        !groups[0]
            .consistency_diagnostics
            .iter()
            .any(|d| d.contains("ctmpl"))
    );
}
#[test]
fn dotenv_definition_satisfies_environment_read_consistency() {
    let db = fixture();
    for (path, language_id, content) in [
        (".env", "unknown", "FEATURE=true"),
        (
            "src/app.js",
            "javascript",
            "const enabled = process.env.FEATURE;",
        ),
    ] {
        for row in crate::code::feature_flags::extract_feature_flags(
            crate::code::feature_flags::FeatureFlagFileInput {
                repository_id: "repo",
                source_scope: "scope",
                file_id: "file",
                path,
                language_id,
                content,
                config_facts: &[],
            },
        )
        .unwrap()
        {
            add(
                &db,
                &row.source_key,
                &row.source_kind,
                &row.edge_kind,
                row.metadata,
            );
        }
    }
    let groups = search(
        &db,
        &status(),
        &request(
            Some("FEATURE"),
            CodeConfigFilter {
                consistency: true,
                ..Default::default()
            },
        ),
    )
    .unwrap();
    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].source_kind, "env_var");
    assert!(
        groups[0]
            .usages
            .iter()
            .any(|u| u.edge_kind == "defines_config")
    );
    assert!(
        !groups[0]
            .consistency_diagnostics
            .iter()
            .any(|d| d == "read_without_definition")
    );
}

#[test]
fn late_loaded_getter_checks_its_conversion_shadow() {
    let db = fixture();
    add(
        &db,
        "app.Boolean",
        "config_symbol",
        "config_type_declaration",
        CodeConfigMetadata::default(),
    );
    add(
        &db,
        "flag",
        "config_key",
        "reads_config",
        CodeConfigMetadata {
            bindings: vec!["app.Config.isEnabled".into()],
            declared_getter: Some("app.Config.isEnabled".into()),
            conversion_platform_owners: vec!["app.Boolean".into()],
            ..Default::default()
        },
    );
    add(
        &db,
        "app.Config.isEnabled",
        "config_symbol",
        "guards_code",
        CodeConfigMetadata {
            reference: Some("app.Config.isEnabled".into()),
            ..Default::default()
        },
    );
    db.execute(
        "UPDATE code_repository_feature_flags SET path='Reader.java' WHERE edge_kind='guards_code'",
        [],
    )
    .unwrap();
    let mut query = request(None, CodeConfigFilter::default());
    query.repository.path_filters = vec!["Reader.java".into()];
    let groups = search(&db, &status(), &query).unwrap();
    assert!(
        groups.is_empty(),
        "a shadowed conversion cannot supply getter evidence: {groups:?}"
    );
}
