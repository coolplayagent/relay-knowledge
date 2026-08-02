use super::super::test_support::partitioned_store;
use super::{candidate_paths_for_scope, fingerprints};

#[tokio::test]
async fn empty_file_index_routes_return_bounded_empty_results() {
    let store = partitioned_store("file-index-fallback");

    assert!(
        fingerprints(&store, "repo-missing".to_owned())
            .await
            .expect("fingerprint lookup should succeed")
            .is_empty()
    );
    assert!(
        candidate_paths_for_scope(
            &store,
            "scope-missing".to_owned(),
            Vec::new(),
            Vec::new(),
            false,
            1,
        )
        .await
        .expect("candidate lookup should succeed")
        .is_empty()
    );
}
