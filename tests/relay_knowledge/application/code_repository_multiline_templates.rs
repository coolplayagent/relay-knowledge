//! Whole template actions retain their persisted read ranges across physical lines.
use super::*;

#[tokio::test]
async fn go_variable_declarations_preserve_freshness_and_literal_config_reads() {
    for (suffix, partial) in [
        (
            "{{ $flag := key \"feature_x\" }}{{ $1bad := key \"digit_name\" }}",
            false,
        ),
        ("{{ $missing }}", true),
        (
            "{{ range $i, $value := .Items }}{{ key \"feature_x\" }}{{ key \"digit_name\" }}{{ end }}",
            false,
        ),
        ("{{ if $i, $value := .Items }}x{{ end }}", true),
        ("{{ range $i, $value, $third := .Items }}x{{ end }}", true),
    ] {
        let repo = FixtureRepo::create("template-variable-declarations");
        repo.git(["config", "core.autocrlf", "false"]);
        let prefix = "{{/* ignored {{ key \"fake\" }}\r\n */}}\r\n{{ keyOrDefault\r\n \"quoted\" \"a}}b\\\"c\" }}\r\n{{ keyOrDefault\r\n \"raw\" `x}}y\r\nz` }}\r\n";
        repo.write("src/config.ctmpl", &format!("{prefix}{suffix}"));
        repo.git(["add", "."]);
        repo.git(["commit", "-m", "Go template variable proof"]);
        let service = service_with_memory_store().await;
        register_fixture_repo(&service, &repo, "fixture").await;
        service
            .index_code_repository(
                CodeIndexRequest {
                    repository: selector("fixture", "HEAD"),
                    mode: CodeIndexMode::Full,
                    workspace_detection: Default::default(),
                    freshness_policy: FreshnessPolicy::WaitUntilFresh,
                    reuse_historical: false,
                },
                context("index-template-variables"),
            )
            .await
            .unwrap();
        let response = service
            .query_code_repository_feature_flags(
                CodeFeatureFlagRequest::new(
                    None,
                    selector("fixture", "HEAD"),
                    20,
                    FreshnessPolicy::AllowStale,
                )
                .unwrap(),
                context("query-template-variables"),
            )
            .await
            .unwrap();
        assert_eq!(response.degraded_reason.is_some(), partial);
        if !partial {
            for key in ["feature_x", "digit_name"] {
                let flag = response.flags.iter().find(|f| f.source_key == key).unwrap();
                assert!(flag.usages.iter().any(|u| u.edge_kind == "reads_config"));
            }
        }
        let quoted = response
            .flags
            .iter()
            .find(|f| f.source_key == "quoted")
            .unwrap();
        assert!(
            quoted
                .usages
                .iter()
                .any(|u| u.metadata.default_value.as_deref() == Some("a}}b\"c"))
        );
    }
}

#[tokio::test]
async fn template_action_boundaries_and_pipeline_reads_survive_real_git_indexing() {
    let repo = FixtureRepo::create("template-pipelines");
    repo.write("src/pipes.ctmpl", "{{ \"piped_key\" | key }}\n{{ \"PIPED_ENV\" | env }}\n{{ \"true\" | keyOrDefault \"piped_default\" }}\n");
    repo.write("src/config.ctmpl", "REAL=true\n{{/*\nFAKE_COMMENT=true\nkey \"COMMENT_CALL\"\n*/}}\n{{ printf `%s` `\nFAKE_RAW=true\nkey \"RAW_CALL\"\n` }}\n{{ with key \"feature_x\" }}ok{{ end }}\n{{ if env \"FEATURE_X\" }}yes{{ end }}\n{{ printf `%s` (keyOrDefault \"nested\" \"true\") }}\n{{ $name := \"dynamic\" }}{{ key $name }}\n{{ printf `%s/%s` (key \"same\") (key \"same\") }}\n");
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "Template pipelines and action masking"]);
    let service = service_with_memory_store().await;
    register_fixture_repo(&service, &repo, "fixture").await;
    service
        .index_code_repository(
            CodeIndexRequest {
                repository: selector("fixture", "HEAD"),
                mode: CodeIndexMode::Full,
                workspace_detection: Default::default(),
                freshness_policy: FreshnessPolicy::WaitUntilFresh,
                reuse_historical: false,
            },
            context("index-template-pipeline"),
        )
        .await
        .unwrap();
    let response = service
        .query_code_repository_feature_flags(
            CodeFeatureFlagRequest::new(
                None,
                selector("fixture", "HEAD"),
                20,
                FreshnessPolicy::WaitUntilFresh,
            )
            .unwrap(),
            context("template-pipeline"),
        )
        .await
        .unwrap();
    assert_eq!(response.flags.len(), 8, "{:?}", response.flags);
    for (key, kind, edge, count) in [
        ("REAL", "config_key", "defines_config", 1),
        ("feature_x", "config_key", "reads_config", 1),
        ("FEATURE_X", "env_var", "reads_config", 1),
        ("nested", "config_key", "reads_config", 1),
        ("same", "config_key", "reads_config", 2),
        ("piped_key", "config_key", "reads_config", 1),
        ("PIPED_ENV", "env_var", "reads_config", 1),
        ("piped_default", "config_key", "reads_config", 1),
    ] {
        let flag = response.flags.iter().find(|f| f.source_key == key).unwrap();
        assert_eq!(flag.source_kind, kind);
        assert_eq!(
            flag.usages.iter().filter(|u| u.edge_kind == edge).count(),
            count
        );
        if matches!(key, "nested" | "piped_default") {
            assert_eq!(
                flag.usages[0].metadata.default_value.as_deref(),
                Some("true")
            );
        }
    }
}

#[tokio::test]
async fn multiline_template_reads_round_trip_through_git_indexing_with_exact_ranges() {
    let repo = FixtureRepo::create("multiline-template");
    repo.git(["config", "core.autocrlf", "false"]);
    let content = "# 妯℃澘\r\n  {{- key\r\n \"feature_x\" -}}\r\n{{ keyOrDefault\n \"feature_y\"\n \"true\" }}\n{{ env\n `ENV_SWITCH` }}\n{{/* {{ key \"false_comment\" }} */}}\n";
    repo.write("src/config.ctmpl", content);
    repo.write(
        "src/literals.ctmpl",
        "{{ keyOrDefault \"hex\" \"\\x74rue\" }}\n{{ keyOrDefault \"octal\" \"\\164rue\" }}\n{{ keyOrDefault \"raw_cr\" `first\rsecond` }}\n{{ keyOrDefault \"non_utf8\" \"\\xff\" }}\n",
    );
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "Multiline template actions"]);
    let service = service_with_memory_store().await;
    register_fixture_repo(&service, &repo, "fixture").await;
    service
        .index_code_repository(
            CodeIndexRequest {
                repository: selector("fixture", "HEAD"),
                mode: CodeIndexMode::Full,
                workspace_detection: Default::default(),
                freshness_policy: FreshnessPolicy::WaitUntilFresh,
                reuse_historical: false,
            },
            context("index-multiline-template"),
        )
        .await
        .unwrap();
    let response = service
        .query_code_repository_feature_flags(
            CodeFeatureFlagRequest::new(
                None,
                selector("fixture", "HEAD"),
                20,
                FreshnessPolicy::WaitUntilFresh,
            )
            .unwrap(),
            context("query-multiline-template"),
        )
        .await
        .unwrap();
    assert_eq!(response.flags.len(), 7, "{:?}", response.flags);
    for (key, expected) in [
        ("hex", Some("true")),
        ("octal", Some("true")),
        ("raw_cr", Some("firstsecond")),
        ("non_utf8", None),
    ] {
        let flag = response.flags.iter().find(|f| f.source_key == key).unwrap();
        assert_eq!(flag.usages[0].metadata.default_value.as_deref(), expected);
    }
    for (key, kind, lines) in [
        ("feature_x", "config_key", (2, 3)),
        ("feature_y", "config_key", (4, 6)),
        ("ENV_SWITCH", "env_var", (7, 8)),
    ] {
        let flag = response.flags.iter().find(|f| f.source_key == key).unwrap();
        assert_eq!(flag.source_kind, kind);
        let usage = flag
            .usages
            .iter()
            .find(|u| u.edge_kind == "reads_config")
            .unwrap();
        assert_eq!((usage.line_range.start, usage.line_range.end), lines);
        assert_eq!(usage.metadata.source_format, "ctmpl");
        let start = usize::try_from(usage.byte_range.start).unwrap();
        let end = usize::try_from(usage.byte_range.end).unwrap();
        assert!(content[start..end].starts_with("{{"));
        assert!(content[start..end].ends_with("}}"));
        if key == "feature_y" {
            assert_eq!(usage.metadata.default_value.as_deref(), Some("true"));
            assert_eq!(usage.metadata.value_type.as_deref(), Some("boolean"));
        }
    }
}

#[tokio::test]
async fn parenthesized_template_literals_and_export_defaults_survive_git_indexing() {
    let repo = FixtureRepo::create("template-recovery");
    repo.git(["config", "core.autocrlf", "false"]);
    repo.write("src/config.ctmpl", "{{/* ignored {{ key \"fake\" }}\r\n */}}\r\n{{ (\"paren_key\") | key }}\r\n{{ ((\"PAREN_ENV\")) | env }}\r\n{{ keyOrDefault \"quoted\" \"a}}b\" }}\r\n{{ keyOrDefault \"raw\" `x}}y\r\nz` }}\r\n");
    repo.write(
        "src/defaults.sh",
        "export EXPORTED=tr\"u\"'e'\nprintf '%s' \"$EXPORTED\"\n",
    );
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "Literal-aware template recovery"]);
    let service = service_with_memory_store().await;
    register_fixture_repo(&service, &repo, "fixture").await;
    service
        .index_code_repository(
            CodeIndexRequest {
                repository: selector("fixture", "HEAD"),
                mode: CodeIndexMode::Full,
                workspace_detection: Default::default(),
                freshness_policy: FreshnessPolicy::WaitUntilFresh,
                reuse_historical: false,
            },
            context("index-template-recovery"),
        )
        .await
        .unwrap();
    let response = service
        .query_code_repository_feature_flags(
            CodeFeatureFlagRequest::new(
                None,
                selector("fixture", "HEAD"),
                20,
                FreshnessPolicy::WaitUntilFresh,
            )
            .unwrap(),
            context("query-template-recovery"),
        )
        .await
        .unwrap();
    assert!(
        response.degraded_reason.is_none(),
        "{:?}",
        response.degraded_reason
    );
    assert_eq!(response.flags.len(), 5, "{:?}", response.flags);
    for (key, default) in [
        ("paren_key", None),
        ("PAREN_ENV", None),
        ("quoted", Some("a}}b")),
        ("raw", Some("x}}y\nz")),
        ("EXPORTED", Some("true")),
    ] {
        let flag = response
            .flags
            .iter()
            .find(|flag| flag.source_key == key)
            .unwrap();
        assert_eq!(
            flag.source_kind,
            if matches!(key, "PAREN_ENV" | "EXPORTED") {
                "env_var"
            } else {
                "config_key"
            }
        );
        if let Some(default) = default {
            assert!(
                flag.usages
                    .iter()
                    .any(|usage| usage.metadata.default_value.as_deref() == Some(default)),
                "{:?}",
                flag.usages
            );
        }
    }
}

#[tokio::test]
async fn numeric_template_diagnostics_and_tilde_defaults_survive_git_indexing() {
    let repo = FixtureRepo::create("numeric-template-tilde");
    repo.git(["config", "core.autocrlf", "false"]);
    repo.write("src/bad.ctmpl", "{{/* ignored {{ key \"fake\" }}\r\n */}}\r\n{{ keyOrDefault\r\n \"quoted\" \"a}}b\\\"c\" }}\r\n{{ keyOrDefault\r\n \"raw\" `x}}y\r\nz` }}\r\n{{ 123abc }}\r\n{{ 18446744073709551616 }}\r\n{{ 0x10000000000000000 }}\r\n");
    repo.write(
        "src/defaults.sh",
        "export FLAG=~\nexport PATH_FLAG=~/path\nexport QUOTED=\"~\"\n",
    );
    repo.git(["add", "."]);
    repo.git([
        "commit",
        "-m",
        "Malformed numeric action and tilde defaults",
    ]);
    let service = service_with_memory_store().await;
    register_fixture_repo(&service, &repo, "fixture").await;
    service
        .index_code_repository(
            CodeIndexRequest {
                repository: selector("fixture", "HEAD"),
                mode: CodeIndexMode::Full,
                workspace_detection: Default::default(),
                freshness_policy: FreshnessPolicy::WaitUntilFresh,
                reuse_historical: false,
            },
            context("index-numeric-tilde"),
        )
        .await
        .unwrap();
    let response = service
        .query_code_repository_feature_flags(
            CodeFeatureFlagRequest::new(
                None,
                selector("fixture", "HEAD"),
                20,
                FreshnessPolicy::AllowStale,
            )
            .unwrap(),
            context("query-numeric-tilde"),
        )
        .await
        .unwrap();
    assert!(
        response.degraded_reason.is_some(),
        "malformed numeric template retains diagnostic"
    );
    for (key, expected) in [("FLAG", None), ("PATH_FLAG", None), ("QUOTED", Some("~"))] {
        let flag = response
            .flags
            .iter()
            .find(|flag| flag.source_key == key)
            .unwrap();
        let definition = flag
            .usages
            .iter()
            .find(|usage| usage.edge_kind == "defines_config")
            .unwrap();
        assert_eq!(definition.metadata.default_value.as_deref(), expected);
        assert_eq!(
            definition.metadata.value_type.as_deref(),
            expected.map(|_| "string")
        );
    }
}

#[tokio::test]
async fn mixed_script_config_keys_keep_distinct_display_names_after_git_indexing() {
    let repo = FixtureRepo::create("mixed-script-config-display");
    repo.write("src/App.java", "class App { void read(String dynamic) { java.lang.System.getProperty(\"功能.flag\"); java.lang.System.getProperty(\"flag.功能\"); java.lang.System.getProperty(\"flag\"); java.lang.System.getProperty(\"功能\"); java.lang.System.getProperty(dynamic); } }");
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "Mixed script configuration names"]);
    let service = service_with_memory_store().await;
    register_fixture_repo(&service, &repo, "fixture").await;
    service
        .index_code_repository(
            CodeIndexRequest {
                repository: selector("fixture", "HEAD"),
                mode: CodeIndexMode::Full,
                workspace_detection: Default::default(),
                freshness_policy: FreshnessPolicy::WaitUntilFresh,
                reuse_historical: false,
            },
            context("index-mixed-script-names"),
        )
        .await
        .unwrap();
    let response = service
        .query_code_repository_feature_flags(
            CodeFeatureFlagRequest::new(
                None,
                selector("fixture", "HEAD"),
                20,
                FreshnessPolicy::WaitUntilFresh,
            )
            .unwrap(),
            context("query-mixed-script-names"),
        )
        .await
        .unwrap();
    assert!(response.degraded_reason.is_none());
    assert_eq!(response.flags.len(), 4);
    for (key, name) in [
        ("功能.flag", "功能_flag"),
        ("flag.功能", "flag_功能"),
        ("flag", "flag"),
        ("功能", "功能"),
    ] {
        let flag = response
            .flags
            .iter()
            .find(|flag| flag.source_key == key)
            .unwrap();
        assert_eq!(flag.name, name);
        assert!(
            flag.usages
                .iter()
                .any(|usage| usage.edge_kind == "reads_config")
        );
    }
}
