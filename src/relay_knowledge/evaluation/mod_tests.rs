use super::*;
use crate::domain::{CodeRetrievalLayer, RepositoryCodeRange};

#[test]
fn evaluates_phase4_case_families() {
    let cases = phase4_fixture_cases().expect("fixture cases should validate");
    let observations = vec![
        observation(&["ev-exact"], &[RetrieverSource::Bm25], false),
        observation(&["ev-path"], &[RetrieverSource::GraphPath], false),
        observation(&["ev-temporal"], &[RetrieverSource::Temporal], false),
        observation(&[], &[], false),
        observation(&["ev-stale"], &[], true),
        observation(&["ev-rust-language", "ev-rust-material"], &[], false),
        observation(
            &["symbol:retry_policy"],
            &[RetrieverSource::CodeGraph],
            false,
        ),
    ];

    let report = evaluate_suite(&cases, &observations).expect("suite should score");

    assert!(report.passed);
    assert_eq!(report.total, 7);
}

#[test]
fn reports_missing_forbidden_and_stale_failures() {
    let case = EvaluationCase::new("case", EvaluationCaseKind::ExactFact, "query")
        .unwrap()
        .requiring_results(&["wanted"])
        .unwrap()
        .forbidding_results(&["forbidden"])
        .unwrap()
        .requiring_sources(&[RetrieverSource::Vector])
        .expecting_stale(false);
    let result = evaluate_case(
        &case,
        &observation(&["forbidden"], &[RetrieverSource::Bm25], true),
    );

    assert!(!result.passed);
    assert_eq!(result.missing_result_ids, ["wanted"]);
    assert_eq!(result.forbidden_result_ids, ["forbidden"]);
    assert_eq!(result.missing_sources, [RetrieverSource::Vector]);
    assert_eq!(result.stale_mismatch, Some(false));
}

#[test]
fn code_impact_observation_preserves_sources_and_stale_state() {
    let observation = EvaluationObservation::from_code_impact(&[code_hit(true)]);

    assert_eq!(observation.result_ids, ["symbol:retry_policy"]);
    assert_eq!(observation.retriever_sources, [RetrieverSource::CodeGraph]);
    assert!(observation.stale);
}

fn observation(ids: &[&str], sources: &[RetrieverSource], stale: bool) -> EvaluationObservation {
    EvaluationObservation {
        result_ids: ids.iter().map(|id| (*id).to_owned()).collect(),
        retriever_sources: sources.to_vec(),
        stale,
    }
}

fn code_hit(stale: bool) -> CodeRetrievalHit {
    CodeRetrievalHit {
        repository_id: "repo".to_owned(),
        scope_id: "main".to_owned(),
        resolved_commit_sha: "abc".to_owned(),
        tree_hash: "tree".to_owned(),
        path: "src/lib.rs".to_owned(),
        language_id: "rust".to_owned(),
        byte_range: RepositoryCodeRange { start: 0, end: 10 },
        line_range: RepositoryCodeRange { start: 1, end: 1 },
        symbol_snapshot_id: Some("symbol:retry_policy".to_owned()),
        canonical_symbol_id: Some("repo://repo/src::lib::retry_policy".to_owned()),
        file_id: Some("file:src/lib.rs".to_owned()),
        retrieval_layers: vec![CodeRetrievalLayer::Impact],
        index_versions: vec!["code_graph:1".to_owned()],
        stale,
        staleness_hint: None,
        degraded_reason: None,
        edge_kind: None,
        edge_resolution_state: None,
        edge_target_hint: None,
        edge_confidence_basis_points: None,
        edge_confidence_tier: None,
        score: 1.0,
        excerpt: "fn retry_policy() {}".to_owned(),
    }
}
