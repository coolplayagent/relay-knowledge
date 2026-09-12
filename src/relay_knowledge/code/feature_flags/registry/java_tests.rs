use super::*;
use crate::code::feature_flags::registry::test_support::*;

#[test]
fn return_only_getters_ignore_comments_but_not_executable_statements() {
    let rows = facts(
        "java",
        r#"class Config {
        boolean getX() { /* context */ return Boolean.getBoolean("flag"); // explanation
        }
        boolean getY() { log(); return Boolean.getBoolean("other"); }
    }"#,
    );
    let row = rows.iter().find(|r| r.source_key == "flag").unwrap();
    assert!(row.metadata.bindings.contains(&"Config.getX".into()));
    assert!(row.metadata.flow_incomplete.is_none());
    assert!(
        rows.iter()
            .find(|r| r.source_key == "other")
            .unwrap()
            .metadata
            .flow_incomplete
            .is_some()
    );
}

#[test]
fn local_guard_reads_exclude_unrelated_method_and_field_names() {
    let rows = facts(
        "java",
        r#"class App { void run(Service service) {
        boolean enabled = Boolean.getBoolean("feature_x");
        if (service.enabled()) {} if (service.enabled) {} if (this.enabled) {}
        if (enabled) {} if (service.accept(enabled)) {}
    }}"#,
    );
    assert_eq!(
        rows.iter().filter(|r| r.edge_kind == "guards_code").count(),
        2
    );
}

#[test]
fn java_getter_markers_and_collection_obey_the_file_budget() {
    for (methods, reads) in [(10_001, 0), (10_000, 1)] {
        let mut source = String::from("class Many {");
        for index in 0..methods {
            source.push_str(&format!("boolean get{index}() {{ return false; }}"));
        }
        if reads > 0 {
            source.push_str("static final String FLAG_KEY=\"flag\";");
        }
        source.push('}');
        let error = extract(&FeatureFlagFileInput {
            repository_id: "repo",
            source_scope: "scope",
            file_id: "file",
            path: "Many.java",
            language_id: "java",
            content: &source,
            config_facts: &[],
        })
        .map(|_| ())
        .expect_err("over-budget getter facts must fail");
        assert!(error.to_string().contains("budget exceeded"));
    }
}

#[test]
fn java_field_annotations_apply_before_variable_declarator() {
    let rows = facts(
        "java",
        "class Constants {\n// @config domain=business hot-reload=true\nstatic final String NAME=\"feature_x\";\n}",
    );
    let row = rows.iter().find(|r| r.source_key == "feature_x").unwrap();
    assert_eq!(row.edge_kind, "declares_config_key");
    assert_eq!(row.metadata.domain.as_deref(), Some("business"));
    assert_eq!(row.metadata.hot_reload, Some(true));
    assert_eq!(row.line_range.start, 3);
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
fn local_value_aliases_mark_guard_flow_incomplete() {
    for alias in [
        "boolean active = enabled;",
        "active = enabled;",
        "boolean active = !enabled;",
    ] {
        let source = format!(
            "class App {{ void run() {{ boolean enabled = config.getBoolean(\"flag\"); {alias} if(active) {{}} }} }}"
        );
        let rows = facts("java", &source);
        assert!(
            rows.iter().any(|r| r.source_key == "flag"
                && r.metadata.flow_incomplete.as_deref() == Some("unsupported_local_alias")),
            "{rows:?}"
        );
    }
}

#[test]
fn inline_java_switch_selectors_link_guard_usages() {
    let rows = facts(
        "java",
        r#"class App { void run() { switch(config.get("mode")) { case "on": break; default: break; } String result = switch(System.getProperty("shape")) { case "a" -> "x"; default -> "y"; }; } }"#,
    );
    for key in ["mode", "shape"] {
        let read = rows
            .iter()
            .find(|r| r.source_key == key && r.edge_kind == "reads_config")
            .unwrap();
        assert!(
            rows.iter().any(|r| r.edge_kind == "guards_code"
                && r.metadata.read_usage_id.as_deref() == Some(&read.usage_id)),
            "{rows:?}"
        );
    }
}

#[test]
fn implicit_getters_link_callers_and_guards_without_overload_fallback() {
    let rows = facts(
        "java",
        r#"class App {
      boolean isEnabled() { return Boolean.getBoolean("flag"); }
      void run() { if (isEnabled()) {} }
      class Inner { boolean isEnabled(int n) { return false; } void run() { isEnabled(); } }
    }"#,
    );
    assert!(rows.iter().any(|r| r.edge_kind == "reads_config"
        && r.metadata.reference.as_deref() == Some("App.isEnabled")));
    assert!(rows.iter().any(|r| r.edge_kind == "guards_code"
        && r.metadata.reference.as_deref() == Some("App.isEnabled")));
    assert!(
        !rows
            .iter()
            .any(|r| r.metadata.reference.as_deref() == Some("App.Inner.isEnabled"))
    );
}

#[test]
fn java_guard_scan_exhaustion_is_explicit() {
    let source = format!(
        "class App {{ void run() {{ boolean enabled=Boolean.getBoolean(\"flag\"); {} if(enabled) {{}} }} }}",
        "work();".repeat(700)
    );
    let error = extract(&FeatureFlagFileInput {
        repository_id: "repo",
        source_scope: "scope",
        file_id: "file",
        path: "App.java",
        language_id: "java",
        content: &source,
        config_facts: &[],
    })
    .unwrap_err();
    assert!(error.to_string().contains("guard analysis incomplete"));
}

#[test]
fn conditional_reassignment_retains_possible_guards_as_incomplete_flow() {
    let rows = facts(
        "java",
        r#"class App { void run(Config config) {
      boolean enabled=config.getX(); if(override) enabled=false; if(enabled) {}
    }}"#,
    );
    assert!(
        rows.iter().any(|r| r.edge_kind == "guards_code"
            && r.metadata.reference.as_deref() == Some("Config.getX"))
    );
    assert!(rows.iter().any(|r| r.edge_kind == "reads_config"
        && r.metadata.flow_incomplete.as_deref() == Some("conditional_reassignment")));
}

#[test]
fn boolean_reader_methods_provide_boolean_type_evidence() {
    for method in [
        "getBoolean",
        "get_bool",
        "get_boolean",
        "enabled",
        "is_enabled",
    ] {
        let rows = facts(
            "java",
            &format!("class App {{ void run() {{ config.{method}(\"flag\"); }} }}"),
        );
        assert_eq!(rows[0].metadata.value_type.as_deref(), Some("boolean"));
    }
}

#[test]
fn java_multiline_block_comments_supply_explicit_metadata() {
    for begin in ["/*", "/**"] {
        let rows = facts(
            "java",
            &format!(
                "class App {{ void run() {{\n{begin}\n * @config domain=payments hot-reload=true\n */\nSystem.getProperty(\"flag\"); }} }}"
            ),
        );
        assert_eq!(rows[0].metadata.domain.as_deref(), Some("payments"));
        assert_eq!(rows[0].metadata.hot_reload, Some(true));
    }
}

#[test]
fn direct_java_reads_capture_signed_defaults() {
    let rows = facts(
        "java",
        r#"class App { void run() { config.get("timeout", -1); config.get("limit", +10L); config.get("ratio", -1.25); }}"#,
    );
    for (key, value) in [("timeout", "-1"), ("limit", "10"), ("ratio", "-1.25")] {
        assert_eq!(
            rows.iter()
                .find(|r| r.source_key == key)
                .unwrap()
                .metadata
                .default_value
                .as_deref(),
            Some(value)
        );
    }
}

#[test]
fn boolean_conversions_preserve_nullable_property_fallback_only() {
    for conversion in ["parseBoolean", "valueOf"] {
        for (read, expected) in [
            (r#"System.getProperty("x")"#, Some("false")),
            (r#"System.getProperty("x", "true")"#, Some("true")),
            (r#"System.getenv("X")"#, None),
        ] {
            let rows = facts(
                "java",
                &format!(
                    "class Config {{ boolean getX() {{ return Boolean.{conversion}({read}); }} }}"
                ),
            );
            let row = rows.iter().find(|r| r.edge_kind == "reads_config").unwrap();
            assert_eq!(row.metadata.default_value.as_deref(), expected);
        }
    }
}

#[test]
fn boolean_conversions_normalize_explicit_property_fallbacks() {
    for (fallback, expected) in [
        ("TRUE", "true"),
        ("True", "true"),
        ("false", "false"),
        ("other", "false"),
        ("", "false"),
    ] {
        let source = format!(
            r#"class Config {{ boolean getX() {{ return Boolean.parseBoolean(System.getProperty("flag", "{fallback}")); }} }}"#
        );
        let rows = facts("java", &source);
        let read = rows.iter().find(|r| r.edge_kind == "reads_config").unwrap();
        assert_eq!(read.metadata.default_value.as_deref(), Some(expected));
        assert_eq!(read.metadata.value_type.as_deref(), Some("boolean"));
        assert_eq!(read.metadata.unconverted_default.as_deref(), Some(fallback));
    }
}
