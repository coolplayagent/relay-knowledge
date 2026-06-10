use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::command::CommandResult;

const SCORE_EPSILON: f64 = 0.0005;
const RATIO_EPSILON: f64 = 0.005;
const CASE_SCORE_EPSILON: f64 = 0.005;
const METRIC_RELATIVE_EPSILON: f64 = 0.03;
const METRIC_ABSOLUTE_EPSILON: f64 = 25.0;
const CAPABILITY_CEILING_MAX_BONUS: f64 = 0.06;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GateObservation {
    pub name: String,
    pub passed: bool,
    pub duration_ms: u64,
    pub message: String,
}

impl GateObservation {
    pub fn from_command(result: &CommandResult) -> Self {
        Self {
            name: result.name.clone(),
            passed: result.passed(),
            duration_ms: result.duration_ms,
            message: result.gate_message(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CaseObservation {
    pub case_id: String,
    pub repository: String,
    pub passed: bool,
    #[serde(default)]
    pub guardrail: bool,
    pub rank: Option<usize>,
    pub max_rank: usize,
    pub false_positive_count: usize,
    pub message: String,
    pub objective: String,
    pub score_override: Option<f64>,
}

impl CaseObservation {
    pub fn score(&self) -> f64 {
        if !self.passed {
            return 0.0;
        }
        if let Some(score) = self.score_override {
            return clamp(score);
        }
        let rank_score = match self.rank {
            Some(rank) if rank > 0 => 1.0 / rank as f64,
            _ => 1.0,
        };
        (rank_score - (self.false_positive_count as f64 * 0.1).min(0.5)).max(0.0)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricObservation {
    pub name: String,
    pub value: f64,
    pub budget: Option<f64>,
    pub lower_is_better: bool,
    pub key: bool,
}

impl MetricObservation {
    pub fn score(&self) -> f64 {
        let Some(budget) = self.budget else {
            return 1.0;
        };
        if budget <= 0.0 || self.value < 0.0 {
            return 1.0;
        }
        if self.lower_is_better {
            (budget / self.value.max(1.0)).min(1.0)
        } else {
            (self.value / budget).min(1.0)
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvaluationObservation {
    pub gates: Vec<GateObservation>,
    pub cases: Vec<CaseObservation>,
    pub metrics: Vec<MetricObservation>,
    pub generated_diff: bool,
}

impl EvaluationObservation {
    pub fn empty(generated_diff: bool) -> Self {
        Self {
            gates: Vec::new(),
            cases: Vec::new(),
            metrics: Vec::new(),
            generated_diff,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScoreBreakdown {
    pub score: f64,
    pub foundational_capability: f64,
    pub competitive_capability: f64,
    pub accuracy: f64,
    pub semantic_vector: f64,
    pub research_judge: Option<f64>,
    pub performance: f64,
    pub stability: f64,
    pub base_score: f64,
    pub capability_ceiling_bonus: f64,
    pub scoring_policy: String,
    pub accepted: bool,
    pub reject_reasons: Vec<String>,
    pub performance_strategy: String,
    pub degradations: Vec<Value>,
    pub improvements: Vec<Value>,
    pub metric_budget_failures: Vec<Value>,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct ScoreBaselines<'a> {
    pub workload_previous: Option<&'a Value>,
    pub profile_best_accepted: Option<&'a Value>,
}

include!("scoring_ranked.rs");

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

#[derive(Debug, Clone, Copy)]
struct ScoreComponents {
    score: f64,
    foundational_capability: f64,
    competitive_capability: f64,
    semantic_vector: f64,
    research_judge: Option<f64>,
    performance: f64,
    stability: f64,
}

#[derive(Debug, Clone, Copy)]
struct PreviousCase {
    passed: bool,
    rank: Option<usize>,
    false_positive_count: usize,
    score: f64,
}

fn reject_reasons(
    observation: &EvaluationObservation,
    current: ScoreComponents,
    baselines: ScoreBaselines<'_>,
    improvements: &[Value],
) -> Vec<String> {
    let mut reasons = Vec::new();
    if !observation.generated_diff {
        reasons.push("codex produced no candidate diff".to_owned());
    }
    let failed_gates = observation
        .gates
        .iter()
        .filter(|gate| !gate.passed)
        .map(|gate| gate.name.clone())
        .collect::<Vec<_>>();
    if !failed_gates.is_empty() {
        reasons.push(format!("quality gates failed: {}", failed_gates.join(", ")));
    }
    let bug_fix_priority = bug_fix_priority_improved(improvements);
    let Some(previous) = baselines.workload_previous else {
        if let Some(reason) = profile_best_score_reject_reason(
            current,
            baselines.profile_best_accepted,
            bug_fix_priority,
        ) {
            reasons.push(reason);
        }
        return reasons;
    };
    for (name, value) in [
        ("foundational_capability", current.foundational_capability),
        ("competitive_capability", current.competitive_capability),
        ("semantic_vector", current.semantic_vector),
        ("stability", current.stability),
    ] {
        if value + RATIO_EPSILON < previous_number(previous, name) {
            reasons.push(format!("{name} regressed"));
        }
    }
    if let Some(value) = current.research_judge {
        if value + RATIO_EPSILON < previous_number(previous, "research_judge") {
            reasons.push("research_judge regressed".to_owned());
        }
    }
    if reasons.iter().any(|reason| reason.contains("regressed")) {
        return reasons;
    }
    if let Some(reason) =
        profile_best_score_reject_reason(current, baselines.profile_best_accepted, bug_fix_priority)
    {
        reasons.push(reason);
    }
    if bug_fix_priority
        || current.score > previous_number(previous, "score") + SCORE_EPSILON
        || pareto_improved(current, previous)
    {
        return reasons;
    }
    let metric_improvement_count = improvements
        .iter()
        .filter(|item| item.get("kind").and_then(Value::as_str) == Some("metric"))
        .count();
    if metric_improvement_count > 0 {
        reasons.push(format!(
            "local metric improvements ({metric_improvement_count}) did not beat latest baseline score delta {:+.6}",
            current.score - previous_number(previous, "score")
        ));
    }
    reasons.push("candidate did not improve score or tracked objectives beyond epsilon".to_owned());
    reasons
}

fn profile_best_score_reject_reason(
    current: ScoreComponents,
    profile_best_accepted: Option<&Value>,
    bug_fix_priority: bool,
) -> Option<String> {
    if bug_fix_priority {
        return None;
    }
    let previous = profile_best_accepted?;
    let profile_best_score = previous_number(previous, "score");
    if current.score > profile_best_score + SCORE_EPSILON {
        return None;
    }
    Some(format!(
        "candidate score {:.6} did not beat profile best accepted score {:.6} beyond epsilon",
        current.score, profile_best_score
    ))
}

fn changes(
    observation: &EvaluationObservation,
    current: ScoreComponents,
    previous_run: Option<&Value>,
    improved: bool,
) -> Vec<Value> {
    let Some(previous) = previous_run else {
        return Vec::new();
    };
    let mut changes = Vec::new();
    for (name, value) in [
        ("score", current.score),
        ("foundational_capability", current.foundational_capability),
        ("competitive_capability", current.competitive_capability),
        ("semantic_vector", current.semantic_vector),
        ("performance", current.performance),
        ("stability", current.stability),
    ] {
        push_score_change(
            &mut changes,
            name,
            value,
            previous_number(previous, name),
            improved,
        );
    }
    if let Some(value) = current.research_judge {
        push_score_change(
            &mut changes,
            "research_judge",
            value,
            previous_number(previous, "research_judge"),
            improved,
        );
    }
    for gate in &observation.gates {
        let Some(previous_passed) = previous_gate_passed(previous, &gate.name) else {
            continue;
        };
        if gate.passed != previous_passed
            && ((improved && gate.passed) || (!improved && !gate.passed))
        {
            changes.push(serde_json::json!({
                "kind": "gate",
                "name": gate.name,
                "previous": previous_passed,
                "current": gate.passed
            }));
        }
    }
    for case in &observation.cases {
        let Some(previous_case) = previous_case(previous, &case.case_id) else {
            continue;
        };
        if case.passed != previous_case.passed
            && ((improved && case.passed) || (!improved && !case.passed))
        {
            changes.push(serde_json::json!({
                "kind": "case",
                "name": case.case_id,
                "previous": previous_case.passed,
                "current": case.passed
            }));
            continue;
        }
        push_case_quality_changes(&mut changes, case, previous_case, improved);
    }
    let previous_metrics = previous_metrics(previous);
    for metric in &observation.metrics {
        if let Some(previous_value) = previous_metrics.get(&metric.name).copied() {
            let threshold =
                (previous_value.abs() * METRIC_RELATIVE_EPSILON).max(METRIC_ABSOLUTE_EPSILON);
            let delta = metric.value - previous_value;
            let better = if metric.lower_is_better {
                delta < -threshold
            } else {
                delta > threshold
            };
            let worse = if metric.lower_is_better {
                delta > threshold
            } else {
                delta < -threshold
            };
            if (improved && better) || (!improved && worse) {
                changes.push(serde_json::json!({
                    "kind": "metric",
                    "name": metric.name,
                    "previous": previous_value,
                    "current": metric.value
                }));
            }
        }
    }
    changes
}

fn weighted_score(
    foundational: f64,
    competitive: f64,
    semantic: f64,
    research: Option<f64>,
    performance: f64,
    stability: f64,
) -> f64 {
    if let Some(research) = research {
        foundational * 0.17
            + competitive * 0.17
            + semantic * 0.10
            + research * 0.22
            + performance * 0.15
            + stability * 0.19
    } else {
        foundational * 0.22
            + competitive * 0.22
            + semantic * 0.13
            + performance * 0.18
            + stability * 0.25
    }
}

fn capability_ceiling_bonus(
    current: ScoreComponents,
    baselines: ScoreBaselines<'_>,
    has_key_performance_metrics: bool,
) -> f64 {
    let baseline = CapabilityBaseline::new(baselines);
    if !baseline.available {
        return 0.0;
    }
    let mut weighted_gain = 0.0;
    let mut total_weight = 0.0;
    let mut components = vec![
        (
            "competitive_capability",
            current.competitive_capability,
            0.35,
        ),
        ("semantic_vector", current.semantic_vector, 0.15),
    ];
    if has_key_performance_metrics {
        components.push(("performance", current.performance, 0.20));
    }
    for (name, value, weight) in components {
        if let Some(gain) = normalized_ceiling_gain(value, baseline.number(name)) {
            weighted_gain += gain * weight;
            total_weight += weight;
        }
    }
    if let Some(research) = current.research_judge {
        if let Some(gain) = normalized_ceiling_gain(research, baseline.number("research_judge")) {
            weighted_gain += gain * 0.30;
            total_weight += 0.30;
        }
    }
    if total_weight == 0.0 {
        return 0.0;
    }
    clamp(weighted_gain / total_weight) * CAPABILITY_CEILING_MAX_BONUS
}

struct CapabilityBaseline {
    available: bool,
    workload_previous: Option<Value>,
    profile_best_accepted: Option<Value>,
}

impl CapabilityBaseline {
    fn new(baselines: ScoreBaselines<'_>) -> Self {
        Self {
            available: baselines.workload_previous.is_some()
                || baselines.profile_best_accepted.is_some(),
            workload_previous: baselines.workload_previous.cloned(),
            profile_best_accepted: baselines.profile_best_accepted.cloned(),
        }
    }

    fn number(&self, name: &str) -> Option<f64> {
        [
            self.workload_previous.as_ref(),
            self.profile_best_accepted.as_ref(),
        ]
        .into_iter()
        .flatten()
        .filter_map(|run| run.get(name).and_then(Value::as_f64))
        .reduce(f64::max)
    }
}

fn normalized_ceiling_gain(current: f64, baseline: Option<f64>) -> Option<f64> {
    let baseline = baseline?;
    if current <= baseline + RATIO_EPSILON {
        return None;
    }
    let remaining = (1.0 - baseline).max(RATIO_EPSILON);
    Some(((current - baseline) / remaining).clamp(0.0, 1.0))
}

fn performance_score(metrics: &[MetricObservation], previous_run: Option<&Value>) -> f64 {
    let key_metrics = metrics
        .iter()
        .filter(|metric| metric.key)
        .collect::<Vec<_>>();
    if key_metrics.is_empty() {
        return 1.0;
    }
    let previous_metrics = previous_run.map(previous_metrics).unwrap_or_default();
    let scores = key_metrics
        .into_iter()
        .map(|metric| {
            let budget_score = metric.score();
            let Some(previous) = previous_metrics.get(&metric.name).copied() else {
                return budget_score;
            };
            let ratio = if metric.lower_is_better {
                previous / metric.value.max(1.0)
            } else {
                metric.value / previous.max(1.0)
            };
            (budget_score * 0.7 + ratio.min(1.25) / 1.25 * 0.3).min(1.0)
        })
        .collect::<Vec<_>>();
    average(&scores, 1.0)
}

fn stability_score(gates: &[GateObservation]) -> f64 {
    if gates.is_empty() {
        return 1.0;
    }
    gates.iter().filter(|gate| gate.passed).count() as f64 / gates.len() as f64
}

fn pareto_improved(current: ScoreComponents, previous: &Value) -> bool {
    let mut improved = false;
    for (name, value) in [
        ("foundational_capability", current.foundational_capability),
        ("competitive_capability", current.competitive_capability),
        ("semantic_vector", current.semantic_vector),
        ("performance", current.performance),
        ("stability", current.stability),
    ] {
        let previous_value = previous_number(previous, name);
        if value + RATIO_EPSILON < previous_value {
            return false;
        }
        improved |= value > previous_value + RATIO_EPSILON;
    }
    if let Some(value) = current.research_judge {
        let previous_value = previous_number(previous, "research_judge");
        if value + RATIO_EPSILON < previous_value {
            return false;
        }
        improved |= value > previous_value + RATIO_EPSILON;
    }
    improved
}

fn push_score_change(
    changes: &mut Vec<Value>,
    name: &str,
    current: f64,
    previous: f64,
    improved: bool,
) {
    let delta = current - previous;
    if (improved && delta > RATIO_EPSILON) || (!improved && delta < -RATIO_EPSILON) {
        changes.push(serde_json::json!({
            "kind": "score_component",
            "name": name,
            "previous": previous,
            "current": current,
        }));
    }
}

fn metric_budget_failures(metrics: &[MetricObservation]) -> Vec<Value> {
    metrics
        .iter()
        .filter(|metric| metric.key && metric.budget.is_some() && metric.score() < 1.0)
        .map(|metric| {
            serde_json::json!({
                "name": metric.name,
                "value": metric.value,
                "budget": metric.budget,
            })
        })
        .collect()
}

fn bug_fix_priority_improved(improvements: &[Value]) -> bool {
    improvements.iter().any(|item| {
        matches!(
            item.get("kind").and_then(Value::as_str),
            Some("case" | "gate")
        )
    })
}

fn objective_scores(cases: &[CaseObservation], objective: &str, aliases: &[&str]) -> Vec<f64> {
    cases
        .iter()
        .filter(|case| case.objective == objective || aliases.contains(&case.objective.as_str()))
        .map(CaseObservation::score)
        .collect()
}

fn previous_number(run: &Value, name: &str) -> f64 {
    run.get(name).and_then(Value::as_f64).unwrap_or(0.0)
}

fn push_case_quality_changes(
    changes: &mut Vec<Value>,
    case: &CaseObservation,
    previous: PreviousCase,
    improved: bool,
) {
    let current_score = case.score();
    let score_delta = current_score - previous.score;
    if (improved && score_delta > CASE_SCORE_EPSILON)
        || (!improved && score_delta < -CASE_SCORE_EPSILON)
    {
        changes.push(serde_json::json!({
            "kind": "case_score",
            "name": case.case_id,
            "previous": previous.score,
            "current": current_score
        }));
    }
    let rank_better = optional_rank_better(case.rank, previous.rank);
    if (improved && rank_better == Some(true)) || (!improved && rank_better == Some(false)) {
        changes.push(serde_json::json!({
            "kind": "case_rank",
            "name": case.case_id,
            "previous": previous.rank,
            "current": case.rank
        }));
    }
    if case.false_positive_count != previous.false_positive_count
        && ((improved && case.false_positive_count < previous.false_positive_count)
            || (!improved && case.false_positive_count > previous.false_positive_count))
    {
        changes.push(serde_json::json!({
            "kind": "case_false_positive_count",
            "name": case.case_id,
            "previous": previous.false_positive_count,
            "current": case.false_positive_count
        }));
    }
}

fn optional_rank_better(current: Option<usize>, previous: Option<usize>) -> Option<bool> {
    match (current, previous) {
        (Some(current), Some(previous)) if current != previous => Some(current < previous),
        (Some(_), None) => Some(true),
        (None, Some(_)) => Some(false),
        _ => None,
    }
}

fn previous_case(run: &Value, case_id: &str) -> Option<PreviousCase> {
    let case = run
        .get("cases")
        .and_then(Value::as_array)
        .and_then(|cases| {
            cases
                .iter()
                .find(|case| case.get("case_id").and_then(Value::as_str) == Some(case_id))
        })?;
    let passed = case.get("passed").and_then(Value::as_bool)?;
    let rank = case
        .get("rank")
        .and_then(Value::as_u64)
        .map(|value| value as usize);
    let false_positive_count = case
        .get("false_positive_count")
        .and_then(Value::as_u64)
        .map(|value| value as usize)
        .unwrap_or_default();
    Some(PreviousCase {
        passed,
        rank,
        false_positive_count,
        score: previous_case_score(case, passed, rank, false_positive_count),
    })
}

fn previous_case_score(
    case: &Value,
    passed: bool,
    rank: Option<usize>,
    false_positive_count: usize,
) -> f64 {
    if !passed {
        return 0.0;
    }
    if let Some(score) = case.get("score_override").and_then(Value::as_f64) {
        return clamp(score);
    }
    let rank_score = match rank {
        Some(rank) if rank > 0 => 1.0 / rank as f64,
        _ => 1.0,
    };
    (rank_score - (false_positive_count as f64 * 0.1).min(0.5)).max(0.0)
}

fn previous_gate_passed(run: &Value, gate_name: &str) -> Option<bool> {
    run.get("gates")
        .and_then(Value::as_array)
        .and_then(|gates| {
            gates
                .iter()
                .find(|gate| gate.get("name").and_then(Value::as_str) == Some(gate_name))
        })
        .and_then(|gate| gate.get("passed"))
        .and_then(Value::as_bool)
}

fn previous_metrics(run: &Value) -> BTreeMap<String, f64> {
    run.get("metrics")
        .and_then(Value::as_array)
        .map(|metrics| {
            metrics
                .iter()
                .filter_map(|metric| {
                    Some((
                        metric.get("name")?.as_str()?.to_owned(),
                        metric.get("value")?.as_f64()?,
                    ))
                })
                .collect()
        })
        .unwrap_or_default()
}

pub fn array_field<'a>(value: &'a Value, name: &str) -> &'a [Value] {
    value
        .get(name)
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or(&[])
}

fn usize_field(value: &Value, name: &str, default: usize) -> usize {
    value
        .get(name)
        .and_then(Value::as_u64)
        .map(|value| value as usize)
        .unwrap_or(default)
}

fn average(values: &[f64], default: f64) -> f64 {
    if values.is_empty() {
        default
    } else {
        values.iter().sum::<f64>() / values.len() as f64
    }
}

fn clamp(value: f64) -> f64 {
    value.clamp(0.0, 1.0)
}

#[cfg(test)]
include!("scoring_tests.rs");
