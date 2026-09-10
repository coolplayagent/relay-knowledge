//! A fenced worktree publication must charge the complete triggered Java projection.
use super::partitioned_sqlite_fixtures::{
    batch_from_snapshot, registration, session_for_snapshot, snapshot, unique_temp_dir,
};
use relay_knowledge::{
    domain::{
        CodeIndexMode, CodeIndexPublicationFence, CodeIndexResourceBudget, JavaNamespaceEvidence,
        JavaSourceSet, code_snapshot_scope_id,
    },
    storage::{
        CodeIndexPublicationStore as _, CodeIndexTaskClaimRequest, CodeIndexTaskSeed,
        CodeIndexTaskStore as _, RepositoryCatalogStore as _, SqliteGraphStore, StorageError,
    },
};
use rusqlite::Connection;

#[tokio::test]
async fn fenced_worktree_clone_admits_all_source_set_projection_bytes_before_publication() {
    let directory = unique_temp_dir("java-clone-source-set-budget");
    std::fs::create_dir_all(&directory).unwrap();
    let database = directory.join("code.sqlite");
    eprintln!("clone budget fixture database={}", database.display());
    let store = SqliteGraphStore::open(&database).unwrap();
    store
        .upsert_code_repository(registration("repo", "fixture"))
        .await
        .unwrap();
    let base_scope = code_snapshot_scope_id("repo", "base-tree", &[], &[]);
    let module_root = std::iter::repeat_n("module_component_with_long_name", 32)
        .collect::<Vec<_>>()
        .join("/");
    let mut base = snapshot("repo", &base_scope, "");
    base.resolved_commit_sha = "base-commit".into();
    base.tree_hash = "base-tree".into();
    base.chunks.clear();
    base.files[0].path = format!("{module_root}/src/main/java/p/App.java");
    base.files[0].language_id = "java".into();
    base.files[0].java_namespace = Some(JavaNamespaceEvidence {
        source_set: JavaSourceSet::Main {
            module_root: module_root.clone(),
        },
        package: "p".into(),
        top_level_types: (0..16).map(|index| format!("Type{index}")).collect(),
        complete: true,
    });
    // Use the public checkpointed path so fallback cannot stop at a missing
    // immutable-base proof instead of exercising its row/byte-quantum guard.
    let budget = CodeIndexResourceBudget::new(8, 60 * 1024, 1000).unwrap();
    let mut session = session_for_snapshot(&base);
    session.resource_budget = CodeIndexResourceBudget::new(
        budget.max_files_per_batch,
        budget.max_bytes_per_batch * 2,
        budget.max_rows_per_batch,
    )
    .unwrap();
    store
        .begin_code_index_session(session.clone())
        .await
        .unwrap();
    store
        .apply_code_index_batch(batch_from_snapshot(base.clone()))
        .await
        .unwrap();
    store.finalize_code_index_session(session).await.unwrap();
    let checkpoint = store
        .code_index_checkpoint(base_scope.clone())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(checkpoint.state, "completed");
    assert!(checkpoint.committed_fact_row_count > 0);
    assert_eq!(
        checkpoint.resource_budget.max_rows_per_batch,
        budget.max_rows_per_batch
    );
    assert_eq!(
        checkpoint.resource_budget.max_bytes_per_batch,
        budget.max_bytes_per_batch * 2
    );
    let audit = Connection::open(&database).unwrap();
    let projected: (usize, usize) = audit.query_row(
        "SELECT count(*), coalesce(sum(length(CAST(source_scope AS BLOB)) + length(CAST(path AS BLOB)) + length(CAST(package AS BLOB)) + length(CAST(type_name AS BLOB)) + length(CAST(source_set_kind AS BLOB)) + length(CAST(module_root AS BLOB))), 0) FROM code_repository_java_types WHERE source_scope=?1",
        [&base_scope], |row| Ok((row.get(0)?, row.get(1)?)),
    ).unwrap();
    assert_eq!(
        projected.0, 16,
        "fixture must persist all valid source-set types"
    );
    // The clone contract charges source reads and target writes, even before row overhead.
    assert!(projected.1 * 2 > budget.max_bytes_per_batch);
    let tree = "worktree:0123456789abcdef";
    let target_scope = code_snapshot_scope_id("repo", tree, &[], &[]);
    let mut delta = base;
    delta.full_replace = false;
    delta.base_resolved_commit_sha = Some("base-commit".into());
    delta.resolved_commit_sha = "worktree:base-commit:0123456789abcdef".into();
    delta.source_scope = target_scope.clone();
    delta.tree_hash = tree.into();
    delta.changed_path_count = 0;
    delta.skipped_unchanged_count = 1;
    delta.files.clear();
    let now_ms = u64::try_from(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis(),
    )
    .unwrap();
    let queued = store
        .queue_code_index_task(CodeIndexTaskSeed {
            repository_id: "repo".into(),
            alias: "fixture".into(),
            ref_selector: "base-commit".into(),
            resolved_commit_sha: delta.resolved_commit_sha.clone(),
            tree_hash: tree.into(),
            source_scope: target_scope.clone(),
            path_filters: vec![],
            language_filters: vec![],
            mode: CodeIndexMode::WorktreeOverlay,
            input_fingerprint: "java-clone-budget".into(),
            resource_budget: budget,
            payload_json: "{}".into(),
            now_ms,
        })
        .await
        .unwrap();
    let claimed = store
        .claim_code_index_task(CodeIndexTaskClaimRequest {
            task_id: Some(queued.task_id),
            lease_owner: "budget-worker".into(),
            lease_duration_ms: 600_000,
            max_attempts: 3,
            now_ms,
        })
        .await
        .unwrap()
        .unwrap();
    let result = store
        .apply_code_index_snapshot_with_fence(
            delta,
            CodeIndexPublicationFence {
                repository_id: "repo".into(),
                task_id: claimed.task_id,
                lease_owner: "budget-worker".into(),
                attempt_count: claimed.attempt_count,
                generation: claimed.publication_generation,
            },
        )
        .await;
    let cloned: (usize, usize) = audit.query_row(
        "SELECT count(*), coalesce(sum(length(CAST(source_scope AS BLOB)) + length(CAST(path AS BLOB)) + length(CAST(package AS BLOB)) + length(CAST(type_name AS BLOB)) + length(CAST(source_set_kind AS BLOB)) + length(CAST(module_root AS BLOB))), 0) FROM code_repository_java_types WHERE source_scope=?1",
        [&target_scope], |row| Ok((row.get(0)?, row.get(1)?)),
    ).unwrap();
    eprintln!(
        "database={} base_projection={projected:?} cloned_projection={cloned:?} budget={} result={result:?}",
        database.display(),
        budget.max_bytes_per_batch
    );
    assert!(
        matches!(result, Err(StorageError::DurableStagingRequired(message)) if message.contains("row or byte quantum smaller than its immutable base")),
        "an oversized direct clone must reach the proven-base quantum guard before writing target facts"
    );
    assert_eq!(cloned, (0, 0));
    let status = store
        .code_repository_status("fixture".into())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        status.last_indexed_scope_id.as_deref(),
        Some(base_scope.as_str())
    );
}
