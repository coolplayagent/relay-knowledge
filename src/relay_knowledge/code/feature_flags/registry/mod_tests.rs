use super::*;
use crate::code::feature_flags::registry::test_support::*;

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
            path: if language == "gotemplate" {
                "config.ctmpl"
            } else {
                "config"
            },
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
fn annotation_blank_lines_are_boundaries_for_all_natural_line_endings() {
    // Exercise natural-line metadata boundaries independently of parser newline rules.
    let annotation = |source: &str| {
        metadata(
            &FeatureFlagFileInput {
                repository_id: "repo",
                source_scope: "scope",
                file_id: "file",
                path: "App.java",
                language_id: "java",
                content: source,
                config_facts: &[],
            },
            source.find("System.getProperty").unwrap(),
        )
    };
    for newline in ["\n", "\r", "\r\n"] {
        for comment in [
            "// @config domain=payments hot-reload=true",
            "/* @config domain=payments hot-reload=true */",
            "/**\n * @config domain=payments hot-reload=true\n */",
        ] {
            for blank in ["", " ", "\t"] {
                let source = format!(
                    "class App {{ void run() {{{newline}{comment}{newline}{blank}{newline}System.getProperty(\"flag\"); }} }}"
                );
                let meta = annotation(&source);
                assert!(
                    meta.domain.is_none() && meta.hot_reload.is_none(),
                    "{source}: {meta:?}"
                );
            }
            let source = format!(
                "class App {{ void run() {{{newline}{comment}{newline}System.getProperty(\"flag\"); }} }}"
            );
            assert!(
                annotation(&source).domain.as_deref() == Some("payments"),
                "{source}"
            );
        }
        for (language, comment, source) in [
            ("properties", "#", "flag=true"),
            ("ini", ";", "flag=true"),
            ("bash", "#", "export FLAG=true"),
            ("gotemplate", "{{/*", "{{ key \"flag\" }}"),
        ] {
            let close = if language == "gotemplate" {
                " */}}"
            } else {
                ""
            };
            let content =
                format!("{comment} @config domain=payments{close}{newline}{newline}{source}");
            assert!(
                facts(language, &content)
                    .iter()
                    .all(|r| r.metadata.domain.is_none())
            );
        }
    }
}

#[test]
fn unicode_domain_annotations_use_unicode_lowercase() {
    let rows = facts("properties", "# @config domain=ÜBER\nflag=true\n");
    assert_eq!(rows[0].metadata.domain.as_deref(), Some("über"));
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
fn sdk_evaluations_reuse_adjacent_annotation_metadata() {
    let content = "class App { void run() {\nvar client = OpenFeature.getClient();\n// @config domain=payments hot-reload=true\nclient.getBooleanValue(\"checkout\", false);\n// @config domain=ignored\n\nclient.getBooleanValue(\"plain\", false);\n}}";
    let rows = crate::code::feature_flags::extract_feature_flags(FeatureFlagFileInput {
        repository_id: "repo",
        source_scope: "scope",
        file_id: "file",
        path: "App.java",
        language_id: "java",
        content,
        config_facts: &[],
    })
    .unwrap();
    let checkout = rows.iter().find(|r| r.source_key == "checkout").unwrap();
    assert_eq!(checkout.metadata.domain.as_deref(), Some("payments"));
    assert_eq!(checkout.metadata.hot_reload, Some(true));
    assert!(
        rows.iter()
            .find(|r| r.source_key == "plain")
            .unwrap()
            .metadata
            .domain
            .is_none()
    );
}

#[test]
fn dotenv_assignments_preserve_nonboolean_and_unknown_definitions() {
    let rows = crate::code::feature_flags::extract_feature_flags(FeatureFlagFileInput {
        repository_id: "repo",
        source_scope: "scope",
        file_id: "file",
        path: ".env",
        language_id: "unknown",
        content: "FEATURE=production\nPORT=8080\nDYNAMIC=${OTHER}\n",
        config_facts: &[],
    })
    .unwrap();
    for key in ["FEATURE", "PORT", "DYNAMIC"] {
        assert!(
            rows.iter().any(|r| r.source_key == key
                && r.source_kind == "env_var"
                && r.edge_kind == "defines_config"),
            "{rows:?}"
        );
    }
    assert_eq!(
        rows.iter()
            .find(|r| r.source_key == "FEATURE")
            .unwrap()
            .metadata
            .default_value
            .as_deref(),
        Some("production")
    );
    assert!(
        rows.iter()
            .find(|r| r.source_key == "DYNAMIC")
            .unwrap()
            .metadata
            .default_value
            .is_none()
    );
}
