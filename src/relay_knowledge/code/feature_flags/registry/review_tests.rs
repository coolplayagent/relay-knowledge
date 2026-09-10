//! Scope, truncation and literal-preservation review regressions.
use super::*;

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
