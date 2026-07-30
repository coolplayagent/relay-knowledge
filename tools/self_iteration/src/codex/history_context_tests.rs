use serde_json::json;

use super::*;

#[test]
fn run_brief_and_capability_snapshot_render_missing_and_present_values() {
    let run = json!({
        "run_id": "run-1",
        "score": 0.9,
        "competitive_capability": 0.8,
        "semantic_vector": 0.7,
        "research_judge": 0.6,
        "performance": 0.5,
        "stability": 1.0,
        "reject_reasons": ["reason one", "reason two"]
    });

    let brief = run_brief(&run);
    assert!(brief.contains("run_id=run-1"));
    assert!(brief.contains("reasons=reason one; reason two"));

    let snapshot = capability_snapshot(Some(&run), None, Some(&run));
    assert!(snapshot.contains("- latest: score=0.9"));
    assert!(snapshot.contains("- category_best: none"));
    assert!(snapshot.contains("- profile_best: score=0.9"));
}

#[test]
fn suite_context_is_bounded_and_reports_missing_configuration() {
    let cases = json!({
        "research_judge_suite": {
            "competitive_feature_targets": ["one", "two", "three"],
            "implementation_guardrails": ["guard"]
        }
    });

    assert_eq!(competitive_feature_targets(&cases, 2), "- one\n- two");
    assert_eq!(implementation_guardrails(&cases, 1), "- guard");
    assert_eq!(
        competitive_feature_targets(&json!({}), 2),
        "No research judge targets configured."
    );
}
