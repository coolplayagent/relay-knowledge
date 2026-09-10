use super::*;
fn facts(language: &str, source: &str) -> Vec<CodeFeatureFlagRecord> {
    extract(&FeatureFlagFileInput {
        repository_id: "repo",
        source_scope: "scope",
        file_id: "file",
        path: "sample",
        language_id: language,
        content: source,
        config_facts: &[],
    })
    .unwrap()
}
#[test]
fn java_multiline_reads_have_distinct_identities_and_explicit_guard_links() {
    let rows = facts(
        "java",
        r#"class App { void run() {
      String a=System.getProperty(
        "feature_x", "fallback"); System.getProperty("feature_x");
      boolean enabled=Boolean.getBoolean("feature_x");
      class Local { void reset() { boolean enabled=false; } }
      if(enabled) {} enabled=false; if(enabled) {}
      if(Boolean.getBoolean("direct")) {}
    }}"#,
    );
    let reads = rows
        .iter()
        .filter(|r| r.source_key == "feature_x" && r.edge_kind == "reads_config")
        .collect::<Vec<_>>();
    assert_eq!(reads.len(), 3);
    assert_eq!(
        reads
            .iter()
            .map(|r| &r.usage_id)
            .collect::<std::collections::BTreeSet<_>>()
            .len(),
        3
    );
    let guards = rows
        .iter()
        .filter(|r| r.edge_kind == "guards_code")
        .collect::<Vec<_>>();
    assert_eq!(guards.len(), 2);
    for guard in guards {
        assert!(rows.iter().any(|read| Some(&read.usage_id)
            == guard.metadata.read_usage_id.as_ref()
            && read.edge_kind == "reads_config"));
    }
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
fn properties_values_unicode_continuations_and_metadata_are_static_facts() {
    let rows = facts(
        "properties",
        "# @config domain=business hot-reload=true\nfeature_x=true\nname=hello\\\n  world\nfeature\\u005fy=42\n",
    );
    let first = &rows[0];
    assert_eq!(first.metadata.domain.as_deref(), Some("business"));
    assert_eq!(first.metadata.hot_reload, Some(true));
    assert_eq!(
        rows[1].metadata.default_value.as_deref(),
        Some("helloworld")
    );
    assert_eq!(rows[2].source_key, "feature_y");
    assert_eq!(rows[2].metadata.value_type.as_deref(), Some("integer"));
}
#[test]
fn templates_separate_declarations_from_reads_and_keep_quoted_delimiters() {
    let rows = facts(
        "gotemplate",
        "feature_x={{ key \"feature_x\" }}\n{{ env \"FLAG\" }}\nfeature_y={{ key \"a}}b\" }}\n",
    );
    assert_eq!(
        rows.iter()
            .filter(|r| r.edge_kind == "declares_config_key")
            .count(),
        2
    );
    assert!(
        rows.iter()
            .any(|r| r.source_key == "a}}b" && r.edge_kind == "reads_config")
    );
    assert!(
        rows.iter()
            .any(|r| r.source_kind == "env_var" && r.source_key == "FLAG")
    );
    assert!(!rows.iter().any(|r| r.edge_kind == "defines_config"));
}
#[test]
fn shell_exports_are_definitions_and_local_compound_assignments_do_not_leak() {
    let rows = facts(
        "bash",
        "export FLAG=true\necho \"$FLAG\"\n{ LOCAL=hidden; }; echo \"$LOCAL\"\n",
    );
    assert_eq!(rows.iter().filter(|r| r.source_key == "FLAG").count(), 2);
    assert!(!rows.iter().any(|r| r.source_key == "LOCAL"));
}
#[test]
fn ini_sections_do_not_alias_same_named_keys() {
    let rows = facts("ini", "feature_x=true\n[service]\nfeature_x=false\n");
    assert_eq!(
        rows.iter()
            .map(|r| r.source_key.as_str())
            .collect::<Vec<_>>(),
        ["feature_x", "service.feature_x"]
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
fn shell_export_status_survives_assignments_and_respects_unexport_and_subshells() {
    for (source, expected) in [
        ("export FLAG=yes; { FLAG=no; }; echo $FLAG", true),
        ("FLAG=yes; export FLAG; echo $FLAG", true),
        ("export FLAG=yes; export -n FLAG; echo $FLAG", false),
        ("( FLAG=no ); echo $FLAG", true),
        ("if true; then FLAG=no; fi; echo $FLAG", false),
    ] {
        let rows = facts("bash", source);
        assert_eq!(
            rows.iter()
                .any(|r| r.source_key == "FLAG" && r.edge_kind == "reads_config"),
            expected,
            "{source}"
        );
    }
}

#[test]
fn converted_getter_reads_bind_but_unknown_wrappers_report_incomplete_flow() {
    let rows = facts(
        "java",
        r#"package demo;
      interface FooConfig { boolean getX(); }
      class DefaultFooConfig implements FooConfig {
        public boolean getX() { return Boolean.parseBoolean(System.getProperty("feature_x", "false")); }
        public boolean getUnknown() { return Custom.wrap(System.getProperty("unknown")); }
      }
      class Reader { void run(FooConfig cfg) { if(cfg.getX()) {} } }
    "#,
    );
    let read = rows.iter().find(|r| r.source_key == "feature_x").unwrap();
    assert!(
        read.metadata
            .bindings
            .contains(&"demo.FooConfig.getX".into())
    );
    assert!(read.metadata.flow_incomplete.is_none());
    assert!(
        rows.iter()
            .find(|r| r.source_key == "unknown")
            .unwrap()
            .metadata
            .flow_incomplete
            .is_some()
    );
    let shadow = facts(
        "java",
        r#"class Config { Custom Boolean;
      boolean getX() { return Boolean.parseBoolean(System.getProperty("x")); }
    }"#,
    );
    let read = shadow.iter().find(|r| r.source_key == "x").unwrap();
    assert!(read.metadata.bindings.is_empty());
    assert!(read.metadata.flow_incomplete.is_some());
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
