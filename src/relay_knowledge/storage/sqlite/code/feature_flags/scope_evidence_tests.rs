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
        (".env", "unknown", "FEATURE=production"),
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

fn java_files(db: &Connection, files: &[(&str, &str)]) {
    for (path, content) in files {
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
                db,
                &row.source_key,
                &row.source_kind,
                &row.edge_kind,
                row.metadata,
            );
            db.execute(
                "UPDATE code_repository_feature_flags SET path=? WHERE rowid=last_insert_rowid()",
                [path],
            )
            .unwrap();
        }
    }
}
#[test]
fn wildcard_supertype_and_constant_bindings_reconcile_snapshot_types() {
    let db = fixture();
    java_files(
        &db,
        &[
            (
                "Base.java",
                r#"package app; class Base { public boolean isX(){return Boolean.getBoolean("feature");} }"#,
            ),
            (
                "Child.java",
                "package app; import java.util.*; class Child extends Base {}",
            ),
            (
                "Keys.java",
                r#"package app; class Keys { public static final String FEATURE_KEY="feature"; }"#,
            ),
            (
                "Reader.java",
                r#"package app; import static app.Keys.*; class Reader { void run(Child config){if(config.isX()){} System.getProperty(FEATURE_KEY);} }"#,
            ),
        ],
    );
    let mut query = request(None, CodeConfigFilter::default());
    query.repository.path_filters = vec!["Reader.java".into()];
    let groups = search(&db, &status(), &query).unwrap();
    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].source_key, "feature");
    assert!(
        groups[0]
            .usages
            .iter()
            .any(|u| u.edge_kind == "guards_code")
    );
    assert!(
        groups[0]
            .usages
            .iter()
            .any(|u| u.metadata.reference.as_deref() == Some("app.Keys.FEATURE_KEY"))
    );
}
#[test]
fn inherited_members_precede_static_platform_imports() {
    for (access, parameter, shadow) in [
        ("public", "String", true),
        ("protected", "String", true),
        ("private", "String", false),
        ("public", "int", false),
    ] {
        let db = fixture();
        java_files(
            &db,
            &[
                (
                    "Base.java",
                    &format!(
                        "package app; class Base {{ {access} String getProperty({parameter} key) {{ return null; }} }}"
                    ),
                ),
                (
                    "Reader.java",
                    r#"package app; import static java.lang.System.getProperty; class Reader extends Base { String read(){return getProperty("feature");} }"#,
                ),
            ],
        );
        let groups = search(
            &db,
            &status(),
            &request(Some("feature"), CodeConfigFilter::default()),
        )
        .unwrap();
        assert_eq!(
            groups.is_empty(),
            shadow,
            "{access} {parameter}: {groups:?}"
        );
    }
}
#[test]
fn template_output_is_definition_evidence_but_java_constant_is_not() {
    for format in ["ctmpl", "java"] {
        let db = fixture();
        add(
            &db,
            "feature",
            "config_key",
            "declares_config_key",
            CodeConfigMetadata {
                source_format: format.into(),
                ..Default::default()
            },
        );
        add(
            &db,
            "feature",
            "config_key",
            "reads_config",
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
        assert_eq!(
            groups[0]
                .consistency_diagnostics
                .iter()
                .any(|d| d == "read_without_definition"),
            format == "java"
        );
    }
}

#[test]
fn unrelated_shell_files_do_not_imply_missing_config_key_definitions() {
    let db = fixture();
    db.execute_batch("INSERT INTO code_repository_files VALUES ('scope','scripts/deploy.sh','bash'),('scope','config.properties','properties');").unwrap();
    add(
        &db,
        "feature",
        "config_key",
        "defines_config",
        CodeConfigMetadata {
            source_format: "properties".into(),
            ..Default::default()
        },
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
    assert!(groups[0].consistency_diagnostics.is_empty());
}
#[test]
fn query_terms_match_across_connected_usages_after_resolution() {
    let db = fixture();
    add(
        &db,
        "feature_x",
        "config_key",
        "defines_config",
        CodeConfigMetadata {
            source_format: "properties".into(),
            ..Default::default()
        },
    );
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
    db.execute("UPDATE code_repository_feature_flags SET path='src/Reader.java' WHERE edge_kind='guards_code'",[]).unwrap();
    for (query, count) in [("feature_x Reader", 1), ("feature_x absent", 0)] {
        let groups = search(
            &db,
            &status(),
            &request(Some(query), CodeConfigFilter::default()),
        )
        .unwrap();
        assert_eq!(groups.len(), count);
        if count == 1 {
            assert!(groups[0].usages.iter().any(|u| u.path == "src/Reader.java"));
        }
    }
    db.execute("UPDATE code_repository_feature_flags SET source_scope='other' WHERE edge_kind='guards_code'",[]).unwrap();
    assert!(
        search(
            &db,
            &status(),
            &request(Some("feature_x Reader"), CodeConfigFilter::default())
        )
        .unwrap()
        .is_empty()
    );
}

#[test]
fn converted_explicit_defaults_compare_effective_values_and_restore_shadowed_raw_values() {
    let db = fixture();
    java_files(
        &db,
        &[(
            "Config.java",
            r#"package app; class Config { boolean getX(){return Boolean.parseBoolean(System.getProperty("flag", "TRUE"));} }"#,
        )],
    );
    add(
        &db,
        "flag",
        "config_key",
        "defines_config",
        CodeConfigMetadata {
            source_format: "properties".into(),
            default_value: Some("true".into()),
            ..Default::default()
        },
    );
    let query = request(
        Some("flag"),
        CodeConfigFilter {
            consistency: true,
            ..Default::default()
        },
    );
    let groups = search(&db, &status(), &query).unwrap();
    assert!(
        !groups[0]
            .consistency_diagnostics
            .iter()
            .any(|d| d.starts_with("conflicting_defaults"))
    );
    java_files(&db, &[("Boolean.java", "package app; class Boolean {}")]);
    let groups = search(&db, &status(), &query).unwrap();
    assert!(
        groups[0]
            .usages
            .iter()
            .any(|u| u.edge_kind == "reads_config"
                && u.metadata.default_value.as_deref() == Some("TRUE"))
    );
}
