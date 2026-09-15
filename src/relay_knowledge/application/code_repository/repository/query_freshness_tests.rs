//! Service regressions separating parser coverage from query read-model failures.

use std::sync::Arc;

use crate::{
    api::CodeRepositoryFreshnessState,
    domain::{
        CodeContentIntegrityState, CodeQueryKind, CodeRetrievalLayer, CodeRetrievalRequest,
        FreshnessPolicy,
    },
    storage::SqliteGraphStore,
};

use super::test_support::{
    FixtureRepo, context, register_fixture_repo, request, selector, service_with_store,
};

#[tokio::test]
async fn read_model_outage_remains_degraded_with_complete_or_partial_content() {
    for partial in [false, true] {
        let repo = FixtureRepo::create("query-freshness-read-model");
        repo.write(
            "src/lib.rs",
            "pub fn foo() -> u32 { 1 }\npub fn caller() -> u32 { foo() }\n",
        );
        if partial {
            repo.write("src/broken.py", "def broken(value):\n    return (\n");
        }
        repo.git(["add", "src"]);
        repo.git(["commit", "-m", "query freshness fixture"]);
        let database_path = repo.path.join("query-test.sqlite3");
        let store = Arc::new(SqliteGraphStore::open(&database_path).unwrap());
        let service = service_with_store(store).await;
        register_fixture_repo(&service, &repo, "query-freshness-register").await;
        service
            .index_code_repository(request("fixture", "HEAD"), context("query-freshness-index"))
            .await
            .unwrap();
        let query = CodeRetrievalRequest::new(
            "foo",
            selector("fixture", "HEAD"),
            CodeQueryKind::References,
            10,
            FreshnessPolicy::AllowStale,
        )
        .unwrap();
        let before = service
            .query_code_repository(query.clone(), context("before-outage"))
            .await
            .unwrap();
        let integrity = if partial {
            CodeContentIntegrityState::Partial
        } else {
            CodeContentIntegrityState::Complete
        };
        assert_eq!(before.freshness.content_integrity.state, integrity);
        assert_eq!(before.freshness.state, CodeRepositoryFreshnessState::Fresh);
        assert!(before.results.iter().all(|hit| !hit.query_degraded));
        assert!(before.results.iter().any(|hit| {
            hit.retrieval_layers
                .contains(&CodeRetrievalLayer::Reference)
        }));

        // Fixture-only corruption isolates a query layer outage after a published index.
        // Production storage remains behind its bounded asynchronous worker boundary.
        rusqlite::Connection::open(&database_path)
            .unwrap()
            .execute_batch("DROP TABLE code_repository_search")
            .unwrap();

        for policy in [FreshnessPolicy::AllowStale, FreshnessPolicy::WaitUntilFresh] {
            let mut query = query.clone();
            query.freshness_policy = policy;
            let response = service
                .query_code_repository(query, context("after-outage"))
                .await
                .unwrap();
            assert!(!response.results.is_empty());
            assert_eq!(
                response.freshness.state,
                CodeRepositoryFreshnessState::Degraded
            );
            assert_eq!(response.freshness.content_integrity.state, integrity);
            assert_eq!(
                response.freshness.content_integrity.degraded_file_count,
                Some(usize::from(partial))
            );
            assert!(
                response
                    .degraded_reason
                    .as_deref()
                    .unwrap()
                    .contains("read model")
            );
        }
    }
}
