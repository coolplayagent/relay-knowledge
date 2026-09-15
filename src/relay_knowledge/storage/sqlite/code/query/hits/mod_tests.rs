//! Regression tests for retrieval-hit projection and consolidation.

use super::*;
use crate::domain::{CodeRetrievalLayer, RepositoryCodeRange, StalenessHint};

fn make_hit(staleness_hint: Option<StalenessHint>) -> CodeRetrievalHit {
    let r = RepositoryCodeRange { start: 0, end: 1 };
    CodeRetrievalHit {
        query_degraded: false,
        repository_id: String::new(),
        scope_id: String::new(),
        resolved_commit_sha: String::new(),
        tree_hash: String::new(),
        path: String::new(),
        language_id: String::new(),
        byte_range: r.clone(),
        line_range: r,
        symbol_snapshot_id: None,
        canonical_symbol_id: None,
        file_id: None,
        retrieval_layers: vec![CodeRetrievalLayer::Lexical],
        index_versions: vec![],
        stale: staleness_hint
            .as_ref()
            .is_some_and(StalenessHint::requires_source_verification),
        staleness_hint,
        degraded_reason: None,
        edge_kind: None,
        edge_resolution_state: None,
        edge_target_hint: None,
        edge_confidence_basis_points: None,
        edge_confidence_tier: None,
        score: 1.0,
        excerpt: String::new(),
    }
}

#[test]
fn merge_prefers_stale_over_fresh() {
    let fresh_hit = make_hit(Some(StalenessHint::Fresh));
    let stale_hit = make_hit(Some(StalenessHint::Stale {}));
    let mut target = fresh_hit.clone();
    merge_hit_provenance(&mut target, &stale_hit);
    assert_eq!(target.staleness_hint, Some(StalenessHint::Stale {}));
    assert!(target.stale);
}

#[test]
fn merge_keeps_stale_when_source_is_fresh() {
    let stale_hit = make_hit(Some(StalenessHint::Stale {}));
    let fresh_hit = make_hit(Some(StalenessHint::Fresh));
    let mut target = stale_hit.clone();
    merge_hit_provenance(&mut target, &fresh_hit);
    assert_eq!(target.staleness_hint, Some(StalenessHint::Stale {}));
    assert!(target.stale);
}

#[test]
fn merge_fills_none_from_source() {
    let no_hint = make_hit(None);
    let fresh_hit = make_hit(Some(StalenessHint::Fresh));
    let mut target = no_hint.clone();
    merge_hit_provenance(&mut target, &fresh_hit);
    assert_eq!(target.staleness_hint, Some(StalenessHint::Fresh));
    assert!(!target.stale);
}

#[test]
fn merge_preserves_none_when_both_none() {
    let a = make_hit(None);
    let b = make_hit(None);
    let mut target = a.clone();
    merge_hit_provenance(&mut target, &b);
    assert_eq!(target.staleness_hint, None);
    assert!(!target.stale);
}

#[test]
fn merge_stale_bool_ors() {
    let fresh_hit = make_hit(Some(StalenessHint::Fresh));
    let stale_hit = make_hit(Some(StalenessHint::Stale {}));
    let mut target = fresh_hit.clone();
    assert!(!target.stale);
    merge_hit_provenance(&mut target, &stale_hit);
    assert!(target.stale);
}

#[test]
fn merge_prefers_pending_index_over_stale() {
    let stale_hit = make_hit(Some(StalenessHint::Stale {}));
    let pending_hit = make_hit(Some(StalenessHint::PendingIndex {}));
    let mut target = stale_hit.clone();
    merge_hit_provenance(&mut target, &pending_hit);
    assert_eq!(target.staleness_hint, Some(StalenessHint::PendingIndex {}));
    assert!(target.stale);
}

#[test]
fn query_degradation_survives_file_diagnostics_and_hit_consolidation() {
    let mut hit = make_hit(Some(StalenessHint::Fresh));
    hit.degraded_reason = Some("file parsing is partial".into());
    assert!(!hit.query_degraded);
    mark_hits_degraded(std::slice::from_mut(&mut hit), "read model unavailable");
    assert!(hit.query_degraded);
    assert_eq!(
        hit.degraded_reason.as_deref(),
        Some("file parsing is partial")
    );

    let mut target = make_hit(Some(StalenessHint::Fresh));
    merge_hit_provenance(&mut target, &hit);
    assert!(target.query_degraded);
    merge_hit_provenance(&mut target, &make_hit(Some(StalenessHint::Fresh)));
    assert!(target.query_degraded);

    let mut legacy = serde_json::to_value(target).unwrap();
    legacy.as_object_mut().unwrap().remove("query_degraded");
    let legacy: CodeRetrievalHit = serde_json::from_value(legacy).unwrap();
    assert!(!legacy.query_degraded);
}
