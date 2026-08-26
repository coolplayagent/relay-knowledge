//! Business projection participation in the fenced publication barrier.

use super::*;

#[tokio::test]
async fn fenced_full_checkpoint_waits_for_software_projection_before_becoming_fresh() {
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
    let projection = store
        .refresh_software_global_projection_with_fence(SOURCE_SCOPE.to_owned(), fence)
        .await
        .expect("software facts should complete fenced publication");
    assert!(!projection.status.stale);
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
