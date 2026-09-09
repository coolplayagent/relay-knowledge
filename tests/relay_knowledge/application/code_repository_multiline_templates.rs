//! Whole template actions retain their persisted read ranges across physical lines.
use super::*;

#[tokio::test]
async fn multiline_template_reads_round_trip_through_git_indexing_with_exact_ranges() {
    let repo = FixtureRepo::create("multiline-template");
    repo.git(["config", "core.autocrlf", "false"]);
    let content = "# 模板\r\n  {{- key\r\n \"feature_x\" -}}\r\n{{ keyOrDefault\n \"feature_y\"\n \"true\" }}\n{{ env\n `ENV_SWITCH` }}\n{{/* {{ key \"false_comment\" }} */}}\n";
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
