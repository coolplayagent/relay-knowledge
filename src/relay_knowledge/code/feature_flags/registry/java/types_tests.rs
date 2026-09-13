use crate::code::feature_flags::registry::test_support::*;

#[test]
fn static_and_private_getters_do_not_provide_ancestor_bindings() {
    for modifier in ["static", "private"] {
        let rows = facts(
            "java",
            &format!(
                "class Base {{}} class Child extends Base {{ {modifier} String getX() {{ return System.getProperty(\"flag\"); }} {modifier} String getUnknown() {{ return arbitrary(); }} }}"
            ),
        );
        for row in rows
            .iter()
            .filter(|r| r.source_key == "flag" || r.edge_kind == "declares_config_getter")
        {
            assert_eq!(row.metadata.getter_overridable, Some(false));
            assert!(
                row.metadata
                    .bindings
                    .iter()
                    .all(|b| b.starts_with("Child.")),
                "{row:?}"
            );
        }
    }
}

#[test]
fn getter_bindings_include_transitive_visible_supertypes() {
    let rows = facts(
        "java",
        r#"package app;
      interface Base { boolean getX(); } interface Child extends Base {}
      class Parent implements Child {}
      class Impl extends Parent { public boolean getX() { return Boolean.getBoolean("flag"); } }
      class Reader { void run(Base c) { if(c.getX()) {} } }
    "#,
    );
    let read = rows
        .iter()
        .find(|r| r.source_key == "flag" && r.edge_kind == "reads_config")
        .unwrap();
    for owner in ["app.Impl", "app.Parent", "app.Child", "app.Base"] {
        assert!(
            read.metadata.bindings.contains(&format!("{owner}.getX")),
            "{owner}"
        );
    }
}

#[test]
fn java_emits_snapshot_hierarchy_facts_for_parent_only_files() {
    let rows = facts("java", "package app; interface Child extends Base {}");
    let row = rows
        .iter()
        .find(|r| r.edge_kind == "config_type_hierarchy")
        .unwrap();
    assert_eq!(row.source_key, "app.Child");
    assert_eq!(row.metadata.bindings, ["app.Base"]);
}

#[test]
fn getter_visibility_retains_package_access_and_implicit_interface_public_access() {
    for (modifier, expected) in [
        ("", "package"),
        ("public", "public"),
        ("protected", "protected"),
        ("private", "private"),
    ] {
        let rows = facts(
            "java",
            &format!(
                "package a; class Config {{ {modifier} String getX() {{ return System.getProperty(\"flag\"); }} }}"
            ),
        );
        let read = rows.iter().find(|r| r.source_key == "flag").unwrap();
        assert_eq!(read.metadata.java_package.as_deref(), Some("a"));
        assert_eq!(read.metadata.getter_visibility.as_deref(), Some(expected));
    }
    let rows = facts(
        "java",
        "interface Config { default String getX() { return System.getProperty(\"flag\"); } }",
    );
    assert_eq!(
        rows.iter()
            .find(|r| r.source_key == "flag")
            .unwrap()
            .metadata
            .getter_visibility
            .as_deref(),
        Some("public")
    );
}

#[test]
fn explicit_and_local_types_survive_unrelated_static_wildcards() {
    for source in [
        "import app.FeatureConfig; import static org.junit.Assert.*; class Reader {void run(){FeatureConfig.isEnabled();}}",
        "import static org.junit.Assert.*; import app.FeatureConfig; class Reader {void run(){FeatureConfig.isEnabled();}}",
        "import static org.junit.Assert.*; class FeatureConfig {} class Reader {void run(){FeatureConfig.isEnabled();}}",
    ] {
        assert!(
            facts("java", source).iter().any(|r| r
                .metadata
                .reference
                .as_deref()
                .is_some_and(|s| s.ends_with("FeatureConfig.isEnabled"))),
            "{source}"
        );
    }
}

#[test]
fn long_field_names_produce_persistable_incomplete_type_evidence() {
    let fields = (0..900)
        .map(|i| format!("int field_{i}_{};", "x".repeat(90)))
        .collect::<String>();
    let rows = raw_facts("java", &format!("class Generated {{ {fields} }}"));
    let declaration = rows
        .iter()
        .find(|r| r.edge_kind == "config_type_declaration")
        .unwrap();
    assert!(declaration.metadata.flow_incomplete.is_some());
    assert!(!declaration.metadata.java_fields.is_empty());
    assert!(declaration.metadata.java_fields.len() < 900);
    assert!(serde_json::to_vec(&declaration.metadata).unwrap().len() <= 65_536);
}

#[test]
fn abstract_getters_emit_barriers_without_configuration_providers() {
    let rows = raw_facts(
        "java",
        "abstract class Mid extends Base { abstract boolean getX(); }",
    );
    let marker = rows
        .iter()
        .find(|r| r.edge_kind == "declares_config_getter")
        .unwrap();
    assert_eq!(marker.metadata.declared_getter.as_deref(), Some("Mid.getX"));
    assert!(marker.metadata.getter_abstract);
    assert!(!rows.iter().any(|r| r.edge_kind == "reads_config"));
}
