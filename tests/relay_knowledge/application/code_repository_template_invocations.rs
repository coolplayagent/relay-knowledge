//! Real Git indexing retains diagnostics for invalid Go named-template actions.
use super::*;

#[tokio::test]
async fn named_template_actions_keep_valid_literals_and_reject_proxy_only_syntax() {
    for (name, action, valid) in [
        ("unquoted", r#"{{ template foo . }}"#, false),
        ("rune", r#"{{ template 'f' . }}"#, false),
        ("missing", "{{ template }}", false),
        ("bad_escape", r#"{{ template "bad\q" . }}"#, false),
        ("define_rune", r#"{{ define 'f' }}body{{ end }}"#, false),
        ("block_rune", r#"{{ block 'f' . }}body{{ end }}"#, false),
        (
            "quoted",
            r#"{{ define "foo" }}body{{ end }}{{ template "foo" . }}"#,
            true,
        ),
        (
            "raw",
            "{{ define `foo` }}body{{ end }}{{ template `foo` . }}",
            true,
        ),
        (
            "escaped",
            r#"{{ define "f\x6fo" }}body{{ end }}{{ template "foo" . }}"#,
            true,
        ),
        (
            "no_pipeline",
            r#"{{ define "foo" }}body{{ end }}{{ template "foo" }}"#,
            true,
        ),
        ("block", r#"{{ block "foo" . }}body{{ end }}"#, true),
    ] {
        let repo = FixtureRepo::create("template-name-literals");
        repo.write(
            "src/config.ctmpl",
            &format!(
                r#"{{{{ key "retained.flag" }}}}
{action}
"#
            ),
        );
        repo.git(["add", "."]);
        repo.git(["commit", "-m", "Named template action syntax"]);
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
                context("index-template-name"),
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
                context("query-template-name"),
            )
            .await
            .unwrap();
        assert_eq!(response.degraded_reason.is_none(), valid, "{name}");
        assert!(
            response
                .flags
                .iter()
                .any(|flag| flag.source_key == "retained.flag"),
            "{name}"
        );
    }
}
