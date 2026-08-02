use std::sync::Arc;

use super::mirror_status;
use crate::{
    domain::CodeRepositoryRegistration,
    storage::{CodeRepositoryStore, SqliteGraphStore},
};

#[tokio::test]
async fn status_without_an_index_scope_is_a_noop_mirror() {
    let control = Arc::new(SqliteGraphStore::open_in_memory().expect("control store should open"));
    let registration =
        CodeRepositoryRegistration::new("repo", "fixture", "/tmp/fixture", Vec::new(), Vec::new())
            .expect("registration should validate");
    let status = control
        .upsert_code_repository(registration)
        .await
        .expect("repository should register");

    mirror_status(&control, status)
        .await
        .expect("status without a scope should not write a mirrored scope");
    assert!(
        control
            .latest_code_index_checkpoint("repo".to_owned())
            .await
            .expect("checkpoint lookup should succeed")
            .is_none()
    );
}
