use super::*;
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
