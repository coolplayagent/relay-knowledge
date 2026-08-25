use super::super::test_support::partitioned_store;
use super::{candidate_paths_for_scope, fingerprints};

#[tokio::test]
async fn missing_repository_fingerprints_are_empty_but_missing_scope_is_rejected() {
    let store = partitioned_store("file-index-fallback");

    assert!(
        fingerprints(&store, "repo-missing".to_owned())
            .await
            .expect("fingerprint lookup should succeed")
            .is_empty()
    );
    let error = candidate_paths_for_scope(
        &store,
        "scope-missing".to_owned(),
        Vec::new(),
        Vec::new(),
        false,
        1,
    )
    .await
    .expect_err("an explicit missing scope must not be treated as an empty authorized scope");
    assert!(
        error
            .to_string()
            .contains("scope 'scope-missing' is unavailable")
    );
}
