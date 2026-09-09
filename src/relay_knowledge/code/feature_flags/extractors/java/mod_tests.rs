use super::*;

#[path = "binding_boundaries_tests.rs"]
mod binding_boundaries;

#[test]
fn parameterized_getters_do_not_bind_zero_argument_calls() {
    let records = facts(
        r#"class Config {
        String getValue(int other) { return System.getProperty("parameter"); }
        String getValue() { return "ordinary"; }
        String getDirect() { return System.getProperty("direct"); }
    }"#,
    );
    assert!(
        records
            .iter()
            .find(|r| r.source_key == "parameter")
            .unwrap()
            .metadata
            .bindings
            .is_empty()
    );
    assert!(
        !records
            .iter()
            .find(|r| r.source_key == "direct")
            .unwrap()
            .metadata
            .bindings
            .is_empty()
    );
}

#[test]
fn interface_fields_shadow_platform_receivers_but_qualified_reads_remain() {
    let records = facts(
        r#"interface Config {
        Fake System = new Fake(); Fake Boolean = new Fake();
        default void run() {
            System.getProperty("false_system"); Boolean.getBoolean("false_boolean");
            java.lang.System.getProperty("real");
        }
    }"#,
    );
    assert!(
        !records
            .iter()
            .any(|r| matches!(r.source_key.as_str(), "false_system" | "false_boolean"))
    );
    assert!(
        records
            .iter()
            .any(|r| r.source_key == "real" && r.edge_kind == "reads_config")
    );
}

#[test]
fn direct_for_conditions_guard_but_initializer_and_update_reads_do_not() {
    let records = facts(
        r#"class App { void run() {
        for (String s = System.getProperty("init"); Boolean.getBoolean("condition"); System.getProperty("update")) {}
    }}"#,
    );
    assert_eq!(
        records
            .iter()
            .filter(|r| r.source_key == "condition" && r.edge_kind == "guards_code")
            .count(),
        1
    );
    assert!(!records.iter().any(
        |r| matches!(r.source_key.as_str(), "init" | "update") && r.edge_kind == "guards_code"
    ));
}

fn facts(content: &str) -> Vec<CodeFeatureFlagRecord> {
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_java::LANGUAGE.into())
        .unwrap();
    let tree = parser.parse(content, None).unwrap();
    extract(
        &FeatureFlagFileInput {
            repository_id: "repo",
            source_scope: "scope",
            file_id: "file",
            path: "App.java",
            language_id: "java",
            content,
            config_facts: &[],
        },
        tree.root_node(),
    )
    .unwrap()
}

#[test]
fn indexes_multiline_java_reads_and_separate_guard_locations() {
    let records = facts(
        r#"class App {
        void run() {
            boolean enabled = Boolean.getBoolean(
                "feature_x");
            if (enabled) { work(); }
            if (Boolean.getBoolean("feature_y")) { work(); }
            String path = System.getProperty("app.path", "/tmp");
            String env = System.getenv("APP_ENABLED");
        }
    }"#,
    );
    assert!(records.iter().any(|r| r.source_key == "feature_x"
        && r.edge_kind == "reads_config"
        && r.line_range.start == 3));
    assert!(records.iter().any(|r| r.source_key == "feature_x"
        && r.edge_kind == "guards_code"
        && r.line_range.start == 5));
    assert_eq!(
        records
            .iter()
            .filter(|r| r.source_key == "feature_y")
            .count(),
        2
    );
    assert!(records.iter().any(|r| r.source_key == "app.path"));
    assert!(
        records
            .iter()
            .any(|r| r.source_key == "APP_ENABLED" && r.source_kind == "env_var")
    );
}

#[test]
fn ignores_comments_strings_and_dynamic_keys_and_stops_flow_after_writes() {
    let records = facts(
        r#"class App {
        void run(String key) {
            // System.getProperty("comment");
            String example = "System.getProperty(\"string\")";
            String dynamic = System.getProperty(key);
            boolean enabled = Boolean.getBoolean("feature_x");
            enabled = false;
            if (enabled) { work(); }
        }
        void other() { if (enabled) { work(); } }
    }"#,
    );
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].source_key, "feature_x");
    assert_eq!(records[0].edge_kind, "reads_config");
}

#[test]
fn emits_exact_constant_and_getter_bindings_for_snapshot_resolution() {
    let records = facts(
        r#"package demo;
        class Keys { public static final String FEATURE_Y = "feature_y"; }
        interface FooConfig { boolean getX(); }
        class DefaultFooConfig implements FooConfig {
            public boolean getX() { return Boolean.getBoolean("feature_x"); }
        }
        class App {
            void run(FooConfig config) {
                String value = System.getProperty(Keys.FEATURE_Y, "false");
                if (config.getX()) { work(); }
            }
        }"#,
    );
    let constant = records
        .iter()
        .find(|r| r.edge_kind == "binds_config_symbol")
        .unwrap();
    assert_eq!(constant.source_key, "feature_y");
    assert_eq!(constant.metadata.bindings, vec!["demo.Keys.FEATURE_Y"]);
    let getter = records
        .iter()
        .find(|r| r.source_key == "feature_x")
        .unwrap();
    assert!(
        getter
            .metadata
            .bindings
            .contains(&"demo.FooConfig.getX".to_owned())
    );
    assert!(
        records
            .iter()
            .any(|r| r.metadata.referenced_symbol.as_deref() == Some("demo.Keys.FEATURE_Y"))
    );
    assert!(
        records
            .iter()
            .any(
                |r| r.metadata.referenced_symbol.as_deref() == Some("demo.FooConfig.getX")
                    && r.edge_kind == "guards_code"
            )
    );
}

#[test]
fn does_not_interpret_explicitly_shadowed_platform_classes_as_configuration_apis() {
    for source in [
        "class System {} class App { void run() { System.getProperty(\"wrong\"); } }",
        "import custom.System; class App { void run() { System.getProperty(\"wrong\"); } }",
        "class App { void run(Custom System) { System.getProperty(\"wrong\"); } }",
    ] {
        assert!(facts(source).is_empty());
    }
    assert_eq!(
        facts("class App { void run(Custom System) { java.lang.System.getProperty(\"right\"); } }")
            .len(),
        1
    );
}

#[test]
fn getter_calls_remain_candidates_until_snapshot_config_binding_proves_them() {
    let records =
        facts("class App { void run(Person person) { if (person.isActive()) { work(); } } }");
    assert_eq!(records.len(), 2);
    assert!(
        records
            .iter()
            .all(|record| record.source_kind == "config_getter")
    );
    assert!(
        records
            .iter()
            .all(|record| record.metadata.referenced_symbol.as_deref() == Some("Person.isActive"))
    );
}

#[test]
fn constant_candidates_require_real_static_final_modifiers() {
    let records = facts(
        r#"class Keys {
        @Note(value="label") static final String REAL = "real_key";
        /* static final */ String MUTABLE = "not_constant";
    }"#,
    );
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].source_key, "real_key");
}

#[test]
fn indirect_keys_preserve_the_call_api_namespace_on_reads_and_guards() {
    let records = facts(
        r#"class App { void run() {
        String value = System.getenv(Keys.SWITCH);
        if (value != null) { work(); }
        String property = System.getProperty(Keys.SWITCH);
    } }"#,
    );
    let env = records
        .iter()
        .filter(|r| r.metadata.read_source_kind == Some(CodeConfigurationReadKind::EnvVar))
        .collect::<Vec<_>>();
    assert_eq!(env.len(), 2);
    assert!(env.iter().any(|r| r.edge_kind == "guards_code"));
    assert_eq!(
        records
            .iter()
            .filter(|r| r.metadata.read_source_kind == Some(CodeConfigurationReadKind::ConfigKey))
            .count(),
        1
    );
}

#[test]
fn custom_getter_names_do_not_imply_platform_api_namespace() {
    let records = facts("class App { void run(Service config) { config.getProperty(); } }");
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].source_kind, "config_getter");
    assert!(records[0].metadata.read_source_kind.is_none());
    assert!(records[0].metadata.value_type.is_none());
}

#[test]
fn same_line_reads_and_guard_dependencies_keep_distinct_occurrence_identities() {
    let records = facts(
        "class App { void run() { System.getenv(Keys.X); System.getProperty(Keys.X); if (Boolean.getBoolean(\"x\") || Boolean.getBoolean(\"x\")) { work(); } } }",
    );
    let ids = records
        .iter()
        .map(|record| &record.usage_id)
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(ids.len(), records.len());
    assert_eq!(
        records
            .iter()
            .filter(|record| record.edge_kind == "guards_code")
            .count(),
        2
    );
    assert_eq!(
        records
            .iter()
            .filter(|record| record.source_key == "Keys.X")
            .count(),
        2
    );
}

#[test]
fn copied_local_values_keep_guards_but_member_names_and_real_writes_do_not() {
    let records = facts(
        r#"class App { void run(Settings settings) {
        boolean enabled = Boolean.getBoolean("copy");
        boolean copy = enabled; copy = enabled; this.enabled = false;
        if (settings.enabled) {} if (settings.enabled()) {} if (enabled) {}
        enabled = false; if (enabled) {}
    } }"#,
    );
    assert_eq!(
        records
            .iter()
            .filter(|r| r.edge_kind == "guards_code")
            .count(),
        1
    );
}

#[test]
fn nested_type_owners_and_generic_getters_preserve_structured_identity() {
    let records = facts(
        r#"package demo;
        class OuterA { static class Keys { static final String X = "a"; } }
        class OuterB { static class Keys { static final String X = "b"; } }
        interface Config<T> { String getValue(); }
        class DefaultConfig implements Config<java.util.Map<String, Integer>> {
          public String getValue() { return System.getProperty("generic"); }
        }
        class App { void run(Config<java.util.Map<String, Integer>> config) {
          System.getProperty(OuterA.Keys.X); System.getProperty(OuterB.Keys.X); config.getValue();
        } }"#,
    );
    for (key, binding) in [("a", "demo.OuterA.Keys.X"), ("b", "demo.OuterB.Keys.X")] {
        assert!(
            records
                .iter()
                .any(|r| r.source_key == key && r.metadata.bindings.contains(&binding.to_owned())),
            "{records:#?}"
        );
    }
    assert!(records.iter().any(|r| {
        r.source_key == "generic"
            && r.metadata
                .bindings
                .contains(&"demo.Config.getValue".to_owned())
    }));
    assert!(
        records
            .iter()
            .any(|r| r.source_key == "demo.Config.getValue")
    );
}

#[test]
fn visible_java_type_kinds_shadow_platform_but_unrelated_nested_types_do_not() {
    let enum_records = facts(
        "enum Holder {; static class System {} void run() { System.getProperty(\"enum_nested_false\"); } } class Outside { void run() { System.getProperty(\"outside_true\"); } }",
    );
    assert!(
        !enum_records
            .iter()
            .any(|r| r.source_key == "enum_nested_false")
    );
    assert!(enum_records.iter().any(|r| r.source_key == "outside_true"));
    for declaration in [
        "class System {}",
        "interface System {}",
        "enum System { A }",
        "record System(String name) {}",
    ] {
        let records = facts(&format!(
            "class Outer {{ {declaration} void run() {{ System.getProperty(\"false_key\"); java.lang.System.getProperty(\"true_key\"); }} }} class Other {{ void run() {{ System.getProperty(\"unrelated\"); }} }}"
        ));
        assert!(
            !records.iter().any(|r| r.source_key == "false_key"),
            "{declaration}"
        );
        assert!(records.iter().any(|r| r.source_key == "true_key"));
        assert!(records.iter().any(|r| r.source_key == "unrelated"));
    }
}

#[test]
fn getter_bindings_do_not_cross_expression_or_block_lambda_returns() {
    let records = facts(
        r#"class Config {
      java.util.function.Supplier<String> getLambda() { return () -> System.getProperty("lambda"); }
      java.util.function.Supplier<String> getBlock() { return () -> { return System.getProperty("block"); }; }
      String getDirect() { return System.getProperty("direct"); }
    }"#,
    );
    for key in ["lambda", "block"] {
        assert!(
            records
                .iter()
                .find(|r| r.source_key == key)
                .unwrap()
                .metadata
                .bindings
                .is_empty()
        );
    }
    assert!(
        records
            .iter()
            .find(|r| r.source_key == "direct")
            .unwrap()
            .metadata
            .bindings
            .contains(&"Config.getDirect".to_owned())
    );
}

#[test]
fn interface_constants_use_implicit_modifiers_and_complete_type_owners() {
    let records = facts(
        r#"package demo;
      interface Keys { String FLAG = "top"; }
      class Outer { interface Keys { String FLAG = "nested"; } }
      class Ordinary { String mutable = "not_constant"; }
      class App { void read() { System.getProperty(Keys.FLAG); System.getProperty(Outer.Keys.FLAG); } }
    "#,
    );
    for (key, owner) in [
        ("top", "demo.Keys.FLAG"),
        ("nested", "demo.Outer.Keys.FLAG"),
    ] {
        assert!(
            records
                .iter()
                .any(|r| r.source_key == key && r.metadata.bindings.contains(&owner.to_owned())),
            "{records:#?}"
        );
    }
    assert!(!records.iter().any(|r| r.source_key == "not_constant"));
}

#[test]
fn local_flags_reach_nested_ternary_conditions_without_crossing_callable_or_write_boundaries() {
    let records = facts(
        r#"class App {
      String returned() { boolean enabled = Boolean.getBoolean("returned"); return enabled ? "yes" : "no"; }
      void initialized() { boolean enabled = Boolean.getBoolean("initialized"); String value = enabled ? "yes" : "no"; }
      void boundaries() {
        boolean enabled = Boolean.getBoolean("boundaries");
        java.util.function.Supplier<String> later = () -> enabled ? "yes" : "no";
        { String nested = enabled ? "yes" : "no"; }
        enabled = false; String afterWrite = enabled ? "yes" : "no";
      }
    }"#,
    );
    for key in ["returned", "initialized"] {
        assert_eq!(
            records
                .iter()
                .filter(|r| r.source_key == key && r.edge_kind == "guards_code")
                .count(),
            1
        );
    }
    assert!(
        !records
            .iter()
            .any(|r| r.source_key == "boundaries" && r.edge_kind == "guards_code")
    );
}

#[path = "receiver_proof_tests.rs"]
mod receiver_proof;
