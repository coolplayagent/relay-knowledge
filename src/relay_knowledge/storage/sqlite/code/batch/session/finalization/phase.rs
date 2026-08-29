//! Typed decoding for durable finalization checkpoint tokens.

use crate::domain::{
    CodeQueryIndexRepair, CodeQueryIndexRepairResumePhase, CodeReferenceResolution,
    CodeReferenceResolutionQueryIndexRepair, CodeReferenceSearchQueryIndexRepair,
    CodeReferenceSearchRebuild, code_query_index_repair, code_query_index_subphase,
    code_reference_resolution, code_reference_resolution_query_index_repair,
    code_reference_search_query_index_repair, code_reference_search_rebuild,
};

use super::super::finalize;

pub(super) enum FinalizationCheckpointPhase {
    ReferenceResolutionQueryIndexRepair(CodeReferenceResolutionQueryIndexRepair),
    ReferenceSearchQueryIndexRepair(CodeReferenceSearchQueryIndexRepair),
    QueryIndexRepair(CodeQueryIndexRepair),
    Indexing,
    ReferenceResolution(CodeReferenceResolution),
    ReferenceSearch(CodeReferenceSearchRebuild),
    Coarse {
        resume_phase: CodeQueryIndexRepairResumePhase,
        ready_for_outer_publication: bool,
    },
    Completed,
    ReadyForOuterPublication,
    Unknown,
}

impl FinalizationCheckpointPhase {
    pub(super) fn decode(state: &str) -> Self {
        if let Some(repair) = code_reference_resolution_query_index_repair(state) {
            return Self::ReferenceResolutionQueryIndexRepair(repair);
        }
        if let Some(repair) = code_reference_search_query_index_repair(state) {
            return Self::ReferenceSearchQueryIndexRepair(repair);
        }
        if let Some(repair) = code_query_index_repair(state) {
            return Self::QueryIndexRepair(repair);
        }
        if state == "indexing" || code_query_index_subphase(state).is_some() {
            return Self::Indexing;
        }
        if let Some(resolution) = code_reference_resolution(state) {
            return Self::ReferenceResolution(resolution);
        }
        if let Some(reference_search) = code_reference_search_rebuild(state) {
            return Self::ReferenceSearch(reference_search);
        }
        if let Some(resume_phase) = CodeQueryIndexRepairResumePhase::from_checkpoint_state(state) {
            return Self::Coarse {
                resume_phase,
                ready_for_outer_publication: matches!(
                    state,
                    finalize::phases::SOFTWARE_PROJECTION | finalize::phases::PARTITIONED_PUBLISH
                ),
            };
        }
        match state {
            "completed" => Self::Completed,
            finalize::phases::SOFTWARE_PROJECTION | finalize::phases::PARTITIONED_PUBLISH => {
                Self::ReadyForOuterPublication
            }
            _ => Self::Unknown,
        }
    }
}
