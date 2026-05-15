use super::code_tests::snapshot_with_chunk;
use super::*;
use crate::{
    domain::{
        CodeQueryKind, CodeRepositoryRegistration, CodeRepositorySelector, CodeRetrievalLayer,
        FreshnessPolicy,
    },
    storage::SqliteGraphStore,
};

#[tokio::test]
async fn stores_code_repository_and_queries_fallback_chunks() {
    let store = SqliteGraphStore::open_in_memory().expect("store should open");
    let registration =
        CodeRepositoryRegistration::new("repo", "fixture", "/tmp/repo", Vec::new(), Vec::new())
            .expect("registration should validate");
    store
        .upsert_code_repository(registration)
        .await
        .expect("repository should persist");
    let snapshot = snapshot_with_chunk("repo", "src/lib.rs", "fn retry_policy() {}");
    store
        .apply_code_index_snapshot(snapshot)
        .await
        .expect("snapshot should apply");
    let selector = CodeRepositorySelector::new("fixture", "commit", Vec::new(), Vec::new())
        .expect("selector should validate");

    let hits = store
        .search_code(
            crate::domain::CodeRetrievalRequest::new(
                "retry_policy",
                selector,
                CodeQueryKind::Hybrid,
                5,
                FreshnessPolicy::AllowStale,
            )
            .expect("request should validate"),
        )
        .await
        .expect("query should succeed");

    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].path, "src/lib.rs");
    assert_eq!(hits[0].resolved_commit_sha, "commit");
    assert!(
        !hits[0]
            .retrieval_layers
            .contains(&CodeRetrievalLayer::TextFallback)
    );
}

#[tokio::test]
async fn repository_id_lookup_takes_precedence_over_alias_like_ids() {
    let store = SqliteGraphStore::open_in_memory().expect("store should open");
    store
        .upsert_code_repository(
            CodeRepositoryRegistration::new(
                "repo:first",
                "first",
                "/tmp/first",
                Vec::new(),
                Vec::new(),
            )
            .expect("first registration should validate"),
        )
        .await
        .expect("first repository should persist");
    store
        .upsert_code_repository(
            CodeRepositoryRegistration::new(
                "repo:second",
                "repo:first",
                "/tmp/second",
                Vec::new(),
                Vec::new(),
            )
            .expect("second registration should validate"),
        )
        .await
        .expect("second repository should persist");

    let status = store
        .code_repository_status("repo:first".to_owned())
        .await
        .expect("status should query")
        .expect("repository id should resolve");

    assert_eq!(status.repository_id, "repo:first");
    assert_eq!(status.alias, "first");
}

#[tokio::test]
async fn repo_prefixed_alias_resolves_when_repository_id_is_absent() {
    let store = SqliteGraphStore::open_in_memory().expect("store should open");
    store
        .upsert_code_repository(
            CodeRepositoryRegistration::new(
                "repo:actual",
                "repo:team-a",
                "/tmp/actual",
                Vec::new(),
                Vec::new(),
            )
            .expect("registration should validate"),
        )
        .await
        .expect("repository should persist");

    let status = store
        .code_repository_status("repo:team-a".to_owned())
        .await
        .expect("status should query")
        .expect("repo-prefixed alias should resolve");

    assert_eq!(status.repository_id, "repo:actual");
    assert_eq!(status.alias, "repo:team-a");
}
