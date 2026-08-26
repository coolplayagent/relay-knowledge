use std::time::{SystemTime, UNIX_EPOCH};

use crate::{
    domain::{
        CodeIndexMode, CodeIndexPublicationFence, CodeIndexResourceBudget, CodeIndexSnapshot,
        CodeIndexTaskState, CodeRepositoryRegistration, code_snapshot_scope_id,
    },
    storage::{
        CodeIndexTaskClaimRequest, CodeIndexTaskCompletion, CodeIndexTaskSeed, CodeRepositoryStore,
        CodeScopeRetentionRequest, PartitionedSqliteKnowledgeStore, SqliteGraphStore,
    },
};

const REPOSITORY_ID: &str = "repo-worktree-rebind";
const ALIAS: &str = "worktree-rebind";
const BASE_COMMIT: &str = "base-commit";
const PATH_FILTER: &str = "src";
const LANGUAGE_FILTER: &str = "rust";

#[tokio::test]
async fn live_worktree_attempt_rebinds_target_before_publication_and_retention() {
    let store = registered_store().await;
    let (base_scope, pending_scope, real_scope, snapshot) = worktree_fixture(&store).await;
    let real_commit = snapshot.resolved_commit_sha.clone();
    let real_tree = snapshot.tree_hash.clone();
    let running = claim_worktree_task(&store, &pending_scope, "worker-live", now_millis()).await;
    let publication_fence = fence(&running, "worker-live");
    let summary = store
        .apply_code_index_snapshot_with_fence(snapshot, publication_fence.clone())
        .await
        .expect("live dirty worktree snapshot should stage");
    crate::storage::stage_empty_business_projection_with_fence_for_test(
        &store,
        summary.repository_id.clone(),
        summary.source_scope.clone(),
        summary.resolved_commit_sha.clone(),
        publication_fence.clone(),
    )
    .await
    .expect("worktree business projection should stage");
    store
        .refresh_software_global_projection_with_fence(
            summary.source_scope.clone(),
            publication_fence,
        )
        .await
        .expect("software projection should publish the staged overlay");

    assert_eq!(summary.source_scope, real_scope);
    let active = store
        .active_code_index_task(REPOSITORY_ID.to_owned())
        .await
        .expect("active task should load")
        .expect("task should remain active until completion");
    assert_eq!(active.state, CodeIndexTaskState::Running);
    assert_eq!(active.source_scope, real_scope);
    assert_eq!(active.resolved_commit_sha, real_commit);
    assert_eq!(active.tree_hash, real_tree);

    let retention = store
        .prune_code_repository_scopes(CodeScopeRetentionRequest {
            repository_id: REPOSITORY_ID.to_owned(),
            active_scope: real_scope.clone(),
            retain_recent_successful_scopes: 0,
            repository_retention_cutoff_ms: None,
            repository_retention_cutoff_generation: None,
            repository_retention_initial_scope: None,
        })
        .await
        .expect("retention should observe rebound target and pinned base");
    assert!(retention.retained_scopes.contains(&real_scope));
    assert!(retention.retained_scopes.contains(&base_scope));
    assert!(!retention.pruned_scopes.contains(&real_scope));
    assert!(!retention.pruned_scopes.contains(&base_scope));
}

#[tokio::test]
async fn stale_worktree_attempt_cannot_rebind_or_publish_after_takeover() {
    let store = registered_store().await;
    let (_, pending_scope, real_scope, snapshot) = worktree_fixture(&store).await;
    let first =
        claim_worktree_task_with_lease(&store, &pending_scope, "worker-old", now_millis(), 60_000)
            .await;
    expire_task_lease(&store, &first.task_id).await;
    let second = store
        .claim_code_index_task(CodeIndexTaskClaimRequest {
            task_id: Some(first.task_id.clone()),
            lease_owner: "worker-new".to_owned(),
            lease_duration_ms: 60_000,
            max_attempts: 3,
            now_ms: now_millis(),
        })
        .await
        .expect("takeover should run")
        .expect("expired task should be reclaimed");
    assert!(second.publication_generation > first.publication_generation);

    let error = store
        .apply_code_index_snapshot_with_fence(snapshot, fence(&first, "worker-old"))
        .await
        .expect_err("stale attempt must not rebind or publish the overlay");
    assert!(error.to_string().contains("no longer active"));

    let active = store
        .active_code_index_task(REPOSITORY_ID.to_owned())
        .await
        .expect("active task should load")
        .expect("takeover should remain active");
    assert_eq!(active.task_id, second.task_id);
    assert_eq!(active.source_scope, pending_scope);
    let real_scope_count = store
        .run(move |connection| {
            connection
                .query_row(
                    "SELECT COUNT(*) FROM code_repository_scopes WHERE source_scope = ?1",
                    [real_scope],
                    |row| row.get::<_, usize>(0),
                )
                .map_err(crate::storage::StorageError::from)
        })
        .await
        .expect("scope count should query");
    assert_eq!(real_scope_count, 0);
}

#[tokio::test]
async fn partitioned_publication_rebinds_control_target_before_status_mirror() {
    let store = partitioned_store();
    store
        .upsert_code_repository(registration())
        .await
        .expect("repository should register");
    let base = snapshot_identity("base-tree", BASE_COMMIT, None, true);
    let base_snapshot = base.1;
    let queued_base = store
        .queue_code_index_task(CodeIndexTaskSeed {
            repository_id: REPOSITORY_ID.to_owned(),
            alias: ALIAS.to_owned(),
            ref_selector: BASE_COMMIT.to_owned(),
            resolved_commit_sha: base_snapshot.resolved_commit_sha.clone(),
            tree_hash: base_snapshot.tree_hash.clone(),
            source_scope: base_snapshot.source_scope.clone(),
            path_filters: base_snapshot.path_filters.clone(),
            language_filters: base_snapshot.language_filters.clone(),
            mode: CodeIndexMode::Full,
            input_fingerprint: "partitioned-worktree-base".to_owned(),
            resource_budget: CodeIndexResourceBudget::default(),
            payload_json: "{}".to_owned(),
            now_ms: now_millis(),
        })
        .await
        .expect("base task should queue");
    let base_task = store
        .claim_code_index_task(CodeIndexTaskClaimRequest {
            task_id: Some(queued_base.task_id),
            lease_owner: "worker-base".to_owned(),
            lease_duration_ms: 60_000,
            max_attempts: 3,
            now_ms: now_millis(),
        })
        .await
        .expect("base task claim should run")
        .expect("base task should claim");
    let base_fence = fence(&base_task, "worker-base");
    let base_summary = store
        .apply_code_index_snapshot_with_fence(base_snapshot, base_fence.clone())
        .await
        .expect("base scope should publish");
    crate::storage::stage_empty_business_projection_with_fence_for_test(
        &store,
        base_summary.repository_id.clone(),
        base_summary.source_scope.clone(),
        base_summary.resolved_commit_sha.clone(),
        base_fence.clone(),
    )
    .await
    .expect("base business projection should stage");
    store
        .refresh_software_global_projection_with_fence(base_summary.source_scope, base_fence)
        .await
        .expect("base software projection should publish");
    store
        .complete_code_index_task(CodeIndexTaskCompletion {
            task_id: base_task.task_id,
            lease_owner: "worker-base".to_owned(),
            attempt_count: base_task.attempt_count,
            publication_generation: base_task.publication_generation,
            now_ms: now_millis(),
        })
        .await
        .expect("base task should complete");
    let pending_tree = format!("worktree:pending:{BASE_COMMIT}");
    let pending_scope = scope_for_tree(&pending_tree);
    let overlay_hash = "fedcba9876543210";
    let real_tree = format!("worktree:{overlay_hash}");
    let real_commit = format!("worktree:{BASE_COMMIT}:{overlay_hash}");
    let (real_scope, snapshot) = snapshot_identity(
        &real_tree,
        &real_commit,
        Some(BASE_COMMIT.to_owned()),
        false,
    );
    let queued = store
        .queue_code_index_task(worktree_seed(pending_scope, pending_tree, "partitioned"))
        .await
        .expect("worktree task should queue");
    let running = store
        .claim_code_index_task(CodeIndexTaskClaimRequest {
            task_id: Some(queued.task_id),
            lease_owner: "worker-partitioned".to_owned(),
            lease_duration_ms: 60_000,
            max_attempts: 3,
            now_ms: now_millis(),
        })
        .await
        .expect("partitioned claim should run")
        .expect("worktree task should claim");

    let publication_fence = fence(&running, "worker-partitioned");
    let summary = store
        .apply_code_index_snapshot_with_fence(snapshot, publication_fence.clone())
        .await
        .expect("partitioned overlay should stage across shard and control");
    crate::storage::stage_empty_business_projection_with_fence_for_test(
        &store,
        summary.repository_id.clone(),
        summary.source_scope.clone(),
        summary.resolved_commit_sha.clone(),
        publication_fence.clone(),
    )
    .await
    .expect("partitioned business projection should stage");
    store
        .refresh_software_global_projection_with_fence(
            summary.source_scope.clone(),
            publication_fence,
        )
        .await
        .expect("software projection should activate shard and control status");

    assert_eq!(summary.source_scope, real_scope);
    let active = store
        .active_code_index_task(REPOSITORY_ID.to_owned())
        .await
        .expect("control task should load")
        .expect("running task should remain visible");
    assert_eq!(active.source_scope, real_scope);
    assert_eq!(active.resolved_commit_sha, real_commit);
    assert_eq!(active.tree_hash, real_tree);
    let status = store
        .code_repository_status(REPOSITORY_ID.to_owned())
        .await
        .expect("partitioned status should load")
        .expect("repository status should exist");
    assert_eq!(
        status.last_indexed_scope_id.as_deref(),
        Some(real_scope.as_str())
    );
}

#[tokio::test]
async fn worktree_reservations_keep_rebinds_inside_scope_capacity() {
    let store = registered_store().await;
    let (_, pending_scope, _, _) = worktree_fixture(&store).await;
    insert_published_scope_fillers(&store, 62).await;
    let first = claim_worktree_task(&store, &pending_scope, "worker-first", now_millis()).await;

    let pending_tree = format!("worktree:pending:{BASE_COMMIT}");
    let rejected = store
        .queue_code_index_task(worktree_seed(
            pending_scope,
            pending_tree,
            "capacity-overflow",
        ))
        .await
        .expect_err("another observation must reserve its eventual real scope");
    assert!(matches!(
        rejected,
        crate::storage::StorageError::CapacityExceeded(_)
    ));
    let active = store
        .active_code_index_task(REPOSITORY_ID.to_owned())
        .await
        .expect("pending task should load")
        .expect("running reservation should remain");
    assert_eq!(active.task_id, first.task_id);
}

async fn registered_store() -> SqliteGraphStore {
    let store = SqliteGraphStore::open_in_memory().expect("store should open");
    store
        .upsert_code_repository(
            CodeRepositoryRegistration::new(
                REPOSITORY_ID,
                ALIAS,
                "/tmp/worktree-rebind",
                vec![PATH_FILTER.to_owned()],
                vec![LANGUAGE_FILTER.to_owned()],
            )
            .expect("registration should validate"),
        )
        .await
        .expect("repository should persist");
    store
}

fn registration() -> CodeRepositoryRegistration {
    CodeRepositoryRegistration::new(
        REPOSITORY_ID,
        ALIAS,
        "/tmp/worktree-rebind",
        vec![PATH_FILTER.to_owned()],
        vec![LANGUAGE_FILTER.to_owned()],
    )
    .expect("registration should validate")
}

fn partitioned_store() -> PartitionedSqliteKnowledgeStore {
    let root = std::env::temp_dir().join(format!(
        "relay-knowledge-worktree-rebind-{}-{}",
        std::process::id(),
        now_millis()
    ));
    let environment = crate::env::EnvironmentConfig::from_pairs(
        crate::env::PlatformKind::current(),
        [(
            "RELAY_KNOWLEDGE_HOME",
            root.to_str().expect("temp path should be UTF-8"),
        )],
    )
    .expect("environment should parse");
    let paths = crate::paths::RuntimePaths::resolve(&environment.platform, &environment.paths)
        .expect("runtime paths should resolve");
    PartitionedSqliteKnowledgeStore::open(paths.database_file(), paths)
        .expect("partitioned store should open")
}

async fn insert_published_scope_fillers(store: &SqliteGraphStore, count: usize) {
    store
        .run(move |connection| {
            for index in 0..count {
                connection.execute(
                    "INSERT INTO code_repository_scopes (
                         source_scope, repository_id, resolved_commit_sha, tree_hash,
                         path_filters_json, language_filters_json, indexed_file_count,
                         symbol_count, reference_count, chunk_count, stale, degraded_reason
                     ) VALUES (?1, ?2, ?3, ?4, '[]', '[]', 0, 0, 0, 0, 0, NULL)",
                    rusqlite::params![
                        format!("filler-scope-{index:03}"),
                        REPOSITORY_ID,
                        format!("filler-commit-{index:03}"),
                        format!("filler-tree-{index:03}"),
                    ],
                )?;
            }
            Ok(())
        })
        .await
        .expect("published filler scopes should persist");
}

async fn worktree_fixture(store: &SqliteGraphStore) -> (String, String, String, CodeIndexSnapshot) {
    let path_filters = vec![PATH_FILTER.to_owned()];
    let language_filters = vec![LANGUAGE_FILTER.to_owned()];
    let base_tree = "base-tree";
    let base_scope =
        code_snapshot_scope_id(REPOSITORY_ID, base_tree, &path_filters, &language_filters);
    let base_scope_for_insert = base_scope.clone();
    let base_path_filters = path_filters.clone();
    let base_language_filters = language_filters.clone();
    store
        .run(move |connection| {
            connection.execute(
                "INSERT INTO code_repository_scopes (
                     source_scope, repository_id, resolved_commit_sha, tree_hash,
                     path_filters_json, language_filters_json, indexed_file_count,
                     symbol_count, reference_count, chunk_count, stale, degraded_reason
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, 0, 0, 0, 0, 0, NULL)",
                rusqlite::params![
                    base_scope_for_insert,
                    REPOSITORY_ID,
                    BASE_COMMIT,
                    base_tree,
                    serde_json::to_string(&base_path_filters).expect("paths should serialize"),
                    serde_json::to_string(&base_language_filters)
                        .expect("languages should serialize"),
                ],
            )?;
            connection.execute(
                "UPDATE code_repositories
                 SET last_indexed_scope_id = ?2, last_indexed_commit = ?3,
                     tree_hash = ?4, state = 'fresh', stale = 0
                 WHERE repository_id = ?1",
                rusqlite::params![REPOSITORY_ID, base_scope_for_insert, BASE_COMMIT, base_tree],
            )?;
            connection.execute(
                "INSERT INTO code_repository_reference_search_manifests (
                     source_scope, projection_version, reference_count, group_count
                 ) VALUES (?1, 2, 0, 0)",
                [base_scope_for_insert],
            )?;
            Ok(())
        })
        .await
        .expect("base scope should persist");

    let pending_tree = format!("worktree:pending:{BASE_COMMIT}");
    let pending_scope = code_snapshot_scope_id(
        REPOSITORY_ID,
        &pending_tree,
        &path_filters,
        &language_filters,
    );
    let overlay_hash = "0123456789abcdef";
    let real_tree = format!("worktree:{overlay_hash}");
    let real_commit = format!("worktree:{BASE_COMMIT}:{overlay_hash}");
    let real_scope =
        code_snapshot_scope_id(REPOSITORY_ID, &real_tree, &path_filters, &language_filters);
    let snapshot = CodeIndexSnapshot {
        repository_id: REPOSITORY_ID.to_owned(),
        source_scope: real_scope.clone(),
        base_resolved_commit_sha: Some(BASE_COMMIT.to_owned()),
        resolved_commit_sha: real_commit,
        tree_hash: real_tree,
        path_filters: path_filters.clone(),
        language_filters: language_filters.clone(),
        full_replace: false,
        changed_path_count: 1,
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
        routes: Vec::new(),
        chunks: Vec::new(),
        workspaces: Vec::new(),
        diagnostics: Vec::new(),
    };
    store
        .queue_code_index_task(CodeIndexTaskSeed {
            repository_id: REPOSITORY_ID.to_owned(),
            alias: ALIAS.to_owned(),
            ref_selector: BASE_COMMIT.to_owned(),
            resolved_commit_sha: pending_tree.clone(),
            tree_hash: pending_tree,
            source_scope: pending_scope.clone(),
            path_filters,
            language_filters,
            mode: CodeIndexMode::WorktreeOverlay,
            input_fingerprint: format!("worktree-rebind-{pending_scope}"),
            resource_budget: CodeIndexResourceBudget::default(),
            payload_json: "{}".to_owned(),
            now_ms: now_millis(),
        })
        .await
        .expect("worktree task should queue");

    (base_scope, pending_scope, real_scope, snapshot)
}

fn scope_for_tree(tree_hash: &str) -> String {
    code_snapshot_scope_id(
        REPOSITORY_ID,
        tree_hash,
        &[PATH_FILTER.to_owned()],
        &[LANGUAGE_FILTER.to_owned()],
    )
}

fn snapshot_identity(
    tree_hash: &str,
    resolved_commit_sha: &str,
    base_resolved_commit_sha: Option<String>,
    full_replace: bool,
) -> (String, CodeIndexSnapshot) {
    let source_scope = scope_for_tree(tree_hash);
    (
        source_scope.clone(),
        CodeIndexSnapshot {
            repository_id: REPOSITORY_ID.to_owned(),
            source_scope,
            base_resolved_commit_sha,
            resolved_commit_sha: resolved_commit_sha.to_owned(),
            tree_hash: tree_hash.to_owned(),
            path_filters: vec![PATH_FILTER.to_owned()],
            language_filters: vec![LANGUAGE_FILTER.to_owned()],
            full_replace,
            changed_path_count: 1,
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
            routes: Vec::new(),
            chunks: Vec::new(),
            workspaces: Vec::new(),
            diagnostics: Vec::new(),
        },
    )
}

fn worktree_seed(source_scope: String, pending_tree: String, suffix: &str) -> CodeIndexTaskSeed {
    CodeIndexTaskSeed {
        repository_id: REPOSITORY_ID.to_owned(),
        alias: ALIAS.to_owned(),
        ref_selector: BASE_COMMIT.to_owned(),
        resolved_commit_sha: pending_tree.clone(),
        tree_hash: pending_tree,
        source_scope,
        path_filters: vec![PATH_FILTER.to_owned()],
        language_filters: vec![LANGUAGE_FILTER.to_owned()],
        mode: CodeIndexMode::WorktreeOverlay,
        input_fingerprint: format!("worktree-rebind-{suffix}"),
        resource_budget: CodeIndexResourceBudget::default(),
        payload_json: "{}".to_owned(),
        now_ms: now_millis(),
    }
}

async fn claim_worktree_task(
    store: &SqliteGraphStore,
    pending_scope: &str,
    owner: &str,
    now_ms: u64,
) -> crate::domain::CodeIndexTaskRecord {
    claim_worktree_task_with_lease(store, pending_scope, owner, now_ms, 60_000).await
}

async fn claim_worktree_task_with_lease(
    store: &SqliteGraphStore,
    pending_scope: &str,
    owner: &str,
    now_ms: u64,
    lease_duration_ms: u64,
) -> crate::domain::CodeIndexTaskRecord {
    let task = store
        .active_code_index_task(REPOSITORY_ID.to_owned())
        .await
        .expect("queued task should load")
        .expect("queued task should exist");
    assert_eq!(task.source_scope, pending_scope);
    store
        .claim_code_index_task(CodeIndexTaskClaimRequest {
            task_id: Some(task.task_id),
            lease_owner: owner.to_owned(),
            lease_duration_ms,
            max_attempts: 3,
            now_ms,
        })
        .await
        .expect("task claim should run")
        .expect("worktree task should claim")
}

async fn expire_task_lease(store: &SqliteGraphStore, task_id: &str) {
    let task_id = task_id.to_owned();
    let changed = store
        .run(move |connection| {
            Ok(connection.execute(
                "UPDATE code_repository_index_tasks
                 SET lease_expires_at_ms = 0
                 WHERE task_id = ?1 AND state = 'running'",
                [task_id],
            )?)
        })
        .await
        .expect("task lease fixture should expire");
    assert_eq!(changed, 1, "one running task lease should expire");
}

fn fence(task: &crate::domain::CodeIndexTaskRecord, owner: &str) -> CodeIndexPublicationFence {
    CodeIndexPublicationFence {
        repository_id: task.repository_id.clone(),
        task_id: task.task_id.clone(),
        lease_owner: owner.to_owned(),
        attempt_count: task.attempt_count,
        generation: task.publication_generation,
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
