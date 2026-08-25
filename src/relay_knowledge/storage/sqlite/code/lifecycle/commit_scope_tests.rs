//! Direct commit-scope owner coverage through checkpointed publication.

use crate::{
    domain::{
        CodeIndexResourceBudget, CodeIndexSession, CodeRepositoryRegistration,
        code_snapshot_scope_id,
    },
    storage::{CodeRepositoryStore, SqliteGraphStore},
};

#[tokio::test]
async fn checkpointed_same_tree_publication_keeps_previous_commit_alias() {
    let store = SqliteGraphStore::open_in_memory().expect("store should open");
    store
        .upsert_code_repository(
            CodeRepositoryRegistration::new("repo", "fixture", "/tmp/repo", Vec::new(), Vec::new())
                .expect("registration should validate"),
        )
        .await
        .expect("repository should register");
    let source_scope = code_snapshot_scope_id("repo", "same-tree", &[], &[]);
    for commit in ["commit-a", "commit-b"] {
        let session = CodeIndexSession {
            repository_id: "repo".to_owned(),
            source_scope: source_scope.clone(),
            base_resolved_commit_sha: None,
            resolved_commit_sha: commit.to_owned(),
            tree_hash: "same-tree".to_owned(),
            path_filters: Vec::new(),
            language_filters: Vec::new(),
            full_replace: true,
            total_path_count: 0,
            changed_path_count: 0,
            skipped_unchanged_count: 0,
            deleted_paths: Vec::new(),
            changed_paths: Vec::new(),
            tombstones: Vec::new(),
            workspaces: Vec::new(),
            resource_budget: CodeIndexResourceBudget::default(),
        };
        store
            .begin_code_index_session(session.clone())
            .await
            .expect("checkpointed session should begin");
        store
            .finalize_code_index_session(session)
            .await
            .expect("checkpointed session should publish");
    }

    for commit in ["commit-a", "commit-b"] {
        let status = store
            .code_repository_scope_status(
                "fixture".to_owned(),
                commit.to_owned(),
                Vec::new(),
                Vec::new(),
            )
            .await
            .expect("scope alias should query")
            .expect("commit should resolve to the shared content scope");
        assert_eq!(
            status.last_indexed_scope_id.as_deref(),
            Some(source_scope.as_str())
        );
        assert_eq!(status.last_indexed_commit.as_deref(), Some(commit));
    }
}

#[tokio::test]
async fn completed_checkpoint_rejects_a_different_content_identity() {
    let store = SqliteGraphStore::open_in_memory().expect("store should open");
    store
        .upsert_code_repository(
            CodeRepositoryRegistration::new("repo", "fixture", "/tmp/repo", Vec::new(), Vec::new())
                .expect("registration should validate"),
        )
        .await
        .expect("repository should register");
    let source_scope = code_snapshot_scope_id("repo", "tree-a", &[], &[]);
    let session = CodeIndexSession {
        repository_id: "repo".to_owned(),
        source_scope: source_scope.clone(),
        base_resolved_commit_sha: None,
        resolved_commit_sha: "commit-a".to_owned(),
        tree_hash: "tree-a".to_owned(),
        path_filters: Vec::new(),
        language_filters: Vec::new(),
        full_replace: true,
        total_path_count: 0,
        changed_path_count: 0,
        skipped_unchanged_count: 0,
        deleted_paths: Vec::new(),
        changed_paths: Vec::new(),
        tombstones: Vec::new(),
        workspaces: Vec::new(),
        resource_budget: CodeIndexResourceBudget::default(),
    };
    store
        .begin_code_index_session(session.clone())
        .await
        .expect("initial checkpoint should begin");
    store
        .finalize_code_index_session(session.clone())
        .await
        .expect("initial checkpoint should complete");

    let mut mismatched = session;
    mismatched.resolved_commit_sha = "commit-b".to_owned();
    mismatched.tree_hash = "tree-b".to_owned();
    let error = store
        .begin_code_index_session(mismatched)
        .await
        .expect_err("a completed checkpoint must not authorize a different content identity");
    assert!(error.to_string().contains("checkpoint identity"));
    let checkpoint = store
        .code_index_checkpoint(source_scope.clone())
        .await
        .expect("checkpoint should query")
        .expect("mismatched restart must not delete the completed checkpoint");
    assert_eq!(checkpoint.state, "completed");
    let status = store
        .code_repository_scope_status(
            "repo".to_owned(),
            "commit-a".to_owned(),
            Vec::new(),
            Vec::new(),
        )
        .await
        .expect("scope status should query")
        .expect("original scope should remain published");
    assert_eq!(status.tree_hash.as_deref(), Some("tree-a"));
}

#[tokio::test]
async fn same_tree_publication_lazily_preserves_a_legacy_commit_alias() {
    let store = SqliteGraphStore::open_in_memory().expect("store should open");
    store
        .upsert_code_repository(
            CodeRepositoryRegistration::new("repo", "fixture", "/tmp/repo", Vec::new(), Vec::new())
                .expect("registration should validate"),
        )
        .await
        .expect("repository should register");
    let source_scope = code_snapshot_scope_id("repo", "same-tree", &[], &[]);
    let legacy_scope = source_scope.clone();
    store
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
        .expect("legacy content scope should persist without an alias");
    let session = CodeIndexSession {
        repository_id: "repo".to_owned(),
        source_scope: source_scope.clone(),
        base_resolved_commit_sha: None,
        resolved_commit_sha: "commit-b".to_owned(),
        tree_hash: "same-tree".to_owned(),
        path_filters: Vec::new(),
        language_filters: Vec::new(),
        full_replace: true,
        total_path_count: 0,
        changed_path_count: 0,
        skipped_unchanged_count: 0,
        deleted_paths: Vec::new(),
        changed_paths: Vec::new(),
        tombstones: Vec::new(),
        workspaces: Vec::new(),
        resource_budget: CodeIndexResourceBudget::default(),
    };
    store
        .begin_code_index_session(session.clone())
        .await
        .expect("same-tree replacement should begin");
    store
        .finalize_code_index_session(session)
        .await
        .expect("same-tree replacement should publish");

    for commit in ["commit-a", "commit-b"] {
        assert!(
            store
                .code_repository_scope_status(
                    "fixture".to_owned(),
                    commit.to_owned(),
                    Vec::new(),
                    Vec::new(),
                )
                .await
                .expect("scope alias should query")
                .is_some(),
            "{commit} should resolve to the shared scope"
        );
    }
}

#[tokio::test]
async fn staged_unpublished_scope_does_not_create_a_queryable_commit_alias() {
    let store = SqliteGraphStore::open_in_memory().expect("store should open");
    store
        .upsert_code_repository(
            CodeRepositoryRegistration::new("repo", "fixture", "/tmp/repo", Vec::new(), Vec::new())
                .expect("registration should validate"),
        )
        .await
        .expect("repository should register");
    store
        .run(|connection| {
            connection.execute(
                "INSERT INTO code_repository_scopes (
                     source_scope, repository_id, resolved_commit_sha, tree_hash,
                     path_filters_json, language_filters_json, indexed_file_count,
                     symbol_count, reference_count, chunk_count, stale, degraded_reason
                 ) VALUES ('staged-scope', 'repo', 'target', 'tree', '[]', '[]',
                           0, 0, 0, 0, 1, NULL)",
                [],
            )?;
            super::preserve_existing_scope_commit(connection, "repo", "staged-scope")?;
            let aliases = connection.query_row(
                "SELECT COUNT(*) FROM code_repository_commit_scopes
                 WHERE repository_id = 'repo' AND source_scope = 'staged-scope'",
                [],
                |row| row.get::<_, usize>(0),
            )?;
            if aliases != 0 {
                return Err(crate::storage::StorageError::Invariant(
                    "an unpublished staged scope became queryable through a commit alias"
                        .to_owned(),
                ));
            }
            Ok(())
        })
        .await
        .expect("staged scope alias invariant should hold");
}
