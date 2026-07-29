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
