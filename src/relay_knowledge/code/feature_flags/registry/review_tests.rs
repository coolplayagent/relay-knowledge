//! Scope, truncation and literal-preservation review regressions.
use super::*;
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
fn properties_semicolons_are_key_content_while_ini_semicolons_are_comments() {
    let rows = facts("properties", ";feature.enabled=true\n");
    assert_eq!(rows[0].source_key, ";feature.enabled");
    assert!(facts("ini", ";feature.enabled=true\n").is_empty());
}

#[test]
fn properties_escaped_trailing_key_spaces_survive_assignment_splitting() {
    let rows = facts("properties", "feature\\ =on\nbare\\ \n");
    assert_eq!(rows[0].source_key, "feature ");
    assert_eq!(rows[0].metadata.default_value.as_deref(), Some("on"));
    assert_eq!(rows[1].source_key, "bare ");
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
fn template_control_pipelines_extract_nested_calls_without_reading_quoted_text() {
    let rows = facts(
        "gotemplate",
        r#"{{ with key "feature_x" }}
        {{ if (keyOrDefault "enabled" "false") }}
        {{ if eq (env "FLAG") "yes" }}
        {{ printf "key \"not_a_read\"" }}
        {{ if and (key "same") (key "same") }}"#,
    );
    assert_eq!(rows.len(), 5);
    assert!(
        rows.iter()
            .any(|r| r.source_key == "enabled"
                && r.metadata.default_value.as_deref() == Some("false"))
    );
    assert!(
        rows.iter()
            .any(|r| r.source_kind == "env_var" && r.source_key == "FLAG")
    );
    let repeated = rows
        .iter()
        .filter(|r| r.source_key == "same")
        .collect::<Vec<_>>();
    assert_eq!(repeated.len(), 2);
    assert_ne!(repeated[0].usage_id, repeated[1].usage_id);
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
fn shell_scan_exhaustion_is_explicit_incomplete_analysis() {
    let source = format!(
        "set -a\n{}FLAG=value\necho \"$FLAG\"\n",
        "echo ignored\n".repeat(1100)
    );
    let error = extract(&FeatureFlagFileInput {
        repository_id: "repo",
        source_scope: "scope",
        file_id: "file",
        path: "config.sh",
        language_id: "bash",
        content: &source,
        config_facts: &[],
    })
    .map(|_| ())
    .expect_err("scan truncation cannot claim no export");
    assert!(error.to_string().contains("incomplete"));
}

#[test]
fn properties_flush_pending_continuations_at_eof() {
    for source in ["flag=on\\", "flag=on\\\n", "flag=\\\n  on\\"] {
        let rows = facts("properties", source);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].metadata.default_value.as_deref(), Some("on"));
    }
}

#[test]
fn unicode_domain_annotations_use_unicode_lowercase() {
    let rows = facts("properties", "# @config domain=ÜBER\nflag=true\n");
    assert_eq!(rows[0].metadata.domain.as_deref(), Some("über"));
}

#[test]
fn shell_quote_forms_preserve_static_dollars_and_backslashes() {
    for (value, expected) in [
        ("'$HOME'", Some("$HOME")),
        ("'`command`'", Some("`command`")),
        ("\"\\$HOME\"", Some("$HOME")),
        ("\"\\n\"", Some("\\n")),
        ("'foo'\"bar\"", Some("foobar")),
        ("prefix\\$HOME", Some("prefix$HOME")),
        ("\"$HOME\"", None),
        ("$(command)", None),
    ] {
        let rows = facts("bash", &format!("export FLAG={value}\n"));
        let definition = rows
            .iter()
            .find(|r| r.edge_kind == "defines_config")
            .unwrap();
        assert_eq!(
            definition.metadata.default_value.as_deref(),
            expected,
            "{value}"
        );
    }
}

#[test]
fn annotations_require_source_format_comments() {
    for (language, source) in [
        (
            "java",
            "class App { void run() {\nString marker = \"@config domain=fake\";\nSystem.getProperty(\"flag\"); }}",
        ),
        ("properties", "marker=@config domain=fake\nflag=true\n"),
        ("ini", "marker=@config domain=fake\nflag=true\n"),
        ("bash", "echo '@config domain=fake'\necho $FLAG\n"),
        ("gotemplate", "@config domain=fake\n{{ key \"flag\" }}\n"),
    ] {
        assert!(
            facts(language, source)
                .iter()
                .all(|r| r.metadata.domain.is_none()),
            "{language}"
        );
    }
    for (language, source) in [
        ("properties", "! @config domain=valid\nflag=true\n"),
        ("ini", "; @config domain=valid\nflag=true\n"),
        ("bash", "# @config domain=valid\nexport FLAG=true\n"),
        (
            "gotemplate",
            "{{/* @config domain=valid */}}\n{{ key \"flag\" }}\n",
        ),
        (
            "java",
            "class App { void run() {\n// @config domain=valid\nSystem.getProperty(\"flag\"); }}",
        ),
    ] {
        assert!(
            facts(language, source)
                .iter()
                .any(|r| r.metadata.domain.as_deref() == Some("valid")),
            "{language}"
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
fn conditional_shell_assignments_preserve_potential_inherited_reads() {
    for source in [
        "if test -f local; then FLAG=local; fi; echo $FLAG",
        "if test -f local; then export -n FLAG; fi; echo $FLAG",
    ] {
        let rows = facts("bash", source);
        assert!(
            rows.iter()
                .any(|r| r.source_key == "FLAG" && r.edge_kind == "reads_config")
        );
        assert!(!rows.iter().any(|r| r.edge_kind == "defines_config"));
    }
    let rows = facts(
        "bash",
        "FLAG=local; if test -f local; then FLAG=other; fi; echo $FLAG",
    );
    assert!(!rows.iter().any(|r| r.edge_kind == "reads_config"));
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
