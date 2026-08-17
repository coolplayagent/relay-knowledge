use std::{
    fs,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::{SystemTime, UNIX_EPOCH},
};

// Unit contract for partitioned SQLite routing, control state, and checkpoints.

use super::*;
use crate::{
    domain::{
        CodeIndexMode, CodeIndexPublicationFence, CodeIndexResourceBudget, CodeParseStatus,
        CodeRepositorySelector, FreshnessPolicy, RepositoryCodeChunkRecord,
        RepositoryCodeFileRecord, RepositoryCodeRange, SoftwareGlobalKind,
    },
    env::{EnvironmentConfig, PlatformKind},
    storage::CodeRepositorySetRefreshPublication,
};

static TEMP_DIR_COUNTER: AtomicU64 = AtomicU64::new(0);

#[tokio::test]
async fn code_index_task_partitioned_takeover_fences_stale_shard_snapshot_publication() {
    let store = partitioned_store("publication-fence-takeover");
    let now_ms = u64::try_from(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock should be after epoch")
            .as_millis(),
    )
    .unwrap_or(u64::MAX);
    store
        .upsert_code_repository(registration())
        .await
        .expect("repository should register");
    let queued = store
        .queue_code_index_task(task_seed("scope-fenced-new"))
        .await
        .expect("task should queue");
    let first = store
        .claim_code_index_task(CodeIndexTaskClaimRequest {
            task_id: Some(queued.task_id.clone()),
            lease_owner: "worker-old".to_owned(),
            lease_duration_ms: 60_000,
            max_attempts: 3,
            now_ms,
        })
        .await
        .expect("first claim should run")
        .expect("first attempt should claim");
    let shard = store
        .catalog
        .staged_repository_store("repo".to_owned())
        .await
        .expect("repository shard should open");
    store
        .catalog
        .import_control_repository(Arc::clone(&shard), "repo".to_owned(), None)
        .await
        .expect("control repository should import into the shard");
    shard
        .apply_code_index_snapshot_with_fence(
            snapshot("scope-fenced-new"),
            publication_fence(&first, "worker-old"),
        )
        .await
        .expect("first attempt should commit its shard while its lease is live");
    let second = store
        .claim_code_index_task(CodeIndexTaskClaimRequest {
            task_id: Some(queued.task_id),
            lease_owner: "worker-new".to_owned(),
            lease_duration_ms: 60_000,
            max_attempts: 3,
            now_ms: now_ms.saturating_add(60_000),
        })
        .await
        .expect("takeover claim should run")
        .expect("expired task should be reclaimed");

    let shard_error = shard
        .apply_code_index_snapshot_with_fence(
            snapshot("scope-fenced-new"),
            publication_fence(&first, "worker-old"),
        )
        .await
        .expect_err("stale shard writer must be fenced");
    assert!(matches!(shard_error, StorageError::InvalidInput(_)));
    let stage_error = store
        .catalog
        .stage_scope_with_fence(
            "repo".to_owned(),
            "scope-fenced-new".to_owned(),
            publication_fence(&first, "worker-old"),
        )
        .await
        .expect_err("stale writer must not stage catalog routing after takeover");
    assert!(matches!(stage_error, StorageError::InvalidInput(_)));
    let catalog_error = store
        .catalog
        .record_scope_with_fence(
            "repo".to_owned(),
            "scope-fenced-new".to_owned(),
            publication_fence(&first, "worker-old"),
        )
        .await
        .expect_err("stale writer must not advance the catalog after shard commit");
    assert!(matches!(catalog_error, StorageError::InvalidInput(_)));
    assert!(
        store
            .catalog
            .repository_for_scope("scope-fenced-new".to_owned())
            .await
            .expect("catalog scope should load")
            .is_none()
    );
    store
        .apply_code_index_snapshot_with_fence(
            snapshot("scope-fenced-new"),
            publication_fence(&second, "worker-new"),
        )
        .await
        .expect("current shard writer should publish and mirror control status");

    let status = store
        .code_repository_status("fixture".to_owned())
        .await
        .expect("status should load")
        .expect("repository should exist");
    assert_eq!(
        status.last_indexed_scope_id.as_deref(),
        Some("scope-fenced-new")
    );
}

#[tokio::test]
async fn empty_partitioned_store_delegates_control_defaults_explicitly() {
    let store = partitioned_store("empty-contract");

    assert!(
        store
            .code_repository_status("missing".to_owned())
            .await
            .expect("missing status should load")
            .is_none()
    );
    assert!(
        store
            .list_code_repositories()
            .await
            .expect("empty repository list should load")
            .is_empty()
    );
    assert!(
        store
            .latest_code_repository_scope_status("missing".to_owned(), Vec::new(), Vec::new())
            .await
            .expect("missing latest scope should load")
            .is_none()
    );
    assert!(
        store
            .latest_code_index_checkpoint("missing".to_owned())
            .await
            .expect("missing checkpoint should load")
            .is_none()
    );
    assert!(
        store
            .active_code_index_task("missing".to_owned())
            .await
            .expect("missing active task should load")
            .is_none()
    );
    assert!(
        store
            .code_index_task("missing".to_owned())
            .await
            .expect("missing task should load")
            .is_none()
    );
    assert!(
        store
            .running_code_index_task_leases()
            .await
            .expect("empty leases should load")
            .is_empty()
    );
    assert!(
        store
            .code_file_fingerprints("missing".to_owned())
            .await
            .expect("missing fingerprints should load")
            .is_empty()
    );
    assert!(
        store
            .code_repository_set("missing".to_owned())
            .await
            .expect("missing set should load")
            .is_none()
    );
    assert!(
        store
            .code_repository_set_status("missing".to_owned())
            .await
            .expect("missing set status should load")
            .is_none()
    );
    assert!(
        store
            .code_repository_set_cross_edges("missing".to_owned())
            .await
            .expect("missing set edges should load")
            .is_empty()
    );
    assert!(
        store
            .claim_code_repository_set_refresh_task(CodeRepositorySetRefreshTaskClaimRequest {
                task_id: None,
                lease_owner: "worker".to_owned(),
                lease_duration_ms: 10,
                max_attempts: 1,
                now_ms: 1,
            })
            .await
            .expect("empty set refresh queue should load")
            .is_none()
    );

    store
        .recover_code_index_task_leases(1, 1)
        .await
        .expect("empty lease recovery should succeed");
    assert_eq!(
        store
            .recover_code_index_task_leases_by_task(CodeIndexTaskLeaseRecovery {
                task_ids: vec!["missing".to_owned()],
                now_ms: 1,
                max_attempts: 1,
                error_kind: "lease".to_owned(),
                error_message: "expired".to_owned(),
            })
            .await
            .expect("targeted empty recovery should succeed"),
        0
    );
    assert!(
        store
            .reset_code_index_tasks("missing".to_owned(), 1)
            .await
            .expect("empty reset should succeed")
            .is_empty()
    );
    store
        .clear_code_workspace_state("missing".to_owned(), "missing-scope".to_owned())
        .await
        .expect("empty workspace cleanup should succeed");

    assert!(
        store
            .refresh_code_repository_set_overlay(
                "missing".to_owned(),
                CodeRepositorySetRefreshPublication {
                    task_id: "missing".to_owned(),
                    set_id: "missing".to_owned(),
                    lease_owner: "worker".to_owned(),
                    attempt_count: 1,
                    member_replacements: Vec::new(),
                },
            )
            .await
            .expect_err("partitioned overlay refresh should be explicit")
            .to_string()
            .contains("single_sqlite")
    );
    let completion_error = store
        .complete_code_repository_set_refresh_task(CodeRepositorySetRefreshTaskCompletion {
            task_id: "missing".to_owned(),
            lease_owner: "worker".to_owned(),
            attempt_count: 1,
            now_ms: 1,
        })
        .await
        .expect_err("missing refresh completion should fail");
    assert!(matches!(completion_error, StorageError::InvalidInput(_)));
    let failure_error = store
        .fail_code_repository_set_refresh_task(CodeRepositorySetRefreshTaskFailure {
            task_id: "missing".to_owned(),
            lease_owner: "worker".to_owned(),
            attempt_count: 1,
            error_kind: "worker".to_owned(),
            error_message: "failed".to_owned(),
            retry_backoff_ms: 10,
            max_attempts: 1,
            now_ms: 1,
        })
        .await
        .expect_err("missing refresh failure should fail");
    assert!(matches!(failure_error, StorageError::InvalidInput(_)));
}

#[tokio::test]
async fn indexed_partition_routes_repository_capabilities_to_its_shard() {
    let store = partitioned_store("indexed-contract");
    let source_scope = "scope-indexed";
    store
        .upsert_code_repository(registration())
        .await
        .expect("repository should register");
    store
        .apply_code_index_snapshot(snapshot(source_scope))
        .await
        .expect("snapshot should apply");

    let status = store
        .code_repository_status("fixture".to_owned())
        .await
        .expect("status should load")
        .expect("repository should exist");
    assert_eq!(status.last_indexed_scope_id.as_deref(), Some(source_scope));
    assert_eq!(
        store
            .list_code_repositories()
            .await
            .expect("repository list should load")
            .len(),
        1
    );
    assert!(
        store
            .latest_code_repository_scope_status("fixture".to_owned(), Vec::new(), Vec::new())
            .await
            .expect("latest scope should load")
            .is_some()
    );
    assert!(
        store
            .latest_code_index_checkpoint("repo".to_owned())
            .await
            .expect("latest checkpoint should load")
            .is_none()
    );
    assert_eq!(
        store
            .code_scope_retention("repo".to_owned())
            .await
            .expect("retention should load")
            .repository_id,
        "repo"
    );
    assert_eq!(
        store
            .code_file_fingerprints("repo".to_owned())
            .await
            .expect("fingerprints should load")
            .len(),
        1
    );
    assert_eq!(
        store
            .code_file_candidate_paths_for_scope(
                source_scope.to_owned(),
                Vec::new(),
                Vec::new(),
                true,
                5,
            )
            .await
            .expect("scope candidates should load"),
        ["src/lib.rs"]
    );
    assert_eq!(
        store
            .code_file_candidate_paths_for_query_scope(
                source_scope.to_owned(),
                "indexed contract".to_owned(),
                Vec::new(),
                Vec::new(),
                true,
                5,
            )
            .await
            .expect("query candidates should load"),
        ["src/lib.rs"]
    );
    assert!(
        store
            .search_code_feature_flags(feature_flag_request())
            .await
            .expect("feature flags should route")
            .is_empty()
    );
    assert!(
        store
            .analyze_code_impact(impact_request(), CodeImpactChanges::default())
            .await
            .expect("impact should route")
            .is_empty()
    );
    assert_eq!(
        store
            .code_repository_scope_symbol_generation_counts(source_scope.to_owned())
            .await
            .expect("generation counts should load")
            .handwritten_symbol_count,
        0
    );
    store
        .refresh_software_global_projection(source_scope.to_owned())
        .await
        .expect("software projection should refresh");
    assert_eq!(
        store
            .software_global_projection(software_request())
            .await
            .expect("software projection should route")
            .status
            .source_scope,
        source_scope
    );
    store
        .clear_code_workspace_state("repo".to_owned(), source_scope.to_owned())
        .await
        .expect("workspace state should clear in both stores");
}

#[tokio::test]
async fn partitioned_control_plane_delegates_tasks_and_repository_sets() {
    let store = partitioned_store("control-contract");
    let source_scope = "scope-control";
    store
        .upsert_code_repository(registration())
        .await
        .expect("repository should register");
    store
        .apply_code_index_snapshot(snapshot(source_scope))
        .await
        .expect("snapshot should apply");

    let queued = store
        .queue_code_index_task(task_seed(source_scope))
        .await
        .expect("task should queue");
    assert!(
        store
            .active_code_index_task("repo".to_owned())
            .await
            .expect("active task should load")
            .is_some()
    );
    assert!(
        store
            .code_index_task(queued.task_id.clone())
            .await
            .expect("task should load")
            .is_some()
    );
    let claimed = store
        .claim_code_index_task(CodeIndexTaskClaimRequest {
            task_id: Some(queued.task_id.clone()),
            lease_owner: "worker".to_owned(),
            lease_duration_ms: 100,
            max_attempts: 3,
            now_ms: 2,
        })
        .await
        .expect("claim should run")
        .expect("task should claim");
    assert_eq!(
        store
            .running_code_index_task_leases()
            .await
            .expect("running leases should load")
            .len(),
        1
    );
    store
        .renew_code_index_task_lease(CodeIndexTaskLeaseRenewal {
            task_id: claimed.task_id.clone(),
            lease_owner: "worker".to_owned(),
            attempt_count: claimed.attempt_count,
            lease_duration_ms: 100,
            now_ms: 3,
        })
        .await
        .expect("lease should renew");
    assert_eq!(
        store
            .recover_code_index_task_leases_by_task(CodeIndexTaskLeaseRecovery {
                task_ids: vec![claimed.task_id.clone()],
                now_ms: 4,
                max_attempts: 3,
                error_kind: "lease".to_owned(),
                error_message: "orphaned".to_owned(),
            })
            .await
            .expect("targeted recovery should run"),
        1
    );
    assert_eq!(
        store
            .reset_code_index_tasks("repo".to_owned(), 5)
            .await
            .expect("tasks should reset")
            .len(),
        1
    );
    assert!(
        store
            .complete_code_index_task(CodeIndexTaskCompletion {
                task_id: "missing".to_owned(),
                lease_owner: "worker".to_owned(),
                attempt_count: 1,
                now_ms: 6,
            })
            .await
            .expect_err("missing completion should fail")
            .to_string()
            .contains("missing")
    );
    assert!(
        store
            .fail_code_index_task(CodeIndexTaskFailure {
                task_id: "missing".to_owned(),
                lease_owner: "worker".to_owned(),
                attempt_count: 1,
                error_kind: "worker".to_owned(),
                error_message: "failed".to_owned(),
                retry_backoff_ms: 10,
                max_attempts: 1,
                now_ms: 6,
            })
            .await
            .expect_err("missing failure should fail")
            .to_string()
            .contains("missing")
    );

    let set = store
        .create_code_repository_set(CodeRepositorySetSeed {
            alias: "workspace".to_owned(),
            description: Some("contract".to_owned()),
            default_ref_policy_json: "{}".to_owned(),
            now_ms: 10,
        })
        .await
        .expect("repository set should create");
    let member = store
        .add_code_repository_set_member(CodeRepositorySetMemberSeed {
            set_alias: "workspace".to_owned(),
            repository_id: "repo".to_owned(),
            repository_alias: "fixture".to_owned(),
            ref_selector: "HEAD".to_owned(),
            resolved_commit_sha: "commit".to_owned(),
            source_scope: source_scope.to_owned(),
            path_filters: Vec::new(),
            language_filters: Vec::new(),
            priority: 0,
        })
        .await
        .expect("repository set member should add");
    assert!(
        store
            .code_repository_set("workspace".to_owned())
            .await
            .expect("repository set should load")
            .is_some()
    );
    assert!(
        store
            .code_repository_set_status("workspace".to_owned())
            .await
            .expect("repository set status should load")
            .is_some()
    );
    assert!(
        store
            .code_repository_set_cross_edges(set.set_id)
            .await
            .expect("repository set edges should load")
            .is_empty()
    );
    store
        .remove_code_repository_set_member("workspace".to_owned(), member.repository_alias)
        .await
        .expect("repository set member should remove");
}

#[tokio::test]
async fn partitioned_checkpoint_lifecycle_finishes_in_staged_shard() {
    let store = partitioned_store("checkpoint-contract");
    store
        .upsert_code_repository(registration())
        .await
        .expect("repository should register");
    let snapshot = snapshot("scope-checkpoint");
    let session = session_from_snapshot(&snapshot);

    store
        .begin_code_index_session(session.clone())
        .await
        .expect("session should begin");
    store
        .apply_code_index_batch(batch_from_snapshot(snapshot))
        .await
        .expect("batch should apply");
    let summary = store
        .finalize_code_index_session(session)
        .await
        .expect("session should finalize");

    assert_eq!(summary.source_scope, "scope-checkpoint");
}

#[tokio::test]
async fn code_index_task_partitioned_catalog_route_holds_scope_slot_until_final_release() {
    let store = partitioned_store("retention-catalog-capacity-reservation");
    store
        .upsert_code_repository(registration())
        .await
        .expect("repository should register");
    store
        .control
        .run(|connection| {
            for index in 0..crate::storage::sqlite::code::MAX_SCOPE_SLOTS_PER_REPOSITORY - 1 {
                connection.execute(
                    "INSERT INTO code_repository_scopes (
                         source_scope, repository_id, resolved_commit_sha, tree_hash,
                         path_filters_json, language_filters_json, indexed_file_count,
                         symbol_count, reference_count, chunk_count, stale, degraded_reason
                     ) VALUES (?1, 'repo', ?2, ?3, '[]', '[]', 0, 0, 0, 0, 0, NULL)",
                    rusqlite::params![
                        format!("scope-filler-{index:03}"),
                        format!("commit-filler-{index:03}"),
                        format!("tree-filler-{index:03}"),
                    ],
                )?;
            }
            Ok(())
        })
        .await
        .expect("published scope fillers should persist");
    store
        .catalog
        .stage_scope("repo".to_owned(), "scope-retiring-route".to_owned())
        .await
        .expect("catalog route should reserve the final scope slot");

    let error = store
        .queue_code_index_task(task_seed("scope-next"))
        .await
        .expect_err("a retained catalog route must keep the next target behind backpressure");
    assert!(matches!(error, StorageError::CapacityExceeded(_)));

    let removed = store
        .catalog
        .remove_scope_route("repo".to_owned(), "scope-retiring-route".to_owned())
        .await
        .expect("final-phase coordinator should release the catalog route");
    assert_eq!(removed, 1);
    let queued = store
        .queue_code_index_task(task_seed("scope-next"))
        .await
        .expect("the released final slot should admit the next target");
    assert_eq!(queued.source_scope, "scope-next");
}

#[tokio::test]
async fn code_index_task_partitioned_retention_removes_retired_scope_catalog_route() {
    let store = partitioned_store("retention-catalog-route");
    store
        .upsert_code_repository(registration())
        .await
        .expect("repository should register");
    store
        .apply_code_index_snapshot(snapshot("scope-old"))
        .await
        .expect("old snapshot should apply");
    store
        .apply_code_index_snapshot(snapshot("scope-active"))
        .await
        .expect("active snapshot should apply");
    assert_eq!(
        store
            .catalog
            .repository_for_scope("scope-old".to_owned())
            .await
            .expect("old catalog route should load")
            .as_deref(),
        Some("repo")
    );

    for _ in 0..160 {
        let pass = store
            .prune_code_repository_scopes(CodeScopeRetentionRequest {
                repository_id: "repo".to_owned(),
                active_scope: "scope-active".to_owned(),
                retain_recent_successful_scopes: 0,
                repository_retention_cutoff_ms: None,
                repository_retention_initial_scope: None,
            })
            .await
            .expect("partitioned retention pass should run");
        if !pass.maintenance_pending {
            break;
        }
    }

    assert!(
        store
            .catalog
            .repository_for_scope("scope-old".to_owned())
            .await
            .expect("retired catalog route should load")
            .is_none()
    );
    assert!(
        store
            .catalog
            .repository_for_scope("scope-active".to_owned())
            .await
            .expect("active catalog route should load")
            .is_some()
    );
}

#[tokio::test]
async fn partitioned_repository_retention_drains_control_and_shard_before_completion() {
    let store = partitioned_store("repository-retention");
    store
        .upsert_code_repository(registration())
        .await
        .expect("repository should register");
    store
        .apply_code_index_snapshot(snapshot("scope-active"))
        .await
        .expect("active snapshot should apply");
    store
        .control
        .run(|connection| {
            connection.execute(
                "INSERT INTO code_repository_retention_jobs (
                     repository_id, initial_scope, cutoff_ms, phase,
                     created_at_ms, updated_at_ms, last_error
                 ) VALUES ('repo', 'scope-active', 100, 'retiring_scopes', 100, 100, NULL)",
                [],
            )?;
            Ok(())
        })
        .await
        .expect("repository retention job should insert");

    for _ in 0..160 {
        let pass = store
            .prune_code_repository_scopes(CodeScopeRetentionRequest {
                repository_id: "repo".to_owned(),
                active_scope: "scope-active".to_owned(),
                retain_recent_successful_scopes: 2,
                repository_retention_cutoff_ms: None,
                repository_retention_initial_scope: None,
            })
            .await
            .expect("partitioned repository retention should advance");
        if !pass.maintenance_pending {
            break;
        }
    }

    assert!(
        store
            .catalog
            .repository_for_scope("scope-active".to_owned())
            .await
            .expect("scope route should load")
            .is_none()
    );
    let status = store
        .code_repository_status("fixture".to_owned())
        .await
        .expect("repository status should load")
        .expect("repository registration should remain");
    assert!(status.last_indexed_scope_id.is_none());
    assert_eq!(status.state, "registered");
    assert!(
        store
            .control
            .code_scope_retention("repo".to_owned())
            .await
            .expect("control retention status should load")
            .repository_retention_job
            .is_none()
    );
}

#[tokio::test]
async fn code_index_task_partitioned_retention_cleans_staged_partial_scope_route() {
    let store = partitioned_store("retention-staged-partial-route");
    store
        .upsert_code_repository(registration())
        .await
        .expect("repository should register");
    let partial = snapshot("scope-partial");
    store
        .begin_code_index_session(session_from_snapshot(&partial))
        .await
        .expect("partial session should begin");
    store
        .apply_code_index_batch(batch_from_snapshot(partial))
        .await
        .expect("partial batch should persist");
    assert_eq!(
        store
            .catalog
            .repository_for_scope("scope-partial".to_owned())
            .await
            .expect("staged route should load")
            .as_deref(),
        Some("repo")
    );

    for _ in 0..80 {
        let pass = store
            .prune_code_repository_scopes(CodeScopeRetentionRequest {
                repository_id: "repo".to_owned(),
                active_scope: String::new(),
                retain_recent_successful_scopes: 2,
                repository_retention_cutoff_ms: None,
                repository_retention_initial_scope: None,
            })
            .await
            .expect("staged partial retention should run");
        if !pass.maintenance_pending {
            break;
        }
    }

    assert!(
        store
            .catalog
            .repository_for_scope("scope-partial".to_owned())
            .await
            .expect("retired staged route should load")
            .is_none()
    );
    assert!(
        store
            .code_index_checkpoint("scope-partial".to_owned())
            .await
            .expect("retired partial checkpoint should query")
            .is_none()
    );
}

fn partitioned_store(name: &str) -> PartitionedSqliteKnowledgeStore {
    let root = unique_temp_dir(name);
    let environment = EnvironmentConfig::from_pairs(
        PlatformKind::current(),
        [(
            "RELAY_KNOWLEDGE_HOME",
            root.to_str().expect("temp root should be UTF-8"),
        )],
    )
    .expect("environment should parse");
    let paths =
        RuntimePaths::resolve(&environment.platform, &environment.paths).expect("paths resolve");
    PartitionedSqliteKnowledgeStore::open(paths.database_file(), paths).expect("store should open")
}

fn unique_temp_dir(name: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock should be after epoch")
        .as_nanos();
    let sequence = TEMP_DIR_COUNTER.fetch_add(1, Ordering::Relaxed);
    let path = std::env::temp_dir().join(format!(
        "relay-knowledge-partitioned-{name}-{}-{nanos}-{sequence}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&path);
    path
}

fn registration() -> CodeRepositoryRegistration {
    CodeRepositoryRegistration::new("repo", "fixture", "/tmp/fixture", Vec::new(), Vec::new())
        .expect("registration should validate")
}

fn selector() -> CodeRepositorySelector {
    CodeRepositorySelector::new("fixture", "commit", Vec::new(), Vec::new())
        .expect("selector should validate")
}

fn feature_flag_request() -> CodeFeatureFlagRequest {
    CodeFeatureFlagRequest::new(None, selector(), 5, FreshnessPolicy::AllowStale)
        .expect("feature flag request should validate")
}

fn impact_request() -> crate::domain::CodeImpactRequest {
    crate::domain::CodeImpactRequest::new(selector(), "base", "commit", 5)
        .expect("impact request should validate")
}

fn software_request() -> SoftwareGlobalRequest {
    SoftwareGlobalRequest::new(
        selector(),
        SoftwareGlobalKind::All,
        FreshnessPolicy::AllowStale,
        5,
    )
    .expect("software request should validate")
}

fn task_seed(source_scope: &str) -> crate::storage::CodeIndexTaskSeed {
    crate::storage::CodeIndexTaskSeed {
        repository_id: "repo".to_owned(),
        alias: "fixture".to_owned(),
        ref_selector: "HEAD".to_owned(),
        resolved_commit_sha: "commit".to_owned(),
        tree_hash: "tree".to_owned(),
        source_scope: source_scope.to_owned(),
        path_filters: Vec::new(),
        language_filters: Vec::new(),
        mode: CodeIndexMode::Full,
        input_fingerprint: "partitioned-contract".to_owned(),
        resource_budget: CodeIndexResourceBudget::default(),
        payload_json: "{}".to_owned(),
        now_ms: 1,
    }
}

fn publication_fence(
    task: &crate::domain::CodeIndexTaskRecord,
    lease_owner: &str,
) -> CodeIndexPublicationFence {
    CodeIndexPublicationFence {
        repository_id: task.repository_id.clone(),
        task_id: task.task_id.clone(),
        lease_owner: lease_owner.to_owned(),
        attempt_count: task.attempt_count,
        generation: task.publication_generation,
    }
}

fn snapshot(source_scope: &str) -> CodeIndexSnapshot {
    let file = RepositoryCodeFileRecord {
        repository_id: "repo".to_owned(),
        source_scope: source_scope.to_owned(),
        file_id: "file".to_owned(),
        path: "src/lib.rs".to_owned(),
        language_id: "rust".to_owned(),
        blob_hash: "hash".to_owned(),
        byte_len: 16,
        line_count: 1,
        parse_status: CodeParseStatus::Parsed,
        is_generated: false,
        degraded_reason: None,
    };
    let chunk = RepositoryCodeChunkRecord {
        repository_id: "repo".to_owned(),
        source_scope: source_scope.to_owned(),
        chunk_id: "chunk".to_owned(),
        file_id: file.file_id.clone(),
        path: file.path.clone(),
        language_id: file.language_id.clone(),
        content: "indexed contract".to_owned(),
        byte_range: RepositoryCodeRange { start: 0, end: 16 },
        line_range: RepositoryCodeRange { start: 1, end: 1 },
        symbol_snapshot_id: None,
    };

    CodeIndexSnapshot {
        repository_id: "repo".to_owned(),
        source_scope: source_scope.to_owned(),
        base_resolved_commit_sha: None,
        resolved_commit_sha: "commit".to_owned(),
        tree_hash: "tree".to_owned(),
        path_filters: Vec::new(),
        language_filters: Vec::new(),
        full_replace: true,
        changed_path_count: 1,
        skipped_unchanged_count: 0,
        deleted_paths: Vec::new(),
        tombstones: Vec::new(),
        files: vec![file],
        symbols: Vec::new(),
        references: Vec::new(),
        imports: Vec::new(),
        calls: Vec::new(),
        dependencies: Vec::new(),
        feature_flags: Vec::new(),
        routes: Vec::new(),
        chunks: vec![chunk],
        workspaces: Vec::new(),
        diagnostics: Vec::new(),
    }
}

fn session_from_snapshot(snapshot: &CodeIndexSnapshot) -> CodeIndexSession {
    CodeIndexSession {
        repository_id: snapshot.repository_id.clone(),
        source_scope: snapshot.source_scope.clone(),
        base_resolved_commit_sha: None,
        resolved_commit_sha: snapshot.resolved_commit_sha.clone(),
        tree_hash: snapshot.tree_hash.clone(),
        path_filters: Vec::new(),
        language_filters: Vec::new(),
        full_replace: true,
        total_path_count: 1,
        changed_path_count: 1,
        skipped_unchanged_count: 0,
        deleted_paths: Vec::new(),
        tombstones: Vec::new(),
        workspaces: Vec::new(),
        resource_budget: CodeIndexResourceBudget::default(),
    }
}

fn batch_from_snapshot(snapshot: CodeIndexSnapshot) -> CodeIndexBatch {
    CodeIndexBatch {
        repository_id: snapshot.repository_id,
        source_scope: snapshot.source_scope,
        batch_index: 0,
        parsed_byte_count: snapshot.files.iter().map(|file| file.byte_len).sum(),
        files: snapshot.files,
        symbols: snapshot.symbols,
        references: snapshot.references,
        imports: snapshot.imports,
        dependencies: snapshot.dependencies,
        feature_flags: snapshot.feature_flags,
        routes: snapshot.routes,
        chunks: snapshot.chunks,
        diagnostics: snapshot.diagnostics,
    }
}
