use super::super::test_support::partitioned_store;
use super::{by_scope, latest};

#[tokio::test]
async fn empty_checkpoint_routes_fall_back_without_creating_a_shard() {
    let store = partitioned_store("checkpoint-fallback");

    assert!(
        by_scope(&store, "scope-missing".to_owned())
            .await
            .expect("scope checkpoint lookup should succeed")
            .is_none()
    );
    assert!(
        latest(&store, "repo-missing".to_owned())
            .await
            .expect("repository checkpoint lookup should succeed")
            .is_none()
    );
}
