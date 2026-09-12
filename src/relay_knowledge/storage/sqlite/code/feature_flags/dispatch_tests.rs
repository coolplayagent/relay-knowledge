use super::*;
#[test]
fn resolved_key_and_metadata_filters_do_not_discard_path_selected_callers() {
    let db = fixture();
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
        "feature_x",
        "config_key",
        "defines_config",
        CodeConfigMetadata {
            source_format: "properties".into(),
            domain: Some("payments".into()),
            hot_reload: Some(true),
            ..Default::default()
        },
    );
    add(
        &db,
        "call",
        "config_symbol",
        "guards_code",
        CodeConfigMetadata {
            reference: Some("Config.getX".into()),
            ..Default::default()
        },
    );
    db.execute("UPDATE code_repository_feature_flags SET path='src/Reader.java' WHERE source_kind='config_symbol'",[]).unwrap();
    for term in [None, Some("feature_x")] {
        for filters in [
            CodeConfigFilter::default(),
            CodeConfigFilter {
                domain: Some("payments".into()),
                source: Some("properties".into()),
                hot_reload: Some(true),
                ..Default::default()
            },
        ] {
            let mut query = request(term, filters);
            query.repository.path_filters = vec!["src/Reader.java".into()];
            let groups = search(&db, &status(), &query).unwrap();
            assert_eq!(groups.len(), 1);
            assert_eq!(groups[0].source_key, "feature_x");
            assert!(groups[0].usages.iter().all(|u| u.path == "src/Reader.java"));
            query.query = Some("different_key".into());
            assert!(search(&db, &status(), &query).unwrap().is_empty());
        }
    }
}
#[test]
fn package_private_getters_do_not_link_cross_package_overrides() {
    for visibility in ["package", "protected", "public"] {
        let db = fixture();
        for (owner, package) in [("a.Base", "a"), ("b.Child", "b")] {
            add(
                &db,
                owner,
                "config_symbol",
                "config_type_declaration",
                CodeConfigMetadata {
                    java_package: Some(package.into()),
                    ..Default::default()
                },
            );
        }
        add(
            &db,
            "b.Child",
            "config_symbol",
            "config_type_hierarchy",
            CodeConfigMetadata {
                bindings: vec!["a.Base".into()],
                ..Default::default()
            },
        );
        for (key, owner, package, access) in [
            ("base", "a.Base", "a", visibility),
            ("child", "b.Child", "b", "public"),
        ] {
            add(
                &db,
                key,
                "config_key",
                "reads_config",
                CodeConfigMetadata {
                    bindings: vec![format!("{owner}.getX")],
                    declared_getter: Some(format!("{owner}.getX")),
                    java_package: Some(package.into()),
                    getter_visibility: Some(access.into()),
                    ..Default::default()
                },
            );
        }
        add(
            &db,
            "call",
            "config_symbol",
            "guards_code",
            CodeConfigMetadata {
                reference: Some("a.Base.getX".into()),
                ..Default::default()
            },
        );
        let mut query = request(None, CodeConfigFilter::default());
        query.limit = 10;
        let groups = search(&db, &status(), &query).unwrap();
        let base = groups.iter().find(|g| g.source_key == "base").unwrap();
        assert_eq!(
            base.usages.iter().any(|u| u.edge_kind == "guards_code"),
            visibility == "package"
        );
    }
}
#[test]
fn inherited_getter_bindings_stop_when_a_child_declares_an_override() {
    let db = fixture();
    add(
        &db,
        "Child",
        "config_symbol",
        "config_type_hierarchy",
        CodeConfigMetadata {
            bindings: vec!["Base".into()],
            ..Default::default()
        },
    );
    add(
        &db,
        "base",
        "config_key",
        "reads_config",
        CodeConfigMetadata {
            bindings: vec!["Base.getX".into()],
            declared_getter: Some("Base.getX".into()),
            getter_inheritable: Some(true),
            ..Default::default()
        },
    );
    add(
        &db,
        "call",
        "config_symbol",
        "guards_code",
        CodeConfigMetadata {
            reference: Some("Child.getX".into()),
            exact_reference: true,
            ..Default::default()
        },
    );
    let mut query = request(None, CodeConfigFilter::default());
    query.limit = 10;
    let groups = search(&db, &status(), &query).unwrap();
    assert!(
        groups
            .iter()
            .find(|g| g.source_key == "base")
            .unwrap()
            .usages
            .iter()
            .any(|u| u.edge_kind == "guards_code")
    );
    add(
        &db,
        "child",
        "config_key",
        "reads_config",
        CodeConfigMetadata {
            bindings: vec!["Child.getX".into()],
            declared_getter: Some("Child.getX".into()),
            ..Default::default()
        },
    );
    let groups = search(&db, &status(), &query).unwrap();
    assert!(
        !groups
            .iter()
            .find(|g| g.source_key == "base")
            .unwrap()
            .usages
            .iter()
            .any(|u| u.edge_kind == "guards_code")
    );
    assert!(
        groups
            .iter()
            .find(|g| g.source_key == "child")
            .unwrap()
            .usages
            .iter()
            .any(|u| u.edge_kind == "guards_code")
    );
}
#[test]
fn path_projection_keeps_authorized_providers_outside_the_requested_file() {
    let db = fixture();
    add(
        &db,
        "flag",
        "config_key",
        "reads_config",
        CodeConfigMetadata {
            bindings: vec!["Config.getX".into()],
            ..Default::default()
        },
    );
    db.execute(
        "UPDATE code_repository_feature_flags SET path='src/Config.java'",
        [],
    )
    .unwrap();
    add(
        &db,
        "call",
        "config_symbol",
        "guards_code",
        CodeConfigMetadata {
            reference: Some("Config.getX".into()),
            ..Default::default()
        },
    );
    db.execute("UPDATE code_repository_feature_flags SET path='src/Reader.java' WHERE source_kind='config_symbol'",[]).unwrap();
    let mut query = request(None, CodeConfigFilter::default());
    query.repository.path_filters = vec!["./src/Reader.java".into()];
    let mut registered = status();
    registered.path_filters = vec!["src".into()];
    let groups = search(&db, &registered, &query).unwrap();
    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].source_key, "flag");
    assert!(groups[0].usages.iter().all(|u| u.path == "src/Reader.java"));
    db.execute("UPDATE code_repository_feature_flags SET path='private/Config.java' WHERE source_kind='config_key'",[]).unwrap();
    assert!(search(&db, &registered, &query).unwrap().is_empty());
}
#[test]
fn snapshot_package_types_precede_wildcard_import_ambiguity() {
    let db = fixture();
    add(
        &db,
        "app.Config",
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
            bindings: vec!["app.Config.getX".into()],
            ..Default::default()
        },
    );
    add(
        &db,
        "call",
        "config_symbol",
        "guards_code",
        CodeConfigMetadata {
            reference: Some("<ambiguous-import>.Config.getX".into()),
            same_package_reference: Some("app.Config.getX".into()),
            ..Default::default()
        },
    );
    let groups = search(&db, &status(), &request(None, CodeConfigFilter::default())).unwrap();
    assert!(
        groups[0]
            .usages
            .iter()
            .any(|u| u.edge_kind == "guards_code")
    );
    db.execute("UPDATE code_repository_feature_flags SET source_scope='old' WHERE edge_kind='config_type_declaration'",[]).unwrap();
    let groups = search(&db, &status(), &request(None, CodeConfigFilter::default())).unwrap();
    assert!(
        !groups[0]
            .usages
            .iter()
            .any(|u| u.edge_kind == "guards_code")
    );
}
#[test]
fn exact_and_virtual_references_have_separate_resolution_cache_entries() {
    let db = fixture();
    add(
        &db,
        "Child",
        "config_symbol",
        "config_type_hierarchy",
        CodeConfigMetadata {
            bindings: vec!["Base".into()],
            ..Default::default()
        },
    );
    for (key, owner) in [("base", "Base"), ("child", "Child")] {
        add(
            &db,
            key,
            "config_key",
            "reads_config",
            CodeConfigMetadata {
                bindings: vec![format!("{owner}.getX")],
                declared_getter: Some(format!("{owner}.getX")),
                ..Default::default()
            },
        );
    }
    for (key, exact) in [("exact", true), ("virtual", false)] {
        add(
            &db,
            key,
            "config_symbol",
            "reads_config",
            CodeConfigMetadata {
                reference: Some("Base.getX".into()),
                exact_reference: exact,
                ..Default::default()
            },
        );
    }
    let mut query = request(None, CodeConfigFilter::default());
    query.limit = 10;
    let groups = search(&db, &status(), &query).unwrap();
    let base = groups.iter().find(|g| g.source_key == "base").unwrap();
    assert!(base.usages.iter().any(|u| u.metadata.exact_reference));
    let child = groups.iter().find(|g| g.source_key == "child").unwrap();
    assert!(!child.usages.iter().any(|u| u.metadata.exact_reference));
}
