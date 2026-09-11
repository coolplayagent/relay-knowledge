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
fn shell_ansi_c_defaults_are_unknown_without_losing_definitions() {
    let rows = facts("bash", r#"export FLAG=$'on\n'; export MIX=pre$'\t'post"#);
    assert_eq!(
        rows.iter()
            .filter(|r| r.edge_kind == "defines_config")
            .count(),
        2
    );
    assert!(
        rows.iter()
            .all(|r| r.metadata.default_value.is_none() && r.metadata.value_type.is_none())
    );
}
#[test]
fn shell_function_exports_do_not_change_variable_export_state() {
    for option in ["-f", "-fn", "-nf"] {
        let rows = facts(
            "bash",
            &format!("FLAG=local; export {option} FLAG; echo $FLAG"),
        );
        assert!(rows.is_empty(), "{option}: {rows:?}");
    }
    let rows = facts("bash", "export FLAG=on; export -f FLAG; echo $FLAG");
    assert!(rows.iter().any(|r| r.edge_kind == "reads_config"));
}
#[test]
fn properties_natural_lines_preserve_offsets_and_continuations() {
    for ending in ["\r", "\n", "\r\n"] {
        let content =
            format!("flag=true{ending}other=false{ending}long=first\\{ending} second{ending}");
        let rows = facts("properties", &content);
        assert_eq!(rows.len(), 3, "{ending:?}");
        assert_eq!(rows[1].source_key, "other");
        assert_eq!(rows[1].line_range.start, 2);
        assert_eq!(rows[1].line_range.end, 2);
        assert_eq!(
            &content[rows[1].byte_range.start as usize..rows[1].byte_range.end as usize],
            "other=false"
        );
        assert_eq!(
            rows[2].metadata.default_value.as_deref(),
            Some("firstsecond")
        );
        assert_eq!(rows[2].line_range.start, 3);
        assert_eq!(rows[2].line_range.end, 4);
    }
}

#[test]
fn properties_comments_cannot_continue_into_definitions() {
    for comment in ["# note\\", "! note\\", "  # note\\"] {
        let rows = facts("properties", &format!("{comment}\nflag=true\n"));
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].source_key, "flag");
        assert_eq!(rows[0].line_range.start, 2);
    }
}
#[test]
fn template_keys_and_defaults_follow_go_escape_semantics() {
    let rows = facts(
        "gotemplate",
        r#"{{ key "feature\x2ex" }} {{ keyOrDefault "\U00000061" "\141" }}"#,
    );
    assert_eq!(rows[0].source_key, "feature.x");
    assert_eq!(rows[1].source_key, "a");
    assert_eq!(rows[1].metadata.default_value.as_deref(), Some("a"));
}
#[test]
fn template_comment_quotes_do_not_consume_following_actions() {
    for comment in [
        r#"{{/* document "quoted value */}}"#,
        r#"{{- /* ` unclosed " */ -}}"#,
    ] {
        let rows = facts("gotemplate", &format!("{comment}\n{{{{ key \"flag\" }}}}"));
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].source_key, "flag");
    }
}
#[test]
fn shell_assignments_keep_previously_enabled_export_attributes() {
    for source in [
        "export FLAG; FLAG=true; echo $FLAG",
        "export FLAG=old; FLAG=true; echo $FLAG",
    ] {
        let rows = facts("bash", source);
        assert!(rows.iter().any(|r| r.edge_kind == "defines_config"
            && r.metadata.default_value.as_deref() == Some("true")));
    }
    for source in [
        "export FLAG; export -n FLAG; FLAG=true",
        "if test -f local; then export FLAG; fi; FLAG=true",
    ] {
        assert!(
            !facts("bash", source)
                .iter()
                .any(|r| r.metadata.default_value.as_deref() == Some("true"))
        );
    }
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
fn properties_non_java_whitespace_remains_key_and_value_content() {
    let rows = facts(
        "properties",
        "\u{a0}feature\u{a0}name=\u{a0}value\n\tother\u{c}=\ttrue\n",
    );
    assert_eq!(rows[0].source_key, "\u{a0}feature\u{a0}name");
    assert_eq!(
        rows[0].metadata.default_value.as_deref(),
        Some("\u{a0}value")
    );
    assert_eq!(rows[1].source_key, "other");
    assert_eq!(rows[1].metadata.default_value.as_deref(), Some("true"));
    let rows = facts("properties", "\u{a0}# @config domain=fake\nflag=true\n");
    assert!(rows.iter().all(|row| row.metadata.domain.is_none()));
}
#[test]
fn shell_tilde_expansion_defaults_remain_unknown() {
    let rows = facts(
        "bash",
        "export A=~/cache; export B=first:~user/cache; export C='~/cache'; export D=literal~suffix",
    );
    for key in ["A", "B"] {
        assert!(
            rows.iter()
                .any(|r| r.source_key == key && r.metadata.default_value.is_none())
        );
    }
    assert!(
        rows.iter()
            .any(|r| r.source_key == "C" && r.metadata.default_value.as_deref() == Some("~/cache"))
    );
    assert!(
        rows.iter().any(|r| r.source_key == "D"
            && r.metadata.default_value.as_deref() == Some("literal~suffix"))
    );
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
fn shell_prior_assignment_budget_exhaustion_is_explicit() {
    let source = format!("FLAG=value; {}export FLAG", "echo ignored; ".repeat(1100));
    let error = extract(&FeatureFlagFileInput {
        repository_id: "repo",
        source_scope: "scope",
        file_id: "file",
        path: "config.sh",
        language_id: "bash",
        content: &source,
        config_facts: &[],
    })
    .unwrap_err();
    assert!(
        error
            .to_string()
            .contains("prior assignment analysis incomplete")
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
