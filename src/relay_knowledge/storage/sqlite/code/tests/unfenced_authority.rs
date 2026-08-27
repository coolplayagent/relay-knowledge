//! Direct-writer exclusion tests for the single SQLite task authority.

use std::time::{SystemTime, UNIX_EPOCH};

use crate::{
    domain::{
        CodeIndexBatch, CodeIndexMode, CodeIndexPublicationFence, CodeIndexResourceBudget,
        CodeIndexSession, CodeIndexSnapshot, CodeIndexTaskState, CodeRepositoryRegistration,
        code_snapshot_scope_id,
    },
    storage::{
        CodeIndexTaskClaimRequest, CodeIndexTaskFailure, CodeIndexTaskSeed, CodeRepositoryStore,
        SqliteGraphStore, StorageError,
    },
};

#[tokio::test]
async fn staged_fenced_task_rejects_unfenced_mutations_without_writes() {
    let store = registered_store().await;
    seed_workspace_set(&store).await;
    let fenced_session = session("fenced", "commit-fenced", "tree-fenced");
    let running = queue_claim_and_stage(&store, &fenced_session, "active-worker").await;

    let direct_snapshot = snapshot("direct-snapshot", "commit-direct", "tree-direct");
    let direct_snapshot_scope = direct_snapshot.source_scope.clone();
    let snapshot_error = store
        .apply_code_index_snapshot(direct_snapshot)
        .await
        .expect_err("an unfenced snapshot must not overlap the durable task");
    assert!(
        matches!(snapshot_error, StorageError::InvalidInput(message) if message.contains(&running.task_id) && message.contains("running"))
    );

    let direct_session = session("direct-session", "commit-session", "tree-session");
    let direct_session_scope = direct_session.source_scope.clone();
    let session_error = store
        .begin_code_index_session(direct_session)
        .await
        .expect_err("an unfenced session must not overlap the durable task");
    assert!(
        matches!(session_error, StorageError::InvalidInput(message) if message.contains(&running.task_id) && message.contains("running"))
    );
    let cleanup_error = store
        .clear_code_workspace_state("repo".to_owned(), fenced_session.source_scope.clone())
        .await
        .expect_err("unfenced workspace cleanup must not overlap the durable task");
    assert!(cleanup_error.to_string().contains(&running.task_id));
    assert!(
        store
            .code_repository_auto_workspace_state_exists("repo".to_owned())
            .await
            .expect("workspace state should remain readable")
    );

    let counts = store
        .run(move |connection| {
            connection
                .query_row(
                    "SELECT
                       (SELECT COUNT(*) FROM code_repository_scopes WHERE source_scope = ?1),
                       (SELECT COUNT(*) FROM code_repository_index_checkpoints WHERE source_scope = ?2),
                       (SELECT COUNT(*) FROM code_repository_files WHERE source_scope IN (?1, ?2))",
                    rusqlite::params![direct_snapshot_scope, direct_session_scope],
                    |row| {
                        Ok((
                            row.get::<_, usize>(0)?,
                            row.get::<_, usize>(1)?,
                            row.get::<_, usize>(2)?,
                        ))
                    },
                )
                .map_err(StorageError::from)
        })
        .await
        .expect("rejected target counts should load");
    assert_eq!(counts, (0, 0, 0));
}

#[tokio::test]
async fn terminal_task_and_lingering_fence_allow_unfenced_mutations() {
    let store = registered_store().await;
    let fenced_session = session("terminal", "commit-terminal", "tree-terminal");
    let running = queue_claim_and_stage(&store, &fenced_session, "terminal-worker").await;
    let terminal = store
        .fail_code_index_task(CodeIndexTaskFailure {
            task_id: running.task_id.clone(),
            lease_owner: "terminal-worker".to_owned(),
            attempt_count: running.attempt_count,
            publication_generation: running.publication_generation,
            error_kind: "fixture".to_owned(),
            error_message: "terminal authority fixture".to_owned(),
            retry_backoff_ms: 1,
            max_attempts: 1,
            now_ms: now_millis(),
        })
        .await
        .expect("the running task should enter terminal state");
    assert_eq!(terminal.state, CodeIndexTaskState::DeadLetter);
    let lingering_fence_count = store
        .run(move |connection| {
            connection
                .query_row(
                    "SELECT COUNT(*) FROM code_repository_publication_fences
                     WHERE repository_id = 'repo' AND task_id = ?1",
                    [running.task_id],
                    |row| row.get::<_, usize>(0),
                )
                .map_err(StorageError::from)
        })
        .await
        .expect("fence row count should load");
    assert_eq!(lingering_fence_count, 1);

    let direct = snapshot("after-terminal", "commit-after", "tree-after");
    let source_scope = direct.source_scope.clone();
    store
        .apply_code_index_snapshot(direct)
        .await
        .expect("terminal task authority must not block a direct single-store writer");
    let published = store
        .code_repository_status("repo".to_owned())
        .await
        .expect("repository status should load")
        .expect("repository status should exist");
    assert_eq!(
        published.last_indexed_scope_id.as_deref(),
        Some(source_scope.as_str())
    );
    seed_workspace_set(&store).await;
    store
        .clear_code_workspace_state("repo".to_owned(), source_scope)
        .await
        .expect("terminal task authority must allow direct workspace cleanup");
    assert!(
        !store
            .code_repository_auto_workspace_state_exists("repo".to_owned())
            .await
            .expect("cleared workspace state should remain readable")
    );
}

#[tokio::test]
async fn direct_checkpoint_cannot_be_overtaken_or_continue_after_task_adoption() {
    let store = registered_store().await;
    let direct_session = session("direct-owner", "commit-direct", "tree-direct");
    store
        .begin_code_index_session(direct_session.clone())
        .await
        .expect("direct checkpoint should begin without a durable task");

    let other = session("other-target", "commit-other", "tree-other");
    let queue_error = store
        .queue_code_index_task(task_seed(&other, "other-target"))
        .await
        .expect_err("another target must not overtake the direct checkpoint");
    assert!(
        queue_error
            .to_string()
            .contains("unfinished direct checkpoint")
    );

    let running = queue_and_claim(&store, &direct_session, "adopting-worker").await;
    let batch_error = store
        .apply_code_index_batch(CodeIndexBatch {
            repository_id: direct_session.repository_id.clone(),
            source_scope: direct_session.source_scope.clone(),
            batch_index: 1,
            parsed_byte_count: 0,
            files: Vec::new(),
            symbols: Vec::new(),
            references: Vec::new(),
            imports: Vec::new(),
            dependencies: Vec::new(),
            feature_flags: Vec::new(),
            framework_nodes: Vec::new(),
            framework_edges: Vec::new(),
            routes: Vec::new(),
            chunks: Vec::new(),
            diagnostics: Vec::new(),
        })
        .await
        .expect_err("the adopted task must fence the former direct batch writer");
    assert!(batch_error.to_string().contains(&running.task_id));
    let finalize_error = store
        .finalize_code_index_session(direct_session.clone())
        .await
        .expect_err("the adopted task must fence the former direct finalizer");
    assert!(finalize_error.to_string().contains(&running.task_id));

    let durable = store
        .run(move |connection| {
            connection
                .query_row(
                    "SELECT checkpoint.state, checkpoint.batch_count,
                            (SELECT COUNT(*) FROM code_repository_index_batch_staging
                             WHERE source_scope = checkpoint.source_scope)
                     FROM code_repository_index_checkpoints checkpoint
                     WHERE checkpoint.source_scope = ?1",
                    [direct_session.source_scope],
                    |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, usize>(1)?,
                            row.get::<_, usize>(2)?,
                        ))
                    },
                )
                .map_err(StorageError::from)
        })
        .await
        .expect("direct checkpoint state should load");
    assert_eq!(durable, ("indexing".to_owned(), 0, 0));
}

async fn registered_store() -> SqliteGraphStore {
    let store = SqliteGraphStore::open_in_memory().expect("store should open");
    store
        .upsert_code_repository(
            CodeRepositoryRegistration::new(
                "repo",
                "fixture",
                "/tmp/unfenced-authority",
                Vec::new(),
                Vec::new(),
            )
            .expect("registration should validate"),
        )
        .await
        .expect("repository should register");
    store
}

async fn seed_workspace_set(store: &SqliteGraphStore) {
    let set_id = super::workspace::workspace_set_id("repo");
    store
        .run(move |connection| {
            connection.execute(
                "INSERT OR REPLACE INTO code_repository_sets
                 (set_id, alias, description, default_ref_policy_json,
                  created_at_ms, updated_at_ms)
                 VALUES (?1, 'auto:repo', NULL, '{}', 1, 1)",
                [set_id],
            )?;
            Ok(())
        })
        .await
        .expect("workspace state should seed");
}

async fn queue_claim_and_stage(
    store: &SqliteGraphStore,
    session: &CodeIndexSession,
    lease_owner: &str,
) -> crate::domain::CodeIndexTaskRecord {
    let running = queue_and_claim(store, session, lease_owner).await;
    store
        .begin_code_index_session_with_fence(
            session.clone(),
            CodeIndexPublicationFence {
                repository_id: running.repository_id.clone(),
                task_id: running.task_id.clone(),
                lease_owner: lease_owner.to_owned(),
                attempt_count: running.attempt_count,
                generation: running.publication_generation,
            },
        )
        .await
        .expect("fenced session should stage");
    running
}

async fn queue_and_claim(
    store: &SqliteGraphStore,
    session: &CodeIndexSession,
    lease_owner: &str,
) -> crate::domain::CodeIndexTaskRecord {
    let queued = store
        .queue_code_index_task(task_seed(session, lease_owner))
        .await
        .expect("task should queue");
    store
        .claim_code_index_task(CodeIndexTaskClaimRequest {
            task_id: Some(queued.task_id),
            lease_owner: lease_owner.to_owned(),
            lease_duration_ms: 60_000,
            max_attempts: 3,
            now_ms: now_millis(),
        })
        .await
        .expect("task claim should run")
        .expect("task should claim")
}

fn task_seed(session: &CodeIndexSession, fingerprint: &str) -> CodeIndexTaskSeed {
    CodeIndexTaskSeed {
        repository_id: session.repository_id.clone(),
        alias: "fixture".to_owned(),
        ref_selector: "HEAD".to_owned(),
        resolved_commit_sha: session.resolved_commit_sha.clone(),
        tree_hash: session.tree_hash.clone(),
        source_scope: session.source_scope.clone(),
        path_filters: session.path_filters.clone(),
        language_filters: session.language_filters.clone(),
        mode: CodeIndexMode::Full,
        input_fingerprint: format!("unfenced-authority:{fingerprint}"),
        resource_budget: session.resource_budget,
        payload_json: "{}".to_owned(),
        now_ms: now_millis(),
    }
}

fn snapshot(_label: &str, resolved_commit_sha: &str, tree_hash: &str) -> CodeIndexSnapshot {
    CodeIndexSnapshot {
        repository_id: "repo".to_owned(),
        source_scope: code_snapshot_scope_id("repo", tree_hash, &[], &[]),
        base_resolved_commit_sha: None,
        resolved_commit_sha: resolved_commit_sha.to_owned(),
        tree_hash: tree_hash.to_owned(),
        path_filters: Vec::new(),
        language_filters: Vec::new(),
        full_replace: true,
        changed_path_count: 0,
        skipped_unchanged_count: 0,
        deleted_paths: Vec::new(),
        tombstones: Vec::new(),
        files: Vec::new(),
        symbols: Vec::new(),
        references: Vec::new(),
        imports: Vec::new(),
        calls: Vec::new(),
        dependencies: Vec::new(),
        feature_flags: Vec::new(),
        framework_nodes: Vec::new(),
        framework_edges: Vec::new(),
        routes: Vec::new(),
        chunks: Vec::new(),
        workspaces: Vec::new(),
        diagnostics: Vec::new(),
    }
}

fn session(label: &str, resolved_commit_sha: &str, tree_hash: &str) -> CodeIndexSession {
    let snapshot = snapshot(label, resolved_commit_sha, tree_hash);
    CodeIndexSession {
        repository_id: snapshot.repository_id,
        source_scope: snapshot.source_scope,
        base_resolved_commit_sha: None,
        resolved_commit_sha: snapshot.resolved_commit_sha,
        tree_hash: snapshot.tree_hash,
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
    }
}

fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock should follow epoch")
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX)
}
