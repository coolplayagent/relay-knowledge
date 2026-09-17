use super::*;
use crate::{
    application::{RelayKnowledgeService, RuntimeConfiguration},
    code::prepare_full_index_plan,
    domain::{
        CodeIndexMode, CodeIndexPublicationFence, CodeIndexResourceBudget, CodeIndexSession,
        CodeRepositoryRegistration, CodeRepositorySelector,
    },
    env::{EnvironmentConfig, PlatformKind},
    paths::RuntimePaths,
    storage::{
        CodeIndexTaskClaimRequest, CodeIndexTaskSeed, PartitionedSqliteKnowledgeStore,
        SqliteGraphStore,
    },
};

fn seed(session: &CodeIndexSession, label: &str) -> CodeIndexTaskSeed {
    CodeIndexTaskSeed {
        repository_id: session.repository_id.clone(),
        alias: "fixture".into(),
        ref_selector: "HEAD".into(),
        resolved_commit_sha: session.resolved_commit_sha.clone(),
        tree_hash: session.tree_hash.clone(),
        source_scope: session.source_scope.clone(),
        path_filters: session.path_filters.clone(),
        language_filters: session.language_filters.clone(),
        mode: CodeIndexMode::Full,
        input_fingerprint: label.into(),
        resource_budget: session.resource_budget,
        payload_json: "{}".into(),
        now_ms: crate::clock::system_now_millis().unwrap(),
    }
}

async fn fixture(
    partitioned: bool,
) -> (
    TempSourceDir,
    Arc<dyn KnowledgeStore>,
    RelayKnowledgeService,
    crate::code::CodeIndexPlan,
    CodeIndexTaskLeaseContext,
    std::path::PathBuf,
) {
    let source = TempSourceDir::create("source-replan-ownership");
    source.write("src/a.rs", "pub fn healthy() -> u8 { 1 }\n");
    source.write("src/b.rs", "pub fn transient() -> u8 { 2 }\n");
    let environment = EnvironmentConfig::from_pairs(
        PlatformKind::current(),
        [(
            "RELAY_KNOWLEDGE_HOME",
            source.path.join("runtime").to_str().unwrap(),
        )],
    )
    .unwrap();
    let paths = RuntimePaths::resolve(&environment.platform, &environment.paths).unwrap();
    let facts_database = if partitioned {
        paths.repository_shard_database_file("repo")
    } else {
        paths.database_file()
    };
    let store: Arc<dyn KnowledgeStore> = if partitioned {
        Arc::new(PartitionedSqliteKnowledgeStore::open(paths.database_file(), paths).unwrap())
    } else {
        Arc::new(SqliteGraphStore::open(&facts_database).unwrap())
    };
    let registration = CodeRepositoryRegistration::new(
        "repo",
        "fixture",
        source.path.to_string_lossy(),
        vec!["src".into()],
        vec![],
    )
    .unwrap();
    store
        .upsert_code_repository(registration.clone())
        .await
        .unwrap();
    let service = RelayKnowledgeService::with_store(
        RuntimeConfiguration::from_environment(&environment)
            .await
            .unwrap(),
        store.clone(),
    );
    let plan = prepare_full_index_plan(
        registration,
        CodeRepositorySelector::new("fixture", "HEAD", vec![], vec![]).unwrap(),
        CodeIndexResourceBudget::new(1, 1024 * 1024, 10000).unwrap(),
    )
    .unwrap();
    let session = plan.session();
    let queued = store
        .queue_code_index_task(seed(&session, "initial"))
        .await
        .unwrap();
    let running = store
        .claim_code_index_task(CodeIndexTaskClaimRequest {
            task_id: Some(queued.task_id),
            lease_owner: "worker".into(),
            lease_duration_ms: 60000,
            max_attempts: 3,
            now_ms: crate::clock::system_now_millis().unwrap(),
        })
        .await
        .unwrap()
        .unwrap();
    let lease = CodeIndexTaskLeaseContext {
        task_id: running.task_id.clone(),
        lease_owner: "worker".into(),
        attempt_count: running.attempt_count,
        lease_duration_ms: 60000,
        publication_fence: CodeIndexPublicationFence {
            repository_id: "repo".into(),
            task_id: running.task_id,
            lease_owner: "worker".into(),
            attempt_count: running.attempt_count,
            generation: running.publication_generation,
        },
        source_scope: session.source_scope,
        resolved_commit_sha: session.resolved_commit_sha,
        tree_hash: session.tree_hash,
        path_filters: session.path_filters,
        language_filters: session.language_filters,
        resource_budget: session.resource_budget,
    };
    (source, store, service, plan, lease, facts_database)
}

#[tokio::test]
async fn source_io_replan_removes_old_checkpoint_before_queueing_new_or_restored_identity() {
    for partitioned in [false, true] {
        let (source, store, service, plan, lease, facts_database) = fixture(partitioned).await;
        let old = plan.session();
        store
            .begin_code_index_session_with_fence(old.clone(), lease.publication_fence.clone())
            .await
            .unwrap();
        let (_, first) = plan.clone().parse_next_batch().unwrap();
        store
            .apply_code_index_batch_with_fence(first.unwrap(), lease.publication_fence.clone())
            .await
            .unwrap();
        let prefix_counts =
            scope_row_counts(facts_database.clone(), old.source_scope.clone()).await;
        assert_eq!(
            prefix_counts[0].1, 1,
            "the old scope has a durably written file"
        );
        assert!(
            prefix_counts[1].1 > 0,
            "the committed prefix contains real symbols"
        );
        std::fs::remove_file(source.path.join("src/b.rs")).unwrap();
        let partial = service
            .apply_code_index_from_plan(&store, plan, Some(lease.clone()))
            .await
            .unwrap();
        assert_eq!(partial.indexed_file_count, 1);
        assert_eq!(partial.progress.io_skipped_file_count, 1);
        assert_ne!(partial.source_scope, old.source_scope);
        assert!(
            store
                .code_index_checkpoint(old.source_scope.clone())
                .await
                .unwrap()
                .is_none()
        );
        assert!(matches!(
            store
                .code_file_fingerprints_for_scope(old.source_scope.clone())
                .await,
            Err(crate::storage::StorageError::InvalidInput(_))
        ));
        for (table, count) in scope_row_counts(facts_database, old.source_scope.clone()).await {
            assert_eq!(count, 0, "old scope rows must be gone from {table}");
        }
        let mut third = old.clone();
        third.resolved_commit_sha = "filesystem:0123456789abcdef".into();
        third.tree_hash = third.resolved_commit_sha.clone();
        third.source_scope = crate::domain::code_snapshot_scope_id(
            "repo",
            &third.tree_hash,
            &third.path_filters,
            &third.language_filters,
        );
        store
            .queue_code_index_task(seed(&third, "third"))
            .await
            .expect("unpublished old checkpoint must not block another target");
        let restored = store
            .queue_code_index_task(seed(&old, "restored"))
            .await
            .expect("the restored original identity must not remain retiring");
        assert_eq!(restored.source_scope, old.source_scope);
    }
}

#[tokio::test]
async fn source_io_replan_cleanup_resumes_after_checkpoint_deletion_and_rejects_stale_fences() {
    for partitioned in [false, true] {
        let (_source, store, _service, plan, lease, _facts_database) = fixture(partitioned).await;
        let old = plan.session();
        store
            .begin_code_index_session_with_fence(old.clone(), lease.publication_fence.clone())
            .await
            .unwrap();
        let (_, first) = plan.parse_next_batch().unwrap();
        store
            .apply_code_index_batch_with_fence(first.unwrap(), lease.publication_fence.clone())
            .await
            .unwrap();
        assert!(
            !store
                .cleanup_source_replan_with_fence(
                    old.source_scope.clone(),
                    lease.publication_fence.clone(),
                    false
                )
                .await
                .unwrap()
        );
        let mut expired = lease.publication_fence.clone();
        expired.generation += 1;
        assert!(
            store
                .cleanup_source_replan_with_fence(old.source_scope.clone(), expired, true)
                .await
                .is_err()
        );
        let mut checkpoint_removed_before_completion = false;
        let mut completed = false;
        for _ in 0..100 {
            let done = store
                .cleanup_source_replan_with_fence(
                    old.source_scope.clone(),
                    lease.publication_fence.clone(),
                    true,
                )
                .await
                .unwrap();
            if done {
                completed = true;
                break;
            }
            if store
                .code_index_checkpoint(old.source_scope.clone())
                .await
                .unwrap()
                .is_none()
            {
                checkpoint_removed_before_completion = true;
            }
        }
        assert!(completed);
        assert!(
            checkpoint_removed_before_completion,
            "GC ownership must survive its checkpoint phase"
        );
        assert!(
            store
                .cleanup_source_replan_with_fence(
                    old.source_scope.clone(),
                    lease.publication_fence.clone(),
                    true
                )
                .await
                .unwrap()
        );
        store
            .begin_code_index_session_with_fence(old, lease.publication_fence)
            .await
            .expect("the same original scope can be rebuilt immediately");
    }
}

// Filesystem paths are owned by this tightly scoped, automatically cleaned test fixture.
struct TempSourceDir {
    path: std::path::PathBuf,
}

impl TempSourceDir {
    fn create(label: &str) -> Self {
        static SEQUENCE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let sequence = SEQUENCE.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("relay-{label}-{}-{sequence}", std::process::id()));
        std::fs::create_dir_all(&path).unwrap();
        Self { path }
    }

    fn write(&self, relative: &str, content: &str) {
        let path = self.path.join(relative);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, content).unwrap();
    }
}

impl Drop for TempSourceDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

// Inspect persisted ownership independently of public scope routing, which rejects retired scopes.
async fn scope_row_counts(
    database: std::path::PathBuf,
    scope: String,
) -> Vec<(&'static str, usize)> {
    tokio::task::spawn_blocking(move || {
        let connection = rusqlite::Connection::open_with_flags(
            database,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        )
        .unwrap();
        [
            "code_repository_files",
            "code_repository_symbols",
            "code_repository_references",
            "code_repository_imports",
            "code_repository_chunks",
            "code_repository_search_metadata",
            "code_repository_search",
        ]
        .into_iter()
        .map(|table| {
            let count = connection
                .query_row(
                    &format!("SELECT COUNT(*) FROM {table} WHERE source_scope = ?1"),
                    [&scope],
                    |row| row.get::<_, usize>(0),
                )
                .unwrap();
            (table, count)
        })
        .collect()
    })
    .await
    .unwrap()
}
