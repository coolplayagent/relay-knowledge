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
