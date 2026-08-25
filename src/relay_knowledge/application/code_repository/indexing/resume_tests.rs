use super::{
    CodeIndexTaskLeaseContext, checkpoint_skips_parser, incremental_snapshot_matches_lease,
    should_resume_staged_full,
};
use crate::domain::{
    CodeIncrementalClonePhase, CodeIndexMode, CodeIndexPublicationFence, CodeIndexResourceBudget,
    CodeIndexSnapshot, CodeQueryIndexRepairResumePhase, CodeReferenceSearchRebuildStage,
    code_incremental_clone_state, code_query_index_repair_state, code_reference_resolution,
    code_reference_resolution_query_index_repair_state, code_reference_resolution_state,
    code_reference_search_query_index_repair_state, code_reference_search_rebuild,
    code_reference_search_rebuild_state,
};

#[test]
fn crashed_incremental_checkpoint_resumes_the_durable_full_pipeline() {
    let incremental = incremental_mode();

    assert!(should_resume_staged_full(
        &incremental,
        true,
        Some("finalizing:software_projection")
    ));
    assert!(!should_resume_staged_full(
        &incremental,
        false,
        Some("finalizing:software_projection")
    ));
    assert!(!should_resume_staged_full(&incremental, true, None));
    assert!(!should_resume_staged_full(
        &CodeIndexMode::WorktreeOverlay,
        true,
        Some("finalizing:software_projection")
    ));
}

#[test]
fn crashed_incremental_clone_rebuilds_the_bounded_delta_instead_of_starting_full_index() {
    let state = code_incremental_clone_state(
        CodeIncrementalClonePhase::Search,
        11,
        23,
        40_000,
        "0123456789abcdef",
    )
    .expect("clone checkpoint");

    assert!(!should_resume_staged_full(
        &incremental_mode(),
        true,
        Some(&state)
    ));
    assert!(!checkpoint_skips_parser(&state));
}

#[test]
fn typed_fallback_requires_clean_mode_and_exact_fenced_target() {
    let snapshot = snapshot();
    let lease = matching_lease(&snapshot);

    assert!(incremental_snapshot_matches_lease(
        &incremental_mode(),
        &snapshot,
        Some(&lease)
    ));
    assert!(!incremental_snapshot_matches_lease(
        &CodeIndexMode::WorktreeOverlay,
        &snapshot,
        Some(&lease)
    ));
    assert!(!incremental_snapshot_matches_lease(
        &incremental_mode(),
        &snapshot,
        None
    ));
    let mut mismatched = lease;
    mismatched.source_scope = "other-scope".to_owned();
    assert!(!incremental_snapshot_matches_lease(
        &incremental_mode(),
        &snapshot,
        Some(&mismatched)
    ));
}

fn incremental_mode() -> CodeIndexMode {
    CodeIndexMode::Incremental {
        base_ref: "base".to_owned(),
        head_ref: "head".to_owned(),
    }
}

fn matching_lease(snapshot: &crate::domain::CodeIndexSnapshot) -> CodeIndexTaskLeaseContext {
    CodeIndexTaskLeaseContext {
        task_id: "task".to_owned(),
        lease_owner: "worker".to_owned(),
        attempt_count: 1,
        lease_duration_ms: 60_000,
        publication_fence: CodeIndexPublicationFence {
            repository_id: snapshot.repository_id.clone(),
            task_id: "task".to_owned(),
            lease_owner: "worker".to_owned(),
            attempt_count: 1,
            generation: 1,
        },
        source_scope: snapshot.source_scope.clone(),
        resolved_commit_sha: snapshot.resolved_commit_sha.clone(),
        tree_hash: snapshot.tree_hash.clone(),
        path_filters: snapshot.path_filters.clone(),
        language_filters: snapshot.language_filters.clone(),
        resource_budget: CodeIndexResourceBudget::default(),
    }
}

fn snapshot() -> CodeIndexSnapshot {
    CodeIndexSnapshot {
        repository_id: "repo".to_owned(),
        source_scope: "scope".to_owned(),
        base_resolved_commit_sha: Some("base".to_owned()),
        resolved_commit_sha: "head".to_owned(),
        tree_hash: "tree".to_owned(),
        path_filters: Vec::new(),
        language_filters: Vec::new(),
        full_replace: false,
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
        routes: Vec::new(),
        chunks: Vec::new(),
        workspaces: Vec::new(),
        diagnostics: Vec::new(),
    }
}

#[test]
fn durable_query_index_repairs_skip_parser_restart_for_every_resume_phase() {
    for resume_phase in CodeQueryIndexRepairResumePhase::ALL {
        let state = code_query_index_repair_state(16, resume_phase)
            .expect("bounded repair token should format");
        assert!(checkpoint_skips_parser(&state), "state={state}");
    }

    assert!(!checkpoint_skips_parser("indexing"));
    assert!(!checkpoint_skips_parser("finalizing:resolve_imports"));
    assert!(checkpoint_skips_parser("finalizing:software_projection"));
    assert!(checkpoint_skips_parser("finalizing:partitioned_publish"));
    assert!(checkpoint_skips_parser("completed"));
    for stage in [
        CodeReferenceSearchRebuildStage::Cleanup,
        CodeReferenceSearchRebuildStage::Build,
    ] {
        let progress = code_reference_search_rebuild_state(stage, 7);
        assert!(checkpoint_skips_parser(&progress));
        assert!(checkpoint_skips_parser(
            &code_reference_search_query_index_repair_state(
                16,
                code_reference_search_rebuild(&progress).expect("progress should parse")
            )
            .expect("repair should format")
        ));
    }
}

#[test]
fn crashed_reference_resolution_page_skips_blob_fetch_and_parser_for_direct_and_repair_states() {
    let direct = code_reference_resolution_state(7, 31, Some("reference:31"))
        .expect("reference-resolution cursor should format");
    let parsed = code_reference_resolution(&direct).expect("direct cursor should parse");
    let repair = code_reference_resolution_query_index_repair_state(16, parsed)
        .expect("nested repair cursor should format");

    for state in [&direct, &repair] {
        assert!(
            checkpoint_skips_parser(state),
            "a reopened durable page must continue storage finalization without fetching blobs: {state}"
        );
    }
    assert_eq!(parsed.completed_page_ordinal, 7);
    assert_eq!(parsed.completed_reference_count, 31);
}
