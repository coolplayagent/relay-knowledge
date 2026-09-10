use super::*;
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
fn template_output_and_ini_bare_lines_are_not_configuration_definitions() {
    for language in ["ini", "gotemplate"] {
        let rows = facts(language, "server {\n}\nbare\nvalid = true\nother: false\n");
        let keys = rows
            .iter()
            .map(|r| r.source_key.as_str())
            .collect::<Vec<_>>();
        assert_eq!(keys, vec!["valid", "other"]);
    }
    let rows = facts("properties", "bare\nspace value\n");
    assert_eq!(rows[0].metadata.default_value.as_deref(), Some(""));
    assert_eq!(rows[1].metadata.default_value.as_deref(), Some("value"));
}

#[test]
fn template_key_or_default_retains_literal_fallback_and_type() {
    let rows = facts(
        "gotemplate",
        r#"{{ keyOrDefault "feature_x" "off" }}
        {{ keyOrDefault "enabled" "true" }}
        {{ keyOrDefault "dynamic" $fallback }}
        {{ key "without" }}"#,
    );
    assert_eq!(rows[0].metadata.default_value.as_deref(), Some("off"));
    assert_eq!(rows[0].metadata.value_type.as_deref(), Some("string"));
    assert_eq!(rows[1].metadata.default_value.as_deref(), Some("true"));
    assert_eq!(rows[1].metadata.value_type.as_deref(), Some("boolean"));
    assert!(rows[2..].iter().all(|r| r.metadata.default_value.is_none()));
}

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
fn wildcard_receiver_imports_respect_explicit_and_local_type_precedence() {
    for (imports, declaration, expected) in [
        ("import demo.config.*;", "", "demo.config.Config.getX"),
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
fn properties_defaults_keep_trailing_whitespace_and_continuations() {
    let rows = facts("properties", "mode=on \ncontinued=on\\\n  value \t\n");
    assert_eq!(rows[0].metadata.default_value.as_deref(), Some("on "));
    assert_eq!(
        rows[1].metadata.default_value.as_deref(),
        Some("onvalue \t")
    );
}

#[test]
fn dense_configuration_files_fail_at_the_shared_fact_budget() {
    for (language, line) in [
        ("properties", "x=y\n"),
        ("ini", "x=y\n"),
        ("gotemplate", "{{ env \"X\" }}\n"),
        ("bash", "export X=y\n"),
    ] {
        let source = line.repeat(10_001);
        let result = extract(&FeatureFlagFileInput {
            repository_id: "repo",
            source_scope: "scope",
            file_id: "file",
            path: "config",
            language_id: language,
            content: &source,
            config_facts: &[],
        });
        let error = result
            .map(|_| ())
            .expect_err("dense file must hit the fact budget");
        assert!(
            error.to_string().contains("file fact budget exceeded"),
            "{language}: {error}"
        );
    }
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
fn conditional_deferred_and_subshell_exports_do_not_define_parent_configuration() {
    for declaration in [
        "(export FLAG=true)",
        "f() { export FLAG=true; }",
        "if test x; then export FLAG=true; fi",
        "FLAG=true; if test x; then export FLAG; fi",
        "set -a; (FLAG=true)",
    ] {
        let rows = facts("bash", &format!("{declaration}\necho \"$FLAG\""));
        assert!(
            rows.iter().all(|r| r.edge_kind != "defines_config"),
            "{declaration}: {rows:?}"
        );
    }
    assert!(
        facts("bash", "{ export FLAG=true; }; echo \"$FLAG\"")
            .iter()
            .any(|r| r.edge_kind == "defines_config")
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
fn configuration_ranges_exclude_terminal_newlines() {
    for newline in ["\n", "\r\n"] {
        for language in ["properties", "ini", "gotemplate"] {
            let single = facts(language, &format!("feature=true{newline}"));
            assert_eq!(single[0].line_range.start, 1);
            assert_eq!(single[0].line_range.end, 1);
            assert_eq!(single[0].byte_range.end, 12);
        }
        let continued = facts("properties", &format!("feature=tr\\{newline}ue{newline}"));
        assert_eq!(continued[0].line_range.end, 2);
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

#[test]
fn java_sdk_flags_survive_registry_extraction() {
    let rows = crate::code::feature_flags::extract_feature_flags(FeatureFlagFileInput {
        repository_id: "repo",
        source_scope: "scope",
        file_id: "file",
        path: "App.java",
        language_id: "java",
        content: r#"class App { void run() {
          var client = OpenFeature.getClient();
          if (client.getBooleanValue("sdk_checkout", false)) {}
          ldClient.variation("sdk_payment", false);
          unleash.isEnabled("sdk_orders");
          System.getProperty("local_setting");
        }}"#,
        config_facts: &[],
    })
    .unwrap();
    for key in ["sdk_checkout", "sdk_payment", "sdk_orders"] {
        assert!(
            rows.iter().any(|r| r.source_kind == "sdk_flag_key"
                && r.source_key == key
                && r.metadata.source_format == "java"),
            "{rows:?}"
        );
    }
    assert_eq!(
        rows.iter()
            .filter(|r| r.source_key == "local_setting" && r.edge_kind == "reads_config")
            .count(),
        1
    );
}
#[test]
fn properties_escapes_do_not_corrupt_ini_or_template_defaults() {
    for language in ["ini", "gotemplate"] {
        let rows = facts(language, r"directory=C:\temp\files");
        assert_eq!(
            rows[0].metadata.default_value.as_deref(),
            Some(r"C:\temp\files")
        );
    }
    assert_eq!(
        facts("properties", r"directory=C:\temp")[0]
            .metadata
            .default_value
            .as_deref(),
        Some("C:\temp")
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
fn later_export_retains_only_the_latest_unconditional_shell_assignment() {
    let rows = facts(
        "bash",
        r#"FLAG=no; FLAG=yes; export FLAG; echo "$FLAG"
      OTHER=old; if test -f marker; then OTHER=new; fi; export OTHER
      GONE=old; unset GONE; export GONE
    "#,
    );
    let definitions = rows
        .iter()
        .filter(|r| r.edge_kind == "defines_config")
        .collect::<Vec<_>>();
    assert_eq!(definitions.len(), 1, "{rows:?}");
    assert_eq!(definitions[0].source_key, "FLAG");
    assert_eq!(
        definitions[0].metadata.default_value.as_deref(),
        Some("yes")
    );
    assert!(
        rows.iter()
            .any(|r| r.source_key == "FLAG" && r.edge_kind == "reads_config")
    );
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
fn allexport_enables_definitions_and_survives_disable_for_existing_exports() {
    for enable in ["set -a", "set -o allexport"] {
        let code =
            format!("{enable}\nFLAG=yes\nset +a\necho \"$FLAG\"\nLOCAL=no\necho \"$LOCAL\"\n");
        let rows = facts("bash", &code);
        assert!(
            rows.iter()
                .any(|r| r.source_key == "FLAG" && r.edge_kind == "defines_config"),
            "{rows:?}"
        );
        assert!(
            rows.iter()
                .any(|r| r.source_key == "FLAG" && r.edge_kind == "reads_config")
        );
        assert!(!rows.iter().any(|r| r.source_key == "LOCAL"));
    }
    let rows = facts("bash", "(set -a)\nLOCAL=no\necho \"$LOCAL\"\n");
    assert!(!rows.iter().any(|r| r.source_key == "LOCAL"));
}
