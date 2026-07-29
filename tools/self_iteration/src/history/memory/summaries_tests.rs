use serde_json::json;

use super::*;

#[test]
fn rejected_summary_includes_changes_and_score_delta() {
    let record = rejected_record();

    let summary = primary_summary("rejected_attempt", &record);

    assert!(summary.contains("Score delta: -0.010000"));
    assert!(summary.contains("Changed paths: src/query.rs"));
    assert!(summary.contains("Top improvements: metric:relay_teams_query_p95_ms"));
    assert!(summary.contains("Top degradations: score_component:score"));
}

#[test]
fn accepted_summary_lists_protected_floors() {
    let record = json!({
        "run_id": "accepted",
        "accepted": true,
        "score": 0.8,
        "foundational_capability": 1.0,
        "competitive_capability": 0.8,
        "semantic_vector": 0.0,
        "stability": 1.0,
        "improvements": [{"kind": "score_component", "name": "score", "previous": 0.7, "current": 0.8}],
        "degradations": [],
        "optimization_plan": {"changed_paths": ["src/query.rs"]},
    });

    let summary = primary_summary("accepted_optimization", &record);

    assert!(summary.contains("Protected floors: foundational=1.0"));
    assert!(summary.contains("Key improvements: score_component:score"));
    assert!(summary.contains("Known degradations: none recorded"));
}

#[test]
fn prompt_compaction_removes_blank_lines_and_keeps_bounded_tail() {
    assert_eq!(
        compact_prompt_text(" first \n\n second ", 20),
        "first second"
    );
    assert_eq!(
        compact_prompt_text("prefix-important-tail", 14),
        "important-tail"
    );
}

fn rejected_record() -> serde_json::Value {
    json!({
        "run_id": "current",
        "timestamp": "2",
        "accepted": false,
        "score": 0.79,
        "foundational_capability": 1.0,
        "competitive_capability": 0.8,
        "semantic_vector": 0.0,
        "stability": 1.0,
        "reject_reasons": ["candidate did not improve score or tracked objectives beyond epsilon"],
        "improvements": [{"kind": "metric", "name": "relay_teams_query_p95_ms", "previous": 8000.0, "current": 7000.0}],
        "degradations": [{"kind": "score_component", "name": "score", "previous": 0.8, "current": 0.79}],
        "optimization_plan": {"changed_paths": ["src/query.rs"]},
        "gates": [],
    })
}
