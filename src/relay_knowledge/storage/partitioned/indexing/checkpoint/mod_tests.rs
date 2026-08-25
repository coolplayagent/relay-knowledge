use super::super::test_support::partitioned_store;
use super::{by_scope, latest, project_publication_state};
use crate::domain::{
    CodeIndexCheckpoint, CodeIndexResourceBudget, CodeQueryIndexRepairResumePhase,
    CodeReferenceSearchRebuildStage, code_query_index_repair_state,
    code_reference_search_query_index_repair_state, code_reference_search_rebuild,
    code_reference_search_rebuild_state,
};

#[tokio::test]
async fn empty_checkpoint_routes_fall_back_without_creating_a_shard() {
    let store = partitioned_store("checkpoint-fallback");

    assert!(
        by_scope(&store, "scope-missing".to_owned())
            .await
            .expect("scope checkpoint lookup should succeed")
            .is_none()
    );
    assert!(
        latest(&store, "repo-missing".to_owned())
            .await
            .expect("repository checkpoint lookup should succeed")
            .is_none()
    );
}

#[test]
fn active_catalog_never_projects_raw_query_index_repair_as_completed() {
    for resume_phase in [
        CodeQueryIndexRepairResumePhase::SoftwareProjection,
        CodeQueryIndexRepairResumePhase::PartitionedPublish,
    ] {
        let state = code_query_index_repair_state(16, resume_phase)
            .expect("repair checkpoint should format");
        let mut checkpoint = checkpoint_with_state(state.clone());

        project_publication_state(&mut checkpoint, true, true);

        assert_eq!(checkpoint.state, state);
    }
}

#[test]
fn active_raw_partitioned_checkpoint_stays_pending_until_query_indexes_are_ready() {
    let mut checkpoint = checkpoint_with_state("finalizing:partitioned_publish".to_owned());

    project_publication_state(&mut checkpoint, true, false);

    assert_eq!(checkpoint.state, "finalizing:partitioned_publish");
}

#[test]
fn active_catalog_never_projects_raw_reference_search_progress_as_completed() {
    let state = code_reference_search_rebuild_state(CodeReferenceSearchRebuildStage::Build, 7);
    let mut checkpoint = checkpoint_with_state(state.clone());

    project_publication_state(&mut checkpoint, true, true);

    assert_eq!(checkpoint.state, state);

    let repair = code_reference_search_query_index_repair_state(
        16,
        code_reference_search_rebuild(&state).expect("progress should parse"),
    )
    .expect("repair should format");
    let mut checkpoint = checkpoint_with_state(repair.clone());
    project_publication_state(&mut checkpoint, true, true);
    assert_eq!(checkpoint.state, repair);
}

fn checkpoint_with_state(state: String) -> CodeIndexCheckpoint {
    CodeIndexCheckpoint {
        repository_id: "repo".to_owned(),
        source_scope: "scope".to_owned(),
        resolved_commit_sha: "commit".to_owned(),
        tree_hash: "tree".to_owned(),
        path_filters: Vec::new(),
        language_filters: Vec::new(),
        state,
        total_path_count: 0,
        parsed_file_count: 0,
        committed_file_count: 0,
        committed_symbol_count: 0,
        committed_reference_count: 0,
        committed_chunk_count: 0,
        committed_fact_row_count: 0,
        incremental_summary: None,
        batch_count: 0,
        last_path: None,
        resource_budget: CodeIndexResourceBudget::default(),
        updated_at_ms: 1,
    }
}
