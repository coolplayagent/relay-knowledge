//! Java identifier categories must not invalidate complete configuration namespaces.
use super::*;

#[tokio::test]
async fn java_unicode_identifiers_keep_configuration_reads_and_original_type_names() {
    let repo = FixtureRepo::create("java-unicode-identifiers");
    for (case, name) in [
        ("currency", "€uro"),
        ("pound", "£ound"),
        ("connector", "‿name"),
        ("combining", "e\u{301}"),
        ("supplementary", "\u{10400}name"),
    ] {
        repo.write(&format!("src/{case}/App.java"), &format!("package demo.{name}; class App {{ boolean run() {{ return Boolean.getBoolean(\"{case}.flag\"); }} }} class {name} {{}}"));
    }
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "Java Unicode namespaces"]);
    let service = service_with_memory_store().await;
    register_complete_java_fixture_repo(&service, &repo, "register-unicode-identifiers").await;
    let indexed = service
        .index_code_repository(
            CodeIndexRequest {
                repository: selector("fixture", "HEAD"),
                mode: CodeIndexMode::Full,
                workspace_detection: Default::default(),
                freshness_policy: FreshnessPolicy::WaitUntilFresh,
                reuse_historical: false,
            },
            context("index-unicode-identifiers"),
        )
        .await
        .unwrap();
    assert_eq!(indexed.summary.degraded_file_count, 0);
    let response = service
        .query_code_repository_feature_flags(
            CodeFeatureFlagRequest::new(
                None,
                filtered_selector("fixture", "HEAD", "src"),
                50,
                FreshnessPolicy::WaitUntilFresh,
            )
            .unwrap(),
            context("query-unicode-identifiers"),
        )
        .await
        .unwrap();
    assert!(response.degraded_reason.is_none());
    let keys = response
        .flags
        .iter()
        .map(|flag| flag.source_key.as_str())
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(
        keys,
        [
            "currency.flag",
            "pound.flag",
            "connector.flag",
            "combining.flag",
            "supplementary.flag"
        ]
        .into_iter()
        .collect()
    );
    let definitions = query(&service, "€uro", CodeQueryKind::Definition).await;
    assert!(definitions.results.iter().any(|hit| {
        hit.retrieval_layers.contains(&CodeRetrievalLayer::Symbol)
            && hit
                .canonical_symbol_id
                .as_deref()
                .is_some_and(|id| id.ends_with("€uro"))
    }));
}
