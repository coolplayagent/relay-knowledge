//! Business projection participation in the fenced publication barrier.

use super::*;
use crate::storage::sqlite::{code::lifecycle, software};

#[tokio::test]
async fn code_index_persistence_performance_suite_fenced_projection_resumes_between_writer_quanta()
{
    const PROJECTED_FILE_COUNT: usize = 12_000;
    let store = registered_store().await;
    let now_ms = now_millis();
    let queued = store
        .queue_code_index_task(CodeIndexTaskSeed {
            repository_id: "repo".to_owned(),
            alias: "fixture".to_owned(),
            ref_selector: "HEAD".to_owned(),
            resolved_commit_sha: "commit".to_owned(),
            tree_hash: "tree".to_owned(),
            source_scope: SOURCE_SCOPE.to_owned(),
            path_filters: Vec::new(),
            language_filters: Vec::new(),
            mode: CodeIndexMode::Full,
            input_fingerprint: "fenced-full-publication".to_owned(),
            resource_budget: Default::default(),
            payload_json: "{}".to_owned(),
            now_ms,
        })
        .await
        .expect("full task should queue");
    let running = store
        .claim_code_index_task(CodeIndexTaskClaimRequest {
            task_id: Some(queued.task_id),
            lease_owner: LEASE_OWNER.to_owned(),
            lease_duration_ms: 60_000,
            max_attempts: 3,
            now_ms: now_millis(),
        })
        .await
        .expect("full task should claim")
        .expect("queued task should be claimable");
    let fence = CodeIndexPublicationFence {
        repository_id: running.repository_id.clone(),
        task_id: running.task_id.clone(),
        lease_owner: LEASE_OWNER.to_owned(),
        attempt_count: running.attempt_count,
        generation: running.publication_generation,
    };
    let session = session_for_scope(SOURCE_SCOPE, 0);
    store
        .begin_code_index_session_with_fence(session.clone(), fence.clone())
        .await
        .expect("fenced full session should begin");
    store
        .finalize_code_index_session_with_fence(session, fence.clone())
        .await
        .expect("fenced code facts should stage");
    store
        .run(|connection| {
            connection.execute(
                "
                WITH digits(value) AS (
                    VALUES (0), (1), (2), (3), (4), (5), (6), (7), (8), (9)
                ), generated(value) AS (
                    SELECT a.value + 10 * b.value + 100 * c.value +
                           1000 * d.value + 10000 * e.value
                    FROM digits a, digits b, digits c, digits d, digits e
                )
                INSERT INTO code_repository_files (
                    repository_id, source_scope, file_id, path, language_id,
                    blob_hash, byte_len, line_count, parse_status, is_generated
                )
                SELECT 'repo', ?1, printf('large-file-%05d', value),
                       printf('src/generated/file_%05d.rs', value), 'rust',
                       printf('blob-%05d', value), 32, 1, 'parsed', 0
                FROM generated
                WHERE value < ?2
                ",
                rusqlite::params![SOURCE_SCOPE, PROJECTED_FILE_COUNT],
            )?;
            connection.execute(
                "
                WITH digits(value) AS (
                    VALUES (0), (1), (2), (3), (4), (5), (6), (7), (8), (9)
                ), generated(value) AS (
                    SELECT a.value + 10 * b.value + 100 * c.value +
                           1000 * d.value + 10000 * e.value
                    FROM digits a, digits b, digits c, digits d, digits e
                )
                INSERT INTO code_repository_imports (
                    repository_id, source_scope, import_id, file_id, path, module,
                    target_hint, resolution_state, confidence_basis_points,
                    confidence_tier, line_start, line_end
                )
                SELECT 'repo', ?1, printf('large-import-%05d', value),
                       printf('large-file-%05d', value),
                       printf('src/generated/file_%05d.rs', value),
                       printf('external_sdk_%05d', value),
                       printf('external_sdk_%05d', value), 'external', 7000,
                       'inferred', 1, 1
                FROM generated
                WHERE value < ?2
                ",
                rusqlite::params![SOURCE_SCOPE, PROJECTED_FILE_COUNT],
            )?;
            Ok(())
        })
        .await
        .expect("large staged file surface should seed");
    let staged_checkpoint = store
        .code_index_checkpoint(SOURCE_SCOPE.to_owned())
        .await
        .expect("checkpoint should load")
        .expect("checkpoint should exist");
    assert_eq!(staged_checkpoint.state, "finalizing:software_projection");
    let staged_status = store
        .code_repository_status("fixture".to_owned())
        .await
        .expect("repository status should load")
        .expect("repository should exist");
    assert_eq!(staged_status.state, "indexing");
    assert!(staged_status.stale);
    crate::storage::stage_empty_business_projection_with_fence_for_test(
        &store,
        running.repository_id.clone(),
        SOURCE_SCOPE.to_owned(),
        running.resolved_commit_sha.clone(),
        fence.clone(),
    )
    .await
    .expect("business facts should stage before fenced publication");
    let reset_fence = fence.clone();
    let reset = store
        .run(move |connection| {
            let guard = lifecycle::publication_fence::prepare_guard(connection, reset_fence, None)?;
            software::advance_fenced_projection(connection, SOURCE_SCOPE, &guard)
        })
        .await
        .expect("reset should commit as one durable writer phase");
    assert!(matches!(
        reset,
        software::FencedProjectionAdvance::Pending { checkpoint_state }
            if checkpoint_state == "finalizing:software_projection:v1:dependencies"
    ));
    let dependency_fence = fence.clone();
    let dependencies = store
        .run(move |connection| {
            let guard =
                lifecycle::publication_fence::prepare_guard(connection, dependency_fence, None)?;
            software::advance_fenced_projection(connection, SOURCE_SCOPE, &guard)
        })
        .await
        .expect("dependencies should commit as a second durable writer phase");
    assert!(matches!(
        dependencies,
        software::FencedProjectionAdvance::Pending { checkpoint_state }
            if checkpoint_state == "finalizing:software_projection:v1:sdk_usages"
    ));
    let resumed_checkpoint = store
        .code_index_checkpoint(SOURCE_SCOPE.to_owned())
        .await
        .expect("resumable projection checkpoint should load")
        .expect("resumable projection checkpoint should exist");
    assert_eq!(
        resumed_checkpoint.state,
        "finalizing:software_projection:v1:sdk_usages"
    );
    let projection = store
        .refresh_software_global_projection_with_fence(SOURCE_SCOPE.to_owned(), fence)
        .await
        .expect("software facts should complete fenced publication");
    assert!(!projection.status.stale);
    assert_eq!(projection.status.file_count, PROJECTED_FILE_COUNT);
    assert_eq!(projection.status.sdk_usage_count, PROJECTED_FILE_COUNT);
    assert_eq!(projection.status.relationship_count, PROJECTED_FILE_COUNT);
    let completed_checkpoint = store
        .code_index_checkpoint(SOURCE_SCOPE.to_owned())
        .await
        .expect("checkpoint should load")
        .expect("checkpoint should exist");
    assert_eq!(completed_checkpoint.state, "completed");
    let published_status = store
        .code_repository_status("fixture".to_owned())
        .await
        .expect("repository status should load")
        .expect("repository should exist");
    assert_eq!(published_status.state, "fresh");
    assert!(!published_status.stale);
    assert_eq!(
        published_status.last_indexed_scope_id.as_deref(),
        Some(SOURCE_SCOPE)
    );
    let active = store
        .active_code_index_task("repo".to_owned())
        .await
        .expect("active task should load")
        .expect("worker completes the task after the publication response");
    assert_eq!(active.state, CodeIndexTaskState::Running);
}
