use super::judge_outcome;

fn judge_payload_with_scores(scores: serde_json::Value) -> serde_json::Value {
    serde_json::json!({
        "passed": true,
        "confidence": 0.9,
        "overall_score": 0.9,
        "summary": "solid",
        "scores": scores,
        "evidence": ["diff and deterministic summary support the score"],
        "risks": [],
        "recommended_cases": ["add a deterministic guardrail"],
        "capability_delta": {"competitive": "improved"},
        "research_gaps": []
    })
}

fn complete_judge_scores() -> serde_json::Value {
    serde_json::json!({
        "research_alignment": 0.9,
        "competitive_advantage": 0.8,
        "architecture_soundness": 0.9,
        "performance_generalization": 0.8,
        "implementation_actionability": 0.8,
        "anti_fixture_special_casing": 0.9,
        "judge_evidence_quality": 0.8
    })
}

#[test]
fn judge_outcome_requires_dimension_scores() {
    let suite = serde_json::json!({"min_dimension_score": 0.7});
    let payload = serde_json::json!({
        "passed": true,
        "confidence": 0.9,
        "overall_score": 0.9,
        "summary": "solid",
        "scores": {
            "anti_fixture_special_casing": 0.9
        }
    });

    let outcome = judge_outcome(&payload.to_string(), &suite);

    assert!(!outcome.0);
    assert!(!outcome.1);
    assert!(outcome.3.contains("research_alignment=missing"));
    assert!(outcome.3.contains("competitive_advantage=missing"));
}

#[test]
fn judge_outcome_requires_evidence_fields() {
    let suite = serde_json::json!({"min_dimension_score": 0.7});
    let payload = serde_json::json!({
        "passed": true,
        "confidence": 0.9,
        "overall_score": 0.9,
        "summary": "solid",
        "scores": complete_judge_scores()
    });

    let outcome = judge_outcome(&payload.to_string(), &suite);

    assert!(!outcome.0);
    assert!(outcome.3.contains("evidence=missing"));
    assert!(outcome.3.contains("recommended_cases=missing"));
    assert!(outcome.3.contains("capability_delta=missing"));
    assert!(outcome.3.contains("research_gaps=missing"));
}

#[test]
fn judge_outcome_rejects_low_dimension_score() {
    let suite = serde_json::json!({"min_dimension_score": 0.7});
    let payload = judge_payload_with_scores(serde_json::json!({
        "research_alignment": 0.9,
        "competitive_advantage": 0.6,
        "architecture_soundness": 0.9,
        "performance_generalization": 0.9,
        "implementation_actionability": 0.9,
        "anti_fixture_special_casing": 0.9,
        "judge_evidence_quality": 0.9
    }));

    let outcome = judge_outcome(&payload.to_string(), &suite);

    assert!(!outcome.0);
    assert!(
        outcome
            .3
            .contains("competitive_advantage=0.600 below min_dimension_score=0.700")
    );
}

#[test]
fn judge_outcome_rejects_out_of_range_dimension_score() {
    let suite = serde_json::json!({"min_dimension_score": 0.7});
    let payload = judge_payload_with_scores(serde_json::json!({
        "research_alignment": 0.9,
        "competitive_advantage": 1.2,
        "architecture_soundness": 0.9,
        "performance_generalization": 0.8,
        "implementation_actionability": 0.8,
        "anti_fixture_special_casing": 0.9,
        "judge_evidence_quality": 0.8
    }));

    let outcome = judge_outcome(&payload.to_string(), &suite);

    assert!(!outcome.0);
    assert!(
        outcome
            .3
            .contains("competitive_advantage=1.200 outside 0.0..1.0")
    );
}

#[test]
fn judge_outcome_accepts_complete_dimension_scores() {
    let suite = serde_json::json!({"min_dimension_score": 0.7});
    let payload = judge_payload_with_scores(complete_judge_scores());

    let outcome = judge_outcome(&payload.to_string(), &suite);

    assert!(outcome.0);
    assert!(outcome.1);
}
