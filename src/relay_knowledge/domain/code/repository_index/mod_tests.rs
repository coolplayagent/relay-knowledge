//! Direct contracts for repository-index budgets and summaries.

use super::{
    CODE_QUERY_INDEX_PLAN_UNIT_COUNT, CODE_QUERY_INDEX_PLAN_VERSION, CodeIndexCheckpoint,
    CodeIndexProgressSummary, CodeIndexResourceBudget, CodeIndexSummary, CodeQueryIndexRepair,
    CodeQueryIndexRepairResumePhase, CodeQueryIndexSubphase, CodeReferenceResolution,
    CodeReferenceResolutionQueryIndexRepair, CodeReferenceResolutionStage,
    CodeReferenceSearchQueryIndexRepair, CodeReferenceSearchRebuild,
    CodeReferenceSearchRebuildStage, CodeScopeRetentionSummary, code_query_index_repair,
    code_query_index_repair_state, code_query_index_subphase, code_query_index_subphase_state,
    code_reference_resolution, code_reference_resolution_cursor_digest,
    code_reference_resolution_query_index_repair,
    code_reference_resolution_query_index_repair_state, code_reference_resolution_state,
    code_reference_search_query_index_repair, code_reference_search_query_index_repair_state,
    code_reference_search_rebuild, code_reference_search_rebuild_state,
};

#[test]
fn reference_resolution_tokens_are_versioned_bounded_and_canonical() {
    let cursor_digest = code_reference_resolution_cursor_digest(Some("reference:max"));
    let state = code_reference_resolution_state(usize::MAX, usize::MAX, Some("reference:max"))
        .expect("canonical reference-resolution token should format");
    assert_eq!(
        code_reference_resolution(&state),
        Some(CodeReferenceResolution {
            protocol_version: 1,
            stage: CodeReferenceResolutionStage::Resolve,
            completed_page_ordinal: usize::MAX,
            completed_reference_count: usize::MAX,
            cursor_digest,
        })
    );
    for malformed in [
        "finalizing:resolve_references:v0:resolve:0:0:none",
        "finalizing:resolve_references:v2:resolve:0:0:none",
        "finalizing:resolve_references:v1:unknown:0:0:none",
        "finalizing:resolve_references:v1:resolve:00:0:none",
        "finalizing:resolve_references:v1:resolve:0:00:none",
        "finalizing:resolve_references:v1:resolve:-1:0:none",
        "finalizing:resolve_references:v1:resolve:0:1:none",
        "finalizing:resolve_references:v1:resolve:2:1:0123456789abcdef",
        "finalizing:resolve_references:v1:resolve:1:1:none",
        "finalizing:resolve_references:v1:resolve:1:1:ABCDEF0123456789",
        "finalizing:resolve_references:v1:resolve:1:1:abc",
        "finalizing:resolve_references:v1:resolve:0:0:none:extra",
    ] {
        assert_eq!(
            code_reference_resolution(malformed),
            None,
            "state={malformed}"
        );
    }
}

#[test]
fn reference_resolution_query_index_repair_preserves_the_exact_page_token() {
    let resolution = CodeReferenceResolution {
        protocol_version: 1,
        stage: CodeReferenceResolutionStage::Resolve,
        completed_page_ordinal: 7,
        completed_reference_count: 31,
        cursor_digest: code_reference_resolution_cursor_digest(Some("reference:31")),
    };
    let state = code_reference_resolution_query_index_repair_state(16, resolution)
        .expect("current repair unit should format");
    assert_eq!(
        code_reference_resolution_query_index_repair(&state),
        Some(CodeReferenceResolutionQueryIndexRepair {
            plan_version: CODE_QUERY_INDEX_PLAN_VERSION,
            completed_unit: 16,
            reference_resolution: resolution,
        })
    );
    let legacy_plan = state.replacen(
        "finalizing:query_index_repair:v3:",
        "finalizing:query_index_repair:v2:",
        1,
    );
    let parsed = code_reference_resolution_query_index_repair(&legacy_plan)
        .expect("version-two repair wrapper should parse");
    assert!(parsed.requires_legacy_retired_prefix());
    let digest = format!(
        "{:016x}",
        code_reference_resolution_cursor_digest(Some("reference:31"))
            .expect("nonempty cursor should hash")
    );
    assert_eq!(
        parsed.next_state(15).as_deref(),
        Some(
            format!(
                "finalizing:query_index_repair:v2:15:resume:reference_resolution:v1:resolve:7:31:{digest}"
            )
            .as_str()
        )
    );
    assert_eq!(
        parsed.reference_resolution.checkpoint_state().as_deref(),
        Some(format!("finalizing:resolve_references:v1:resolve:7:31:{digest}").as_str())
    );
    for malformed in [
        "finalizing:query_index_repair:v3:17:resume:reference_resolution:v1:resolve:7:31:0123456789abcdef",
        "finalizing:query_index_repair:v3:16:resume:reference_resolution:v2:resolve:7:31:0123456789abcdef",
        "finalizing:query_index_repair:v3:16:resume:reference_resolution:v1:resolve:07:31:0123456789abcdef",
        "finalizing:query_index_repair:v3:16:resume:reference_resolution:v1:resolve:7:031:0123456789abcdef",
        "finalizing:query_index_repair:v3:16:resume:reference_resolution:v1:unknown:7:31:0123456789abcdef",
    ] {
        assert_eq!(
            code_reference_resolution_query_index_repair(malformed),
            None,
            "state={malformed}"
        );
    }
}

#[test]
fn reference_search_query_index_repair_tokens_preserve_canonical_page_boundaries() {
    let reference_search = CodeReferenceSearchRebuild {
        protocol_version: 2,
        stage: CodeReferenceSearchRebuildStage::Build,
        completed_page_ordinal: usize::MAX,
    };
    let state = code_reference_search_query_index_repair_state(16, reference_search)
        .expect("current unit should format");
    assert!(
        !code_reference_search_query_index_repair(&state)
            .expect("current wrapper should parse")
            .requires_legacy_retired_prefix()
    );
    assert_eq!(
        code_reference_search_query_index_repair(&state),
        Some(CodeReferenceSearchQueryIndexRepair {
            plan_version: CODE_QUERY_INDEX_PLAN_VERSION,
            completed_unit: 16,
            reference_search,
        })
    );
    let version_two_state = state.replacen(
        "finalizing:query_index_repair:v3:",
        "finalizing:query_index_repair:v2:",
        1,
    );
    assert_eq!(
        code_reference_search_query_index_repair(&version_two_state),
        Some(CodeReferenceSearchQueryIndexRepair {
            plan_version: 2,
            completed_unit: 16,
            reference_search,
        })
    );
    assert!(
        code_reference_search_query_index_repair(&version_two_state)
            .expect("version-two wrapper should parse")
            .requires_legacy_retired_prefix()
    );
    let legacy = code_reference_search_query_index_repair(
        "finalizing:query_index_repair:v2:1:resume:reference_search:v1:build:7",
    )
    .expect("legacy nested cursor should parse");
    assert_eq!(
        legacy.next_state(2).as_deref(),
        Some("finalizing:query_index_repair:v2:2:resume:reference_search:v1:build:7")
    );
    assert_eq!(
        legacy.reference_search.checkpoint_state().as_deref(),
        Some("finalizing:rebuild_reference_search:v1:build:7")
    );
    for state in [
        "finalizing:query_index_repair:v1:16:resume:reference_search:v2:build:0",
        "finalizing:query_index_repair:v4:16:resume:reference_search:v2:build:0",
        "finalizing:query_index_repair:v2:17:resume:reference_search:v2:build:0",
        "finalizing:query_index_repair:v3:16:resume:reference_search:v0:build:0",
        "finalizing:query_index_repair:v3:16:resume:reference_search:v2:build:00",
        "finalizing:query_index_repair:v3:16:resume:reference_search:v2:unknown:0",
        "finalizing:query_index_repair:v3:16:resume:reference_search:v2:build:0:extra",
    ] {
        assert_eq!(
            code_reference_search_query_index_repair(state),
            None,
            "state={state}"
        );
    }
}

#[test]
fn reference_search_tokens_are_versioned_and_canonical() {
    for stage in [
        CodeReferenceSearchRebuildStage::Cleanup,
        CodeReferenceSearchRebuildStage::Discover,
        CodeReferenceSearchRebuildStage::Build,
    ] {
        let state = code_reference_search_rebuild_state(stage, usize::MAX);
        assert_eq!(
            code_reference_search_rebuild(&state),
            Some(CodeReferenceSearchRebuild {
                protocol_version: 2,
                stage,
                completed_page_ordinal: usize::MAX,
            })
        );
    }
    for stage in [
        CodeReferenceSearchRebuildStage::Cleanup,
        CodeReferenceSearchRebuildStage::Build,
    ] {
        let state = format!(
            "finalizing:rebuild_reference_search:v1:{}:{}",
            stage.code(),
            usize::MAX
        );
        let parsed = code_reference_search_rebuild(&state)
            .expect("a canonical legacy reference-search cursor should parse");
        assert_eq!(parsed.protocol_version, 1);
        assert_eq!(parsed.checkpoint_state().as_deref(), Some(state.as_str()));
    }
    for state in [
        "finalizing:rebuild_reference_search:v0:cleanup:0",
        "finalizing:rebuild_reference_search:v3:cleanup:0",
        "finalizing:rebuild_reference_search:v1:discover:0",
        "finalizing:rebuild_reference_search:v1:unknown:0",
        "finalizing:rebuild_reference_search:v1:cleanup:00",
        "finalizing:rebuild_reference_search:v1:cleanup:-1",
        "finalizing:rebuild_reference_search:v1:cleanup:0:extra",
    ] {
        assert_eq!(code_reference_search_rebuild(state), None, "state={state}");
    }
}

#[test]
fn query_index_subphase_tokens_are_versioned_bounded_and_canonical() {
    for unit in 0..CODE_QUERY_INDEX_PLAN_UNIT_COUNT {
        let state = code_query_index_subphase_state(unit).expect("bounded unit should format");
        assert_eq!(state, format!("finalizing:build_query_indexes:v3:{unit}"));
        assert_eq!(
            code_query_index_subphase(&state).map(|cursor| cursor.completed_unit),
            Some(unit)
        );
        assert!(
            !code_query_index_subphase(&state)
                .expect("current cursor should parse")
                .requires_legacy_retired_prefix()
        );
    }

    assert!(code_query_index_subphase_state(CODE_QUERY_INDEX_PLAN_UNIT_COUNT).is_none());
    assert_eq!(
        code_query_index_subphase("finalizing:build_query_indexes:v1:15")
            .map(|cursor| cursor.completed_unit),
        Some(15)
    );
    let version_one = code_query_index_subphase("finalizing:build_query_indexes:v1:1")
        .expect("version-one cursor should parse");
    assert_eq!(
        version_one.next_state(2).as_deref(),
        Some("finalizing:build_query_indexes:v1:2")
    );
    assert!(version_one.next_state(16).is_none());
    let version_two = code_query_index_subphase("finalizing:build_query_indexes:v2:1")
        .expect("version-two cursor should parse");
    assert_eq!(
        version_two.next_state(2).as_deref(),
        Some("finalizing:build_query_indexes:v2:2")
    );
    assert_eq!(
        code_query_index_subphase("finalizing:build_query_indexes:v1:15"),
        Some(CodeQueryIndexSubphase {
            plan_version: 1,
            completed_unit: 15,
        })
    );
    assert!(
        code_query_index_subphase("finalizing:build_query_indexes:v1:15")
            .expect("version-one cursor should parse")
            .requires_legacy_retired_prefix()
    );
    assert_eq!(
        code_query_index_subphase("finalizing:build_query_indexes:v2:16")
            .map(|cursor| cursor.completed_unit),
        Some(16)
    );
    assert_eq!(
        code_query_index_subphase("finalizing:build_query_indexes:v2:16"),
        Some(CodeQueryIndexSubphase {
            plan_version: 2,
            completed_unit: 16,
        })
    );
    assert!(
        code_query_index_subphase("finalizing:build_query_indexes:v2:16")
            .expect("version-two cursor should parse")
            .requires_legacy_retired_prefix()
    );
    assert_eq!(
        code_query_index_subphase("finalizing:build_query_indexes:v4:0")
            .map(|cursor| cursor.completed_unit),
        None
    );
    assert_eq!(
        code_query_index_subphase("finalizing:build_query_indexes:v1:00")
            .map(|cursor| cursor.completed_unit),
        None
    );
    assert_eq!(
        code_query_index_subphase("finalizing:build_query_indexes:v1:16")
            .map(|cursor| cursor.completed_unit),
        None
    );
    assert_eq!(
        code_query_index_subphase("finalizing:build_query_indexes:v2:17")
            .map(|cursor| cursor.completed_unit),
        None
    );
}

#[test]
fn query_index_repair_tokens_preserve_every_stable_coarse_phase() {
    for resume_phase in CodeQueryIndexRepairResumePhase::ALL {
        let state = code_query_index_repair_state(16, resume_phase)
            .expect("bounded repair unit should format");
        assert!(state.starts_with("finalizing:query_index_repair:v3:16:resume:"));
        assert_eq!(
            code_query_index_repair(&state),
            Some(CodeQueryIndexRepair {
                plan_version: CODE_QUERY_INDEX_PLAN_VERSION,
                completed_unit: 16,
                resume_phase,
            })
        );
        assert_eq!(
            CodeQueryIndexRepairResumePhase::from_checkpoint_state(resume_phase.checkpoint_state()),
            Some(resume_phase)
        );
    }
    assert_eq!(CodeQueryIndexRepairResumePhase::ALL.len(), 11);
    assert!(CodeQueryIndexRepairResumePhase::from_checkpoint_state("completed").is_none());
    assert_eq!(
        code_query_index_repair("finalizing:query_index_repair:v2:16:resume:10"),
        Some(CodeQueryIndexRepair {
            plan_version: 2,
            completed_unit: 16,
            resume_phase: CodeQueryIndexRepairResumePhase::PartitionedPublish,
        })
    );
    assert!(
        code_query_index_repair("finalizing:query_index_repair:v2:16:resume:10")
            .expect("version-two repair should parse")
            .requires_legacy_retired_prefix()
    );
    let version_two = code_query_index_repair("finalizing:query_index_repair:v2:1:resume:10")
        .expect("version-two repair should parse");
    assert_eq!(
        version_two.next_state(2).as_deref(),
        Some("finalizing:query_index_repair:v2:2:resume:10")
    );
}

#[test]
fn query_index_repair_tokens_reject_noncanonical_or_unknown_fields() {
    for state in [
        "finalizing:query_index_repair:v1:15:resume:0",
        "finalizing:query_index_repair:v4:0:resume:0",
        "finalizing:query_index_repair:v2:17:resume:0",
        "finalizing:query_index_repair:v3:00:resume:0",
        "finalizing:query_index_repair:v3:0:resume:00",
        "finalizing:query_index_repair:v3:0:resume:11",
        "finalizing:query_index_repair:v3:0:resume:0:extra",
    ] {
        assert_eq!(code_query_index_repair(state), None, "state={state}");
    }
    assert!(
        code_query_index_repair_state(
            CODE_QUERY_INDEX_PLAN_UNIT_COUNT,
            CodeQueryIndexRepairResumePhase::BuildQueryIndexes,
        )
        .is_none()
    );
}

#[test]
fn checkpoint_identity_defaults_when_deserializing_legacy_json() {
    let checkpoint = serde_json::from_value::<CodeIndexCheckpoint>(serde_json::json!({
        "repository_id": "repo",
        "source_scope": "scope",
        "state": "indexing",
        "total_path_count": 0,
        "parsed_file_count": 0,
        "committed_file_count": 0,
        "committed_symbol_count": 0,
        "committed_reference_count": 0,
        "committed_chunk_count": 0,
        "batch_count": 0,
        "resource_budget": CodeIndexResourceBudget::default(),
        "updated_at_ms": 1
    }))
    .expect("legacy checkpoint JSON should remain readable");

    assert!(checkpoint.resolved_commit_sha.is_empty());
    assert!(checkpoint.tree_hash.is_empty());
    assert!(checkpoint.path_filters.is_empty());
    assert!(checkpoint.language_filters.is_empty());
}

#[test]
fn default_budget_batches_more_small_files_without_raising_row_or_byte_caps() {
    let budget = CodeIndexResourceBudget::default();

    assert_eq!(budget.max_files_per_batch, 512);
    assert_eq!(
        budget.max_bytes_per_batch,
        CodeIndexResourceBudget::DEFAULT_MAX_BYTES_PER_BATCH
    );
    assert_eq!(
        budget.max_rows_per_batch,
        CodeIndexResourceBudget::DEFAULT_MAX_ROWS_PER_BATCH
    );
}

#[test]
fn scope_retention_gc_status_defaults_when_deserializing_older_responses() {
    let summary = serde_json::from_value::<CodeScopeRetentionSummary>(serde_json::json!({
        "repository_id": "repo",
        "retained_scope_count": 1,
        "prunable_scope_count": 0,
        "pruned_scope_count": 0,
        "retained_scopes": ["scope"],
        "prunable_scopes": [],
        "pruned_scopes": []
    }))
    .expect("older retention response should deserialize");

    assert_eq!(summary.retiring_job_count, 0);
    assert!(!summary.maintenance_pending);
    assert!(summary.retiring_jobs.is_empty());
    assert!(!summary.scope_listing_truncated);
}

#[test]
fn generated_summary_counts_default_when_deserializing_older_responses() {
    let mut summary_json = serde_json::to_value(CodeIndexSummary {
        repository_id: "repo".to_owned(),
        source_scope: "scope".to_owned(),
        base_resolved_commit_sha: Some("base".to_owned()),
        resolved_commit_sha: "commit".to_owned(),
        tree_hash: "tree".to_owned(),
        indexed_file_count: 1,
        changed_path_count: 1,
        skipped_unchanged_count: 0,
        deleted_path_count: 0,
        symbol_count: 2,
        handwritten_symbol_count: 1,
        generated_symbol_count: 1,
        reference_count: 0,
        chunk_count: 0,
        degraded_file_count: 0,
        progress: CodeIndexProgressSummary::default(),
    })
    .expect("summary should serialize");
    let summary_object = summary_json
        .as_object_mut()
        .expect("summary json should be an object");
    summary_object.remove("handwritten_symbol_count");
    summary_object.remove("generated_symbol_count");
    summary_object.remove("base_resolved_commit_sha");
    let summary = serde_json::from_value::<CodeIndexSummary>(summary_json)
        .expect("older summary response should deserialize");

    assert_eq!(summary.handwritten_symbol_count, 0);
    assert_eq!(summary.generated_symbol_count, 0);
    assert_eq!(summary.base_resolved_commit_sha, None);
}
