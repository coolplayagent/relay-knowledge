use crate::code::feature_flags::registry::test_support::*;

#[test]
fn method_and_block_local_types_do_not_collide_with_each_other_or_members() {
    let first = r#"class Config { boolean getX(){return Boolean.getBoolean("first_flag");} }
        Config first = new Config(); if(first.getX()) {}"#;
    let second = r#"class Config { boolean getX(){return Boolean.getBoolean("second_flag");} }
        Config second = new Config(); if(second.getX()) {}"#;
    for bodies in [
        format!("void a() {{{first}}} void b() {{{second}}}"),
        format!("void run() {{{first}}} void run(int ignored) {{{second}}}"),
        format!("void run() {{ {{ {first} }} {{ {second} }} }}"),
    ] {
        let source = format!(
            r#"package app; class App {{
              static class Config {{ boolean getX() {{return Boolean.getBoolean("member_flag");}} }}
              void member() {{Config member = new Config(); if(member.getX()) {{}}}}
              {bodies}
            }}"#
        );
        let rows = raw_facts("java", &source);
        let mut bindings = std::collections::BTreeSet::new();
        for name in ["first", "second", "member"] {
            let provider = rows
                .iter()
                .find(|row| row.source_key == format!("{name}_flag"))
                .unwrap();
            let binding = provider.metadata.declared_getter.as_ref().unwrap();
            assert!(bindings.insert(binding.clone()), "{source}");
            let usages = rows
                .iter()
                .filter(|row| row.excerpt.contains(&format!("{name}.getX()")))
                .filter(|row| matches!(row.edge_kind.as_str(), "reads_config" | "guards_code"))
                .collect::<Vec<_>>();
            assert_eq!(usages.len(), 2, "{name}: {rows:?}");
            assert!(
                usages
                    .iter()
                    .all(|row| row.metadata.reference.as_ref() == Some(binding))
            );
            if name == "member" {
                assert_eq!(binding, "app.App.Config.getX");
            }
        }
        assert_eq!(
            rows.iter()
                .filter(|row| row.edge_kind == "config_type_declaration")
                .map(|row| &row.source_key)
                .collect::<std::collections::BTreeSet<_>>()
                .len(),
            4
        );
    }
}

#[test]
fn local_type_self_super_and_nested_members_share_the_declaration_identity() {
    let rows = raw_facts(
        "java",
        r#"package app; class App { void run() {
          class Base { boolean getX(){ return Boolean.getBoolean("base_flag"); } }
          class Config extends Base {
            boolean getX(){ return Boolean.getBoolean("child_flag"); }
            void check(Config self) {
              if(this.getX()) {} if(self.getX()) {} if(super.getX()) {}
            }
            class Nested { boolean getX(){ return Boolean.getBoolean("nested_flag"); } }
          }
          Config child = new Config(); if(child.getX()) {}
          Config.Nested nested = null; if(nested.getX()) {}
        } }"#,
    );
    let mut bindings = std::collections::BTreeMap::new();
    for key in ["base_flag", "child_flag", "nested_flag"] {
        let provider = rows.iter().find(|row| row.source_key == key).unwrap();
        bindings.insert(key, provider.metadata.declared_getter.as_ref().unwrap());
    }
    let child_owner = bindings["child_flag"].strip_suffix(".getX").unwrap();
    let base_owner = bindings["base_flag"].strip_suffix(".getX").unwrap();
    let hierarchy = rows
        .iter()
        .find(|row| row.edge_kind == "config_type_hierarchy" && row.source_key == child_owner)
        .unwrap();
    assert_eq!(hierarchy.metadata.bindings, [base_owner]);
    assert_eq!(
        bindings["nested_flag"],
        &format!("{child_owner}.Nested.getX")
    );
    for (receiver, key) in [
        ("this", "child_flag"),
        ("self", "child_flag"),
        ("super", "base_flag"),
        ("child", "child_flag"),
        ("nested", "nested_flag"),
    ] {
        let usages = rows
            .iter()
            .filter(|row| row.excerpt.contains(&format!("{receiver}.getX()")))
            .filter(|row| matches!(row.edge_kind.as_str(), "reads_config" | "guards_code"))
            .collect::<Vec<_>>();
        assert_eq!(usages.len(), 2, "{receiver}: {rows:?}");
        assert!(
            usages
                .iter()
                .all(|row| row.metadata.reference.as_ref() == Some(bindings[key]))
        );
    }
}

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

#[test]
fn oversized_parent_metadata_keeps_bounded_incomplete_hierarchy() {
    let source = format!(
        "class Child extends external.{} {{}}",
        "LongType".repeat(9000)
    );
    let rows = raw_facts("java", &source);
    let hierarchy = rows
        .iter()
        .find(|r| r.edge_kind == "config_type_hierarchy")
        .unwrap();
    assert!(hierarchy.metadata.flow_incomplete.is_some());
    assert!(hierarchy.metadata.bindings.is_empty());
    assert!(serde_json::to_string(&hierarchy.metadata).unwrap().len() < 65536);
}

#[test]
fn collection_and_numeric_platform_shadow_signatures_are_persisted() {
    let rows = raw_facts(
        "java",
        "class Base { public Object getenv(){return null;} public Object getProperties(){return null;} public int getInteger(String key,int value){return value;} public String getProperty(String key,int unrelated){return key;} }",
    );
    let row = rows
        .iter()
        .find(|r| r.edge_kind == "config_type_declaration")
        .unwrap();
    for method in ["getenv/0", "getProperties/0", "getInteger/2"] {
        assert!(row.metadata.java_methods.contains_key(method), "{method}");
    }
    assert!(!row.metadata.java_methods.contains_key("getProperty/2"));
}
