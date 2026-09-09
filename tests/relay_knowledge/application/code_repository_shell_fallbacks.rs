//! Shell static fallback values retain runtime quote semantics in indexed metadata.
use super::*;
use relay_knowledge::domain::CodeFeatureFlagRequest;
#[tokio::test]
async fn shell_fallback_quoting_round_trips_without_false_default_conflicts() {
    let repo = FixtureRepo::create("shell-fallbacks");
    repo.write(
        "src/defaults.sh",
        r#"echo ${FLAG:-"true"}
echo ${FLAG:-true}
echo ${EMPTY:-''}
echo "${OUTER:-''}"
echo ${CONCAT:-tr"u"'e'}
echo ${DYNAMIC:-"$OTHER"}
"#,
    );
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "Static shell quote defaults"]);
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
            context("index-shell-defaults"),
        )
        .await
        .unwrap();
    let mut request = CodeFeatureFlagRequest::new(
        None,
        selector("fixture", "HEAD"),
        20,
        FreshnessPolicy::WaitUntilFresh,
    )
    .unwrap();
    request.consistency = true;
    let response = service
        .query_code_repository_feature_flags(request, context("query-shell-defaults"))
        .await
        .unwrap();
    for (key, default, kind) in [
        ("FLAG", Some("true"), Some("boolean")),
        ("EMPTY", Some(""), Some("string")),
        ("OUTER", Some("''"), Some("string")),
        ("CONCAT", Some("true"), Some("boolean")),
        ("DYNAMIC", None, None),
    ] {
        let flag = response
            .flags
            .iter()
            .find(|flag| flag.source_key == key)
            .unwrap();
        for usage in &flag.usages {
            assert_eq!(usage.metadata.default_value.as_deref(), default, "{key}");
            assert_eq!(usage.metadata.value_type.as_deref(), kind, "{key}");
        }
        assert!(
            !flag
                .consistency_diagnostics
                .iter()
                .any(|diagnostic| diagnostic.contains("conflicting_defaults")),
            "{flag:?}"
        );
    }
}
