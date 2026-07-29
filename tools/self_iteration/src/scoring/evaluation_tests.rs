use super::*;

#[test]
fn evaluation_composes_stage_scores_into_public_breakdown() {
    let observation = EvaluationObservation::empty(true);

    let score = score_evaluation(&observation, ScoreBaselines::default());

    assert_eq!(score.foundational_capability, 0.0);
    assert_eq!(score.competitive_capability, 0.0);
    assert_eq!(score.performance, 1.0);
    assert_eq!(score.stability, 1.0);
    assert_eq!(score.scoring_policy, "dynamic_capability_ceiling_v1");
    assert!(score.accepted);
}
