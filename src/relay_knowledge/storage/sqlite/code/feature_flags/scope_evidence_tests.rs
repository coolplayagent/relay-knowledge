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

#[test]
fn inherited_fields_take_precedence_over_static_key_imports() {
    for (visibility, package, expected) in [
        ("public", "app", "base_key"),
        ("protected", "other", "base_key"),
        ("private", "app", "imported_key"),
        ("", "other", "imported_key"),
    ] {
        let db = fixture();
        java_files(
            &db,
            &[
                (
                    "Keys.java",
                    r#"package app; class Keys { public static final String FEATURE_KEY="imported_key"; }"#,
                ),
                (
                    "Base.java",
                    &format!(
                        r#"package app; class Base {{ {visibility} static final String FEATURE_KEY="base_key"; }}"#
                    ),
                ),
                (
                    "Child.java",
                    &format!(
                        r#"package {package}; import static app.Keys.FEATURE_KEY; class Child extends app.Base {{ String read(){{ return System.getProperty(FEATURE_KEY); }} }}"#
                    ),
                ),
            ],
        );
        let mut query = request(None, CodeConfigFilter::default());
        query.repository.path_filters = vec!["Child.java".into()];
        let groups = search(&db, &status(), &query).unwrap();
        assert_eq!(groups.len(), 1, "{groups:?}");
        assert_eq!(groups[0].source_key, expected, "{visibility} {package}");
        let groups = search(
            &db,
            &status(),
            &request(Some(expected), CodeConfigFilter::default()),
        )
        .unwrap();
        assert!(
            groups.iter().any(|g| g.source_key == expected
                && g.usages
                    .iter()
                    .any(|u| u.path == "Child.java" && u.edge_kind == "reads_config")),
            "{groups:?}"
        );
    }
}

#[test]
fn inherited_inapplicable_reference_overloads_preserve_platform_imports() {
    let db = fixture();
    java_files(
        &db,
        &[
            (
                "Base.java",
                "package app; class Base { public String getenv(Integer key){return null;} }",
            ),
            (
                "Child.java",
                r#"package app; import static java.lang.System.getenv; class Child extends Base { String read(){return getenv("REAL_ENV");} }"#,
            ),
        ],
    );
    let groups = search(
        &db,
        &status(),
        &request(Some("REAL_ENV"), CodeConfigFilter::default()),
    )
    .unwrap();
    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].source_kind, "env_var");
}

#[test]
fn numeric_getter_fallbacks_compare_effective_values() {
    let db = fixture();
    java_files(
        &db,
        &[(
            "Config.java",
            r#"class Config { int getPort(){return Integer.parseInt(System.getProperty("port", "08"));} }"#,
        )],
    );
    add(
        &db,
        "port",
        "config_key",
        "defines_config",
        CodeConfigMetadata {
            source_format: "properties".into(),
            default_value: Some("8".into()),
            ..Default::default()
        },
    );
    let groups = search(
        &db,
        &status(),
        &request(
            Some("port"),
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
            .any(|d| d.starts_with("conflicting_defaults"))
    );
    assert!(
        groups[0]
            .usages
            .iter()
            .any(|u| u.metadata.numeric_converted_default)
    );
}

#[test]
fn consistency_uses_last_definition_per_file_and_keeps_source_provenance() {
    for format in ["properties", "ini", "dotenv", "shell"] {
        let db = fixture();
        for (offset, value) in [(0, Some("false")), (10, Some("true"))] {
            add(
                &db,
                "feature",
                "config_key",
                "defines_config",
                CodeConfigMetadata {
                    source_format: format.into(),
                    default_value: value.map(str::to_owned),
                    ..Default::default()
                },
            );
            db.execute("UPDATE code_repository_feature_flags SET byte_start=?1,byte_end=?1+1 WHERE rowid=?2",params![offset,db.last_insert_rowid()]).unwrap();
        }
        let query = request(
            None,
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
                .any(|d| d.starts_with("conflicting_defaults")),
            "{format}"
        );
        assert_eq!(groups[0].usages.len(), 2);
        add(
            &db,
            "feature",
            "config_key",
            "defines_config",
            CodeConfigMetadata {
                source_format: format.into(),
                default_value: Some("false".into()),
                ..Default::default()
            },
        );
        db.execute(
            "UPDATE code_repository_feature_flags SET path='other/config' WHERE rowid=?1",
            [db.last_insert_rowid()],
        )
        .unwrap();
        let groups = search(&db, &status(), &query).unwrap();
        assert_eq!(groups[0].conflicting_default_sources.len(), 2);
        assert!(
            groups[0]
                .conflicting_default_sources
                .iter()
                .all(|u| u.path != "src/config" || u.byte_range.start == 10)
        );
        db.execute("UPDATE code_repository_feature_flags SET metadata_json=json_remove(metadata_json,'$.default_value') WHERE byte_start=10",[]).unwrap();
        let groups = search(&db, &status(), &query).unwrap();
        assert!(groups[0].conflicting_default_sources.is_empty());
    }
}

#[test]
fn path_filters_preserve_case_for_requests_and_registration() {
    let db = fixture();
    for path in ["src/config", "SRC/config"] {
        add(
            &db,
            "feature",
            "config_key",
            "reads_config",
            CodeConfigMetadata::default(),
        );
        db.execute(
            "UPDATE code_repository_feature_flags SET path=?1 WHERE rowid=?2",
            params![path, db.last_insert_rowid()],
        )
        .unwrap();
    }
    for registration in [false, true] {
        let mut status = status();
        let mut query = request(None, CodeConfigFilter::default());
        if registration {
            status.path_filters = vec!["src".into()];
        } else {
            query.repository.path_filters = vec!["src".into()];
        }
        let groups = search(&db, &status, &query).unwrap();
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].usages.len(), 1);
        assert_eq!(groups[0].usages[0].path, "src/config");
    }
}

#[test]
fn static_wildcards_do_not_disconnect_explicit_type_getter_providers() {
    let db = fixture();
    java_files(
        &db,
        &[
            (
                "FeatureConfig.java",
                r#"package app; class FeatureConfig {public static boolean isEnabled(){return Boolean.getBoolean("feature");}}"#,
            ),
            (
                "Reader.java",
                "import app.FeatureConfig; import static org.junit.Assert.*; class Reader {void run(){if(FeatureConfig.isEnabled()) {}}}",
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
}

#[test]
fn abstract_redeclarations_stop_inherited_configuration_getters() {
    let db = fixture();
    java_files(
        &db,
        &[
            (
                "Base.java",
                r#"class Base { boolean getX(){return Boolean.getBoolean("base_flag");} }"#,
            ),
            (
                "Mid.java",
                "abstract class Mid extends Base {abstract boolean getX();}",
            ),
            (
                "Child.java",
                "class Child extends Mid {boolean getX(){return false;}}",
            ),
            (
                "Reader.java",
                "class Reader {void run(Mid a, Child b){if(a.getX()){} if(b.getX()) {}}}",
            ),
        ],
    );
    let mut query = request(None, CodeConfigFilter::default());
    query.repository.path_filters = vec!["Reader.java".into()];
    let groups = search(&db, &status(), &query).unwrap();
    assert!(
        groups.iter().all(|g| g.source_key != "base_flag"),
        "{groups:?}"
    );
}

#[test]
fn inherited_unqualified_getters_connect_child_guards() {
    let db = fixture();
    java_files(
        &db,
        &[
            (
                "Base.java",
                r#"class Base {boolean isEnabled(){return Boolean.getBoolean("feature");}}"#,
            ),
            (
                "Child.java",
                "class Child extends Base {void run(){if(isEnabled()) {}}}",
            ),
        ],
    );
    let mut query = request(None, CodeConfigFilter::default());
    query.repository.path_filters = vec!["Child.java".into()];
    let groups = search(&db, &status(), &query).unwrap();
    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].source_key, "feature");
    assert!(
        groups[0]
            .usages
            .iter()
            .any(|u| u.edge_kind == "guards_code")
    );
}

#[test]
fn super_qualified_key_fields_use_the_superclass_provider() {
    let db = fixture();
    java_files(
        &db,
        &[
            (
                "Base.java",
                r#"package app; class Base {protected static final String KEY="base_key";}"#,
            ),
            (
                "Child.java",
                r#"package app; class Child extends Base {static final String KEY="child_key"; String read(){return System.getProperty(super.KEY);}}"#,
            ),
        ],
    );
    let mut query = request(None, CodeConfigFilter::default());
    query.repository.path_filters = vec!["Child.java".into()];
    let groups = search(&db, &status(), &query).unwrap();
    assert!(
        groups.iter().any(|g| g.source_key == "base_key"
            && g.usages.iter().any(|u| u.edge_kind == "reads_config")),
        "{groups:?}"
    );
    assert!(
        groups.iter().all(|g| g.source_key != "child_key"
            || g.usages.iter().all(|u| u.edge_kind != "reads_config"))
    );
}

#[test]
fn this_qualified_inherited_keys_resolve_without_local_field_declarations() {
    let db = fixture();
    java_files(
        &db,
        &[
            (
                "Base.java",
                r#"package app; class Base {protected static final String KEY="base_key";}"#,
            ),
            (
                "Child.java",
                r#"package app; class Child extends Base {String read(){String KEY="local_key"; return System.getProperty(this.KEY);}}"#,
            ),
        ],
    );
    let mut query = request(None, CodeConfigFilter::default());
    query.repository.path_filters = vec!["Child.java".into()];
    let groups = search(&db, &status(), &query).unwrap();
    assert!(
        groups.iter().any(|g| g.source_key == "base_key"
            && g.usages.iter().any(|u| u.edge_kind == "reads_config")),
        "{groups:?}"
    );
}
#[test]
fn oversized_parent_metadata_fails_only_dependent_queries() {
    let db = fixture();
    let source = format!(
        "class Child extends external.{} {{ String read() {{return System.getProperty(this.KEY);}} }}",
        "LongType".repeat(9000)
    );
    java_files(&db, &[("Child.java", &source)]);
    let error = search(&db, &status(), &request(None, CodeConfigFilter::default())).unwrap_err();
    assert!(
        error.to_string().contains("type parent metadata budget"),
        "{error:?}"
    );
}

#[test]
fn qualified_enclosing_this_getters_bind_outer_reads_and_guards() {
    let db = fixture();
    java_files(
        &db,
        &[(
            "Outer.java",
            r#"package app; class Outer {boolean isEnabled(){return Boolean.getBoolean("outer_flag");} class Inner {void run(){if(Outer.this.isEnabled()) {}}}}"#,
        )],
    );
    let groups = search(
        &db,
        &status(),
        &request(Some("outer_flag"), CodeConfigFilter::default()),
    )
    .unwrap();
    assert!(
        groups.iter().any(|g| g.source_key == "outer_flag"
            && g.usages.iter().any(|u| u.edge_kind == "guards_code")),
        "{groups:?}"
    );
}

#[test]
fn qualified_enclosing_key_fields_resolve_explicit_outer_owner() {
    let db = fixture();
    java_files(
        &db,
        &[(
            "Outer.java",
            r#"package app; class Outer {final String KEY="outer_key"; class Inner {final String KEY="inner_key"; String read(){return System.getProperty(Outer.this.KEY);}}}"#,
        )],
    );
    let groups = search(
        &db,
        &status(),
        &request(Some("outer_key"), CodeConfigFilter::default()),
    )
    .unwrap();
    assert!(
        groups.iter().any(|g| g.source_key == "outer_key"
            && g.usages.iter().any(|u| u.edge_kind == "reads_config")),
        "{groups:?}"
    );
}

#[test]
fn inherited_zero_arity_collection_shadow_and_getter_overloads_resolve() {
    let db = fixture();
    java_files(
        &db,
        &[
            (
                "Base.java",
                r#"class Base {public Object getenv(){return null;} boolean isEnabled(){return Boolean.getBoolean("inherited");}}"#,
            ),
            (
                "Child.java",
                r#"import static java.lang.System.getenv; class Child extends Base {boolean isEnabled(int n){return false;} void run(){getenv().get("hidden"); if(isEnabled()) {}}}"#,
            ),
        ],
    );
    let mut query = request(None, CodeConfigFilter::default());
    query.repository.path_filters = vec!["Child.java".into()];
    let groups = search(&db, &status(), &query).unwrap();
    assert!(!groups.iter().any(|g| g.source_key == "hidden"));
    assert!(
        groups.iter().any(|g| g.source_key == "inherited"
            && g.usages.iter().any(|u| u.edge_kind == "guards_code")),
        "{groups:?}"
    );
}
