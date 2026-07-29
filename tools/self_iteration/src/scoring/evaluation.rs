pub fn score_evaluation(
    observation: &EvaluationObservation,
    baselines: ScoreBaselines<'_>,
) -> ScoreBreakdown {
    let previous_run = baselines.workload_previous;
    let foundational_scores =
        objective_scores(&observation.cases, "foundational_capability", &["accuracy"]);
    let competitive_scores = objective_scores(&observation.cases, "competitive_capability", &[]);
    let semantic_scores = objective_scores(&observation.cases, "semantic_vector", &[]);
    let research_scores = objective_scores(&observation.cases, "research_judge", &[]);
    let foundational_capability = average(&foundational_scores, 0.0);
    let competitive_capability = average(&competitive_scores, 0.0);
    let accuracy_components = [
        (!foundational_scores.is_empty()).then_some(foundational_capability),
        (!competitive_scores.is_empty()).then_some(competitive_capability),
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>();
    let accuracy = average(&accuracy_components, 0.0);
    let semantic_vector = average(&semantic_scores, 0.0);
    let research_judge = (!research_scores.is_empty()).then(|| average(&research_scores, 0.0));
    let performance = performance_score(&observation.metrics, previous_run);
    let has_key_performance_metrics = observation.metrics.iter().any(|metric| metric.key);
    let stability = stability_score(&observation.gates);
    let base_score = weighted_score(
        foundational_capability,
        competitive_capability,
        semantic_vector,
        research_judge,
        performance,
        stability,
    );
    let current = ScoreComponents {
        score: base_score,
        foundational_capability,
        competitive_capability,
        semantic_vector,
        research_judge,
        performance,
        stability,
    };
    let capability_ceiling_bonus =
        capability_ceiling_bonus(current, baselines, has_key_performance_metrics);
    let score = clamp(base_score + capability_ceiling_bonus);
    let improvements = changes(
        observation,
        ScoreComponents {
            score,
            foundational_capability,
            competitive_capability,
            semantic_vector,
            research_judge,
            performance,
            stability,
        },
        previous_run,
        true,
    );
    let degradations = changes(
        observation,
        ScoreComponents {
            score,
            foundational_capability,
            competitive_capability,
            semantic_vector,
            research_judge,
            performance,
            stability,
        },
        previous_run,
        false,
    );
    let reject_reasons = reject_reasons(
        observation,
        ScoreComponents {
            score,
            foundational_capability,
            competitive_capability,
            semantic_vector,
            research_judge,
            performance,
            stability,
        },
        baselines,
        &improvements,
    );
    ScoreBreakdown {
        score,
        foundational_capability,
        competitive_capability,
        accuracy,
        semantic_vector,
        research_judge,
        performance,
        stability,
        base_score,
        capability_ceiling_bonus,
        scoring_policy: "dynamic_capability_ceiling_v1".to_owned(),
        accepted: reject_reasons.is_empty(),
        reject_reasons,
        performance_strategy: "budget_relative_v2".to_owned(),
        degradations,
        improvements,
        metric_budget_failures: metric_budget_failures(&observation.metrics),
    }
}

#[cfg(test)]
#[path = "evaluation_tests.rs"]
mod evaluation_tests;
use super::{
    EvaluationObservation, ScoreBaselines, ScoreBreakdown, ScoreComponents,
    capability::{capability_ceiling_bonus, performance_score, stability_score, weighted_score},
    change_detection::{changes, metric_budget_failures, objective_scores},
    decision::reject_reasons,
    score_math::{average, clamp},
};
