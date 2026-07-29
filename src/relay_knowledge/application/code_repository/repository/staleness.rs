use crate::domain::{CodeRetrievalHit, StalenessHint};

pub(in crate::application::code_repository) fn annotate_query_result_staleness(
    results: &mut [CodeRetrievalHit],
    freshness: &crate::api::CodeRepositoryFreshnessDiagnostics,
) {
    let hint = if freshness.pending.active_matches_request && freshness.direct_source_read_required
    {
        StalenessHint::PendingIndex {}
    } else if freshness.direct_source_read_required {
        StalenessHint::Stale {}
    } else {
        StalenessHint::Fresh
    };
    let requires_source_verification = hint.requires_source_verification();
    for hit in results {
        if requires_source_verification {
            hit.stale = true;
        }
        if hint.should_replace(hit.staleness_hint.as_ref()) {
            hit.staleness_hint = Some(hint.clone());
        }
    }
}
