use std::sync::Arc;

use super::mirror_status;
use crate::{
    domain::{
        CodeIndexMode, CodeIndexResourceBudget, CodeRepositoryRegistration, code_snapshot_scope_id,
    },
    storage::{CodeIndexTaskSeed, CodeRepositoryStore, SqliteGraphStore},
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

#[tokio::test]
async fn mirrored_same_tree_commits_keep_both_commit_aliases() {
    let control = Arc::new(SqliteGraphStore::open_in_memory().expect("control store should open"));
    let registration =
        CodeRepositoryRegistration::new("repo", "fixture", "/tmp/fixture", Vec::new(), Vec::new())
            .expect("registration should validate");
    let mut status = control
        .upsert_code_repository(registration)
        .await
        .expect("repository should register");
    let source_scope = code_snapshot_scope_id("repo", "same-tree", &[], &[]);
    status.last_indexed_scope_id = Some(source_scope.clone());
    status.last_indexed_commit = Some("commit-a".to_owned());
    status.tree_hash = Some("same-tree".to_owned());
    status.state = "fresh".to_owned();
    status.stale = false;
    mirror_status(&control, status.clone())
        .await
        .expect("first status should mirror");
    status.last_indexed_commit = Some("commit-b".to_owned());
    mirror_status(&control, status)
        .await
        .expect("same-tree status should mirror");

    for commit in ["commit-a", "commit-b"] {
        let scoped = control
            .code_repository_scope_status(
                "fixture".to_owned(),
                commit.to_owned(),
                Vec::new(),
                Vec::new(),
            )
            .await
            .expect("mirrored scope should query")
            .expect("commit alias should resolve");
        assert_eq!(
            scoped.last_indexed_scope_id.as_deref(),
            Some(source_scope.as_str())
        );
        assert_eq!(scoped.last_indexed_commit.as_deref(), Some(commit));
    }
}

#[tokio::test]
async fn partitioned_legacy_same_tree_mirror_preserves_previous_commit_as_incremental_base() {
    let control = Arc::new(SqliteGraphStore::open_in_memory().expect("control store should open"));
    let registration =
        CodeRepositoryRegistration::new("repo", "fixture", "/tmp/fixture", Vec::new(), Vec::new())
            .expect("registration should validate");
    let mut status = control
        .upsert_code_repository(registration)
        .await
        .expect("repository should register");
    let source_scope = code_snapshot_scope_id("repo", "same-tree", &[], &[]);
    let legacy_scope = source_scope.clone();
    control
        .run(move |connection| {
            connection.execute(
                "INSERT INTO code_repository_scopes (
                     source_scope, repository_id, resolved_commit_sha, tree_hash,
                     path_filters_json, language_filters_json, indexed_file_count,
                     symbol_count, reference_count, chunk_count, stale, degraded_reason
                 ) VALUES (?1, 'repo', 'commit-a', 'same-tree', '[]', '[]',
                           0, 0, 0, 0, 0, NULL)",
                [&legacy_scope],
            )?;
            Ok(())
        })
        .await
        .expect("legacy scope should exist without commit aliases");
    status.last_indexed_scope_id = Some(source_scope.clone());
    status.last_indexed_commit = Some("commit-b".to_owned());
    status.tree_hash = Some("same-tree".to_owned());
    status.state = "fresh".to_owned();
    status.stale = false;

    mirror_status(&control, status)
        .await
        .expect("new same-tree commit should mirror");

    let previous = control
        .code_repository_scope_status(
            "fixture".to_owned(),
            "commit-a".to_owned(),
            Vec::new(),
            Vec::new(),
        )
        .await
        .expect("legacy commit alias should query")
        .expect("legacy commit should resolve to shared content");
    assert_eq!(
        previous.last_indexed_scope_id.as_deref(),
        Some(source_scope.as_str())
    );

    let queued = control
        .queue_code_index_task(CodeIndexTaskSeed {
            repository_id: "repo".to_owned(),
            alias: "fixture".to_owned(),
            ref_selector: "commit-c".to_owned(),
            resolved_commit_sha: "commit-c".to_owned(),
            tree_hash: "next-tree".to_owned(),
            source_scope: code_snapshot_scope_id("repo", "next-tree", &[], &[]),
            path_filters: Vec::new(),
            language_filters: Vec::new(),
            mode: CodeIndexMode::incremental("commit-a", "commit-c")
                .expect("incremental mode should validate"),
            input_fingerprint: "legacy-base".to_owned(),
            resource_budget: CodeIndexResourceBudget::default(),
            payload_json: "{}".to_owned(),
            now_ms: 10,
        })
        .await
        .expect("legacy commit alias should remain a valid incremental base");
    assert_eq!(
        queued.source_scope,
        code_snapshot_scope_id("repo", "next-tree", &[], &[])
    );
}
