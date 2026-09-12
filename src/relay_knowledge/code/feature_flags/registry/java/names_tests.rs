use crate::code::feature_flags::registry::test_support::*;

#[test]
fn this_qualified_key_uses_the_field_despite_a_shadowing_parameter() {
    let rows = facts(
        "java",
        r#"package demo; class Config {
        static final String FEATURE_KEY="feature_x";
        String getX(String FEATURE_KEY) { return System.getProperty(this.FEATURE_KEY); }
    }"#,
    );
    let read = rows.iter().find(|r| r.edge_kind == "reads_config").unwrap();
    assert_eq!(
        read.metadata.reference.as_deref(),
        Some("demo.Config.FEATURE_KEY")
    );
}

#[test]
fn wildcard_receiver_imports_respect_explicit_and_local_type_precedence() {
    for (imports, declaration, expected) in [
        (
            "import demo.config.*;",
            "",
            "<ambiguous-import>.Config.getX",
        ),
        (
            "import demo.config.*; import explicit.Config;",
            "",
            "explicit.Config.getX",
        ),
        (
            "import demo.config.*;",
            "class Config {}",
            "app.Config.getX",
        ),
        (
            "import first.*; import second.*;",
            "",
            "<ambiguous-import>.Config.getX",
        ),
    ] {
        let rows = facts(
            "java",
            &format!(
                "package app; {imports} {declaration} class Reader {{ void run(Config config) {{ if(config.getX()) {{}} }} }}"
            ),
        );
        assert!(
            rows.iter()
                .any(|r| r.metadata.reference.as_deref() == Some(expected)),
            "{expected}"
        );
    }
}

#[test]
fn java_nested_getter_types_resolve_package_and_import_prefixes() {
    let rows = facts(
        "java",
        r#"package demo;
        class Outer { static class Config { boolean getX() { return Boolean.getBoolean("feature_x"); } } }
        class Reader { void run(Outer.Config local, demo.Outer.Config full) { if(local.getX()) {} if(full.getX()) {} } }
    "#,
    );
    assert!(rows.iter().any(|r| {
        r.metadata
            .bindings
            .contains(&"demo.Outer.Config.getX".into())
    }));
    let guards = rows
        .iter()
        .filter(|r| r.edge_kind == "guards_code")
        .collect::<Vec<_>>();
    assert_eq!(guards.len(), 2);
    assert!(
        guards
            .iter()
            .all(|r| r.metadata.reference.as_deref() == Some("demo.Outer.Config.getX"))
    );
    let imported = facts(
        "java",
        "package app; import demo.Outer; class Reader { void run(Outer.Config config) { if(config.getX()) {} } }",
    );
    assert!(
        imported
            .iter()
            .any(|r| r.metadata.reference.as_deref() == Some("demo.Outer.Config.getX"))
    );
}

#[test]
fn constant_keys_and_getter_interfaces_produce_qualified_bindings() {
    let rows = facts(
        "java",
        r#"package demo;
    interface FooConfig { boolean getX(); }
    class DefaultFooConfig implements FooConfig { public boolean getX() { return Boolean.getBoolean(Keys.X); } }
    class Keys { static final String X="feature_"+"x"; }
    class Reader { FooConfig field; void run(FooConfig parameter) { FooConfig local=parameter; if(field.getX()){} if(local.getX()){} if(parameter.getX()){} } }
    "#,
    );
    assert!(
        rows.iter().any(|r| r.source_key == "feature_x"
            && r.metadata.bindings.contains(&"demo.Keys.X".to_owned()))
    );
    assert!(rows.iter().any(|r| {
        r.metadata.reference.as_deref() == Some("demo.Keys.X")
            && r.metadata
                .bindings
                .contains(&"demo.FooConfig.getX".to_owned())
    }));
    assert_eq!(
        rows.iter()
            .filter(|r| r.edge_kind == "guards_code"
                && r.metadata.reference.as_deref() == Some("demo.FooConfig.getX"))
            .count(),
        3
    );
}

#[test]
fn platform_shadows_and_anonymous_getters_are_not_configuration_reads() {
    let rows = facts(
        "java",
        r#"class App { Other System; Other Boolean; void run() {
      System.getProperty("fake"); Boolean.getBoolean("fake");
      java.lang.System.getProperty("real");
      new Config(){ boolean getX(){return false;} }.getX();
    }}"#,
    );
    assert!(!rows.iter().any(|r| r.source_key == "fake"));
    assert_eq!(
        rows.iter()
            .filter(|r| r.edge_kind == "reads_config")
            .count(),
        1
    );
}

#[test]
fn static_platform_imports_and_environment_namespace_are_preserved() {
    let rows = facts(
        "java",
        r#"import static java.lang.System.getenv; import static java.lang.Boolean.getBoolean;
      class App { void run() { getenv("FLAG"); if(getBoolean("FLAG")){} } }"#,
    );
    assert!(
        rows.iter()
            .any(|r| r.source_kind == "env_var" && r.source_key == "FLAG")
    );
    assert!(
        rows.iter()
            .any(|r| r.source_kind == "config_key" && r.edge_kind == "guards_code")
    );
}

#[test]
fn explicit_fields_bypass_local_shadows_and_nested_local_guards_stay_local() {
    let rows = facts(
        "java",
        r#"class App { Config config; void run(Other config) {
      if(this.config.getX()){}
      boolean enabled=Boolean.getBoolean("outer");
      { boolean enabled=false; if(enabled){} }
      if(enabled){}
    }}"#,
    );
    assert!(
        rows.iter()
            .any(|r| r.metadata.reference.as_deref() == Some("Config.getX"))
    );
    assert_eq!(
        rows.iter()
            .filter(|r| r.source_key == "outer" && r.edge_kind == "guards_code")
            .count(),
        1
    );
}

#[test]
fn pattern_receiver_uses_the_type_in_its_boolean_flow_scope() {
    for body in [
        "if(value instanceof Other config){config.getX();}",
        "if(!(value instanceof Other config)){}else{config.getX();}",
        "if(value instanceof Other config && config.getX()){}",
        "if(!(value instanceof Other config)||config.getX()){}",
        "if(!(value instanceof Other config))return;config.getX();",
    ] {
        let source = format!("class App {{ Config config; void run(Object value) {{ {body} }} }}");
        let rows = facts("java", &source);
        assert!(
            rows.iter()
                .any(|r| r.metadata.reference.as_deref() == Some("Other.getX")),
            "{body}: {rows:?}"
        );
        assert!(
            !rows
                .iter()
                .any(|r| r.metadata.reference.as_deref() == Some("Config.getX")),
            "{body}"
        );
    }
}

#[test]
fn ordinary_string_constants_remain_internal_candidates() {
    let rows = facts(
        "java",
        r#"class Messages {
      static final String GREETING="Welcome to the application";
      static final String NAME="application.name";
      static final String TIMEOUT_KEY="timeout";
    }"#,
    );
    for key in ["Welcome to the application", "application.name"] {
        assert_eq!(
            rows.iter().find(|r| r.source_key == key).unwrap().edge_kind,
            "declares_string_constant"
        );
    }
    assert_eq!(
        rows.iter()
            .find(|r| r.source_key == "timeout")
            .unwrap()
            .edge_kind,
        "declares_config_key"
    );
}

#[test]
fn static_imports_ignore_sibling_nested_and_inapplicable_methods() {
    let rows = facts(
        "java",
        r#"import static java.lang.System.getenv;
      class Sibling { String getenv(String key) { return key; } }
      class App { class Nested { String getenv(String key) { return key; } }
        String getenv(int index) { return "local"; }
        void run() { getenv("REAL_ENV"); }
      }
      class Shadow { String getenv(String key) { return key; }
        void run() { getenv("NOT_ENV"); }
      }"#,
    );
    assert!(
        rows.iter()
            .any(|r| r.source_key == "REAL_ENV" && r.source_kind == "env_var")
    );
    assert!(!rows.iter().any(|r| r.source_key == "NOT_ENV"));
}

#[test]
fn generic_receivers_and_simple_assignments_keep_config_guards() {
    let rows = facts(
        "java",
        r#"package demo;
      interface FooConfig<T> { boolean getX(); }
      class DefaultFooConfig implements FooConfig<Prod> { public boolean getX() { return Boolean.getBoolean("feature_x"); } }
      class App { void run(FooConfig<Prod> config) {
        if(config.getX()) {} boolean enabled;
        enabled = Boolean.getBoolean("feature_y"); if(enabled) {}
        enabled = false; if(enabled) {}
      }}"#,
    );
    assert!(rows.iter().any(|r| r.edge_kind == "guards_code"
        && r.metadata.reference.as_deref() == Some("demo.FooConfig.getX")));
    assert!(
        !rows
            .iter()
            .any(|r| r.metadata.bindings.contains(&"demo.Prod.getX".into()))
    );
    assert_eq!(
        rows.iter()
            .filter(|r| r.source_key == "feature_y" && r.edge_kind == "guards_code")
            .count(),
        1
    );
}

#[test]
fn explicit_static_imports_take_precedence_over_wildcards() {
    let rows = facts(
        "java",
        r#"import static java.lang.System.*; import static custom.Env.getenv;
      class App { void run() { getenv("NOT_ENV"); } }"#,
    );
    assert!(!rows.iter().any(|r| r.source_key == "NOT_ENV"));
    let rows = facts(
        "java",
        r#"import static custom.Env.*; import static java.lang.System.getenv;
      class App { void run() { getenv("REAL_ENV"); } }"#,
    );
    assert!(
        rows.iter()
            .any(|r| r.source_key == "REAL_ENV" && r.source_kind == "env_var")
    );
}

#[test]
fn numeric_addition_is_not_misreported_as_string_concatenation() {
    let rows = facts(
        "java",
        r#"class App { void run() {
        System.getProperty("port." + (1 + 2)); System.getProperty(1 + 2 + ".port");
        System.getProperty("port." + 1 + 2); System.getProperty("port." + ("a" + "b"));
    }}"#,
    );
    let keys = rows
        .iter()
        .filter(|r| r.edge_kind == "reads_config")
        .map(|r| r.source_key.as_str())
        .collect::<Vec<_>>();
    assert_eq!(keys, ["port.12", "port.ab"]);
}

#[test]
fn implicit_conversion_owners_carry_snapshot_shadow_requirements() {
    for (owner, method) in [
        ("Boolean", "parseBoolean"),
        ("Integer", "parseInt"),
        ("Long", "parseLong"),
        ("Double", "parseDouble"),
    ] {
        let source = format!(
            "package app; class Config {{ Object getX() {{ return {owner}.{method}(System.getProperty(\"flag\")); }} }}"
        );
        let rows = facts("java", &source);
        let read = rows.iter().find(|r| r.source_key == "flag").unwrap();
        assert_eq!(
            read.metadata.conversion_platform_owners,
            [format!("app.{owner}")]
        );
        for source in [
            source.replace(
                &format!("return {owner}."),
                &format!("return java.lang.{owner}."),
            ),
            source.replace(
                "package app;",
                &format!("package app; import java.lang.{owner};"),
            ),
        ] {
            let rows = facts("java", &source);
            assert!(
                rows.iter()
                    .filter(|r| r.source_key == "flag")
                    .all(|r| r.metadata.conversion_platform_owners.is_empty())
            );
        }
        assert!(
            raw_facts("java", &format!("package app; class {owner} {{}}"))
                .iter()
                .any(|r| r.source_key == format!("app.{owner}")
                    && r.edge_kind == "config_type_declaration")
        );
    }
}

#[test]
fn enhanced_for_receivers_use_loop_types_and_stop_outer_field_fallback() {
    let rows = facts(
        "java",
        r#"class App { WrongConfig config; void run() {
        for (FooConfig config : configs) { if(config.getX()) {} }
        for (var config : unknown) { config.getX(); }
    }}"#,
    );
    assert!(
        rows.iter()
            .any(|r| r.metadata.reference.as_deref() == Some("FooConfig.getX"))
    );
    assert!(
        !rows
            .iter()
            .any(|r| r.metadata.reference.as_deref() == Some("WrongConfig.getX"))
    );
}

#[test]
fn composed_java_keys_resolve_bounded_final_string_references() {
    let rows = facts(
        "java",
        r#"class Keys {
        static final String PREFIX="feature.";
        static final String KEY=PREFIX+"x";
        static final String ALIAS=KEY;
        static String mutable="feature.";
        static final String BAD=mutable+"bad";
        static final String CYCLE=CYCLE+"loop";
    }
    class Reader { void run() { System.getProperty(Keys.KEY); } }
    class Local { static final String KEY=Keys.PREFIX+"local"; }"#,
    );
    assert!(
        rows.iter().any(
            |r| r.source_key == "feature.x" && r.metadata.bindings.contains(&"Keys.KEY".into())
        )
    );
    assert!(
        rows.iter()
            .any(|r| r.source_key == "feature.x"
                && r.metadata.bindings.contains(&"Keys.ALIAS".into()))
    );
    assert!(rows.iter().any(|r| r.source_key == "feature.local"));
    assert!(!rows.iter().any(|r| r.source_key == "feature.bad"));
    assert!(
        !rows
            .iter()
            .any(|r| r.metadata.bindings.contains(&"Keys.CYCLE".into()))
    );
    assert!(
        rows.iter()
            .any(|r| r.metadata.reference.as_deref() == Some("Keys.KEY"))
    );
}

#[test]
fn inferred_lambda_parameters_do_not_resolve_to_outer_config_fields() {
    for parameters in ["config", "(config)", "(config, item)"] {
        let rows = facts(
            "java",
            &format!(
                "class App {{ FooConfig config; void run() {{ items.map({parameters} -> config.getX()); if(config.getX()) {{}} }} }}"
            ),
        );
        assert_eq!(
            rows.iter()
                .filter(|r| r.edge_kind == "reads_config"
                    && r.metadata.reference.as_deref() == Some("FooConfig.getX"))
                .count(),
            1
        );
    }
}

#[test]
fn sibling_nested_platform_names_do_not_shadow_unrelated_java_callers() {
    let rows = facts(
        "java",
        r#"class A { void run() { System.getProperty("property"); Boolean.getBoolean("boolean"); } }
        class B { static class System {} static class Boolean {} }
        class C { static class System {} void run() { System.getProperty("shadowed"); } }"#,
    );
    assert!(rows.iter().any(|r| r.source_key == "property"));
    assert!(rows.iter().any(|r| r.source_key == "boolean"));
    assert!(!rows.iter().any(|r| r.source_key == "shadowed"));
}

#[test]
fn java_keys_use_java_escape_semantics() {
    let rows = facts(
        "java",
        r#"class App { void run() {
      System.getProperty("\141"); System.getProperty("\b\s\t\n\f\r");
      System.getProperty("\u0062"); System.getProperty("\3777");
    }}"#,
    );
    for key in ["a", "\u{0008} \t\n\u{000c}\r", "b", "ÿ7"] {
        assert!(rows.iter().any(|r| r.source_key == key), "{key:?}");
    }
}

#[test]
fn nested_java_receivers_use_visible_lexical_type_owners() {
    let rows = facts(
        "java",
        r#"package app; class Reader {
      static class Config { boolean getX() { return Boolean.getBoolean("flag"); } }
      void run(Config c) { if(c.getX()) {} }
    }"#,
    );
    assert!(rows.iter().any(|r| {
        r.metadata
            .bindings
            .contains(&"app.Reader.Config.getX".into())
    }));
    assert!(rows.iter().any(|r| r.edge_kind == "guards_code"
        && r.metadata.reference.as_deref() == Some("app.Reader.Config.getX")));
    assert!(
        !rows
            .iter()
            .any(|r| r.metadata.reference.as_deref() == Some("app.Config.getX"))
    );
}

#[test]
fn statically_imported_numeric_conversions_bind_configuration_getters() {
    for (owner, method, ty) in [
        ("Integer", "parseInt", "int"),
        ("Long", "parseLong", "long"),
        ("Double", "parseDouble", "double"),
    ] {
        let source = format!(
            "import static java.lang.{owner}.{method}; class App {{ {ty} getPort() {{ return {method}(System.getProperty(\"port\")); }} }}"
        );
        let rows = facts("java", &source);
        assert!(
            rows.iter()
                .any(|r| r.metadata.bindings.contains(&"App.getPort".into())
                    && r.metadata.flow_incomplete.is_none()),
            "{owner}"
        );
    }
}

#[test]
fn direct_java_config_readers_retain_legacy_keys_and_guards() {
    let rows = facts(
        "java",
        r#"class App { void run() {
      if(config.getBoolean("checkout")) {} settings.get("mode");
      options.get("\141", "fallback");
      String text="config.getBoolean(\"fake\")";
      // settings.get("comment")
    }}"#,
    );
    for key in ["checkout", "mode", "a"] {
        assert!(
            rows.iter()
                .any(|r| r.source_key == key && r.edge_kind == "reads_config"),
            "{key}"
        );
    }
    assert!(
        rows.iter()
            .any(|r| r.source_key == "checkout" && r.edge_kind == "guards_code")
    );
    assert!(
        !rows
            .iter()
            .any(|r| matches!(r.source_key.as_str(), "fake" | "comment"))
    );
}

#[test]
fn static_getter_receivers_resolve_local_and_explicit_imported_types() {
    let rows = facts(
        "java",
        r#"package app; class FeatureConfig { static boolean isEnabled() { return Boolean.getBoolean("flag"); } }
      class Reader { void run() { if(FeatureConfig.isEnabled()) {} } }"#,
    );
    assert!(rows.iter().any(|r| r.edge_kind == "guards_code"
        && r.metadata.reference.as_deref() == Some("app.FeatureConfig.isEnabled")));
    let rows = facts(
        "java",
        "package reader; import settings.FeatureConfig; class Reader { void run() { FeatureConfig.isEnabled(); settings.FeatureConfig.isEnabled(); } }",
    );
    assert_eq!(
        rows.iter()
            .filter(|r| r.metadata.reference.as_deref() == Some("settings.FeatureConfig.isEnabled"))
            .count(),
        2
    );
    let rows = facts(
        "java",
        "class FeatureConfig {} class Reader { void run(Unknown FeatureConfig) { FeatureConfig.isEnabled(); } }",
    );
    assert!(
        !rows
            .iter()
            .any(|r| r.metadata.reference.as_deref() == Some("FeatureConfig.isEnabled"))
    );
}

#[test]
fn direct_java_reader_keys_require_string_expressions() {
    let rows = facts(
        "java",
        r#"class App { static final String KEY="flag"; void run() {
      config.get(0); config.get(true); config.get(1+2); config.get((5));
      config.get("flag."+1); config.get(KEY); config.get("limit", 5);
    }}"#,
    );
    assert!(!rows.iter().any(|r| r.edge_kind == "reads_config"
        && matches!(r.source_key.as_str(), "0" | "true" | "3" | "5")));
    assert!(rows.iter().any(|r| r.source_key == "flag.1"));
    assert!(
        rows.iter()
            .any(|r| r.metadata.reference.as_deref() == Some("App.KEY"))
    );
    assert!(
        rows.iter()
            .any(|r| r.source_key == "limit" && r.metadata.default_value.as_deref() == Some("5"))
    );
}

#[test]
fn wildcard_getter_receivers_retain_same_package_candidates_and_types() {
    let rows = facts(
        "java",
        "package app; import java.util.*; class Reader { void run(Config config) { if(config.isEnabled()) {} } }",
    );
    assert!(rows.iter().any(|r| r.edge_kind == "guards_code"
        && r.metadata.same_package_reference.as_deref() == Some("app.Config.isEnabled")));
    assert!(
        raw_facts("java", "package app; class Config {}")
            .iter()
            .any(|r| r.edge_kind == "config_type_declaration" && r.source_key == "app.Config")
    );
}

#[test]
fn java_super_and_constructed_receivers_preserve_exact_owner_evidence() {
    let rows = facts(
        "java",
        r#"package app; class Base { boolean isX() { return Boolean.getBoolean("base"); } }
      class Child extends Base { boolean isX() { return Boolean.getBoolean("child"); } void run() { if(super.isX()) {} if(new Base().isX()) {} if(((Base)new Child()).isX()) {} } }"#,
    );
    for owner in ["app.Base.isX", "app.Child.isX"] {
        assert!(
            rows.iter().any(|r| r.edge_kind == "guards_code"
                && r.metadata.reference.as_deref() == Some(owner)
                && r.metadata.exact_reference),
            "{rows:?}"
        );
        assert!(
            rows.iter()
                .any(|r| r.metadata.declared_getter.as_deref() == Some(owner))
        );
    }
}

#[test]
fn java_numeric_defaults_are_canonical_values() {
    let rows = facts(
        "java",
        r#"class App { void run() { config.get("limit", 1_000); config.get("timeout", 10L); config.get("ratio", 10.0D); config.get("port." + 1_0L); config.get("float." + 1.0); }}"#,
    );
    for (key, expected) in [("limit", "1000"), ("timeout", "10"), ("ratio", "10")] {
        assert_eq!(
            rows.iter()
                .find(|r| r.source_key == key)
                .unwrap()
                .metadata
                .default_value
                .as_deref(),
            Some(expected)
        );
    }
    assert!(rows.iter().any(|r| r.source_key == "port.10"));
    assert!(!rows.iter().any(|r| r.source_key.starts_with("float.")));
}

#[test]
fn wildcard_static_key_imports_preserve_reads_and_ambiguity() {
    for (imports, reference) in [
        ("import static app.Keys.*;", "app.Keys.FEATURE_KEY"),
        (
            "import static app.Keys.*; import static other.Keys.*;",
            "<ambiguous-import>.FEATURE_KEY",
        ),
    ] {
        let rows = facts(
            "java",
            &format!(
                "{imports} class Reader {{ String read() {{ return System.getProperty(FEATURE_KEY); }} }}"
            ),
        );
        assert!(
            rows.iter().any(|r| r.edge_kind == "reads_config"
                && r.metadata.reference.as_deref() == Some(reference))
        );
    }
}
