use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::command::CommandResult;

const SCORE_EPSILON: f64 = 0.0005;
const RATIO_EPSILON: f64 = 0.005;
const CASE_SCORE_EPSILON: f64 = 0.005;
const METRIC_RELATIVE_EPSILON: f64 = 0.03;
const METRIC_ABSOLUTE_EPSILON: f64 = 25.0;

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
    pub accepted: bool,
    pub reject_reasons: Vec<String>,
    pub performance_strategy: String,
    pub degradations: Vec<Value>,
    pub improvements: Vec<Value>,
    pub metric_budget_failures: Vec<Value>,
}

#[derive(Debug, Clone)]
pub struct RankedAssessment {
    pub rank: Option<usize>,
    pub false_positive_count: usize,
    pub score: f64,
    pub details: String,
    pub failures: Vec<String>,
}

pub fn assess_ranked_hits(
    case: &Value,
    hits: &[Value],
    expected: &[Value],
    forbidden: &[Value],
) -> RankedAssessment {
    let max_rank = usize_field(case, "max_rank", 1);
    let rank = first_expected_rank(hits, expected);
    let false_positive_ranks = hits
        .iter()
        .enumerate()
        .filter_map(|(index, hit)| hit_matches_any(hit, forbidden).then_some(index + 1))
        .collect::<Vec<_>>();
    let expected_all = array_field(case, "expected_all");
    let expected_sequence = array_field(case, "expected_sequence");
    let all_ranks = first_match_ranks(hits, expected_all);
    let sequence_ranks = first_match_ranks(hits, expected_sequence);
    let all_score = (!expected_all.is_empty()).then(|| coverage_score(&all_ranks));
    let sequence_score =
        (!expected_sequence.is_empty()).then(|| sequence_quality_score(&sequence_ranks));
    let forbidden_penalty = ranked_forbidden_penalty(
        &false_positive_ranks,
        case.get("forbidden_rank_penalty")
            .and_then(Value::as_f64)
            .unwrap_or(0.1),
    );
    let score = ranked_case_score(
        rank,
        !expected.is_empty(),
        all_score,
        sequence_score,
        forbidden_penalty,
    );
    let failures = ranked_failures(RankedFailureInput {
        case,
        expected,
        rank,
        max_rank,
        false_positive_ranks: &false_positive_ranks,
        all_ranks: &all_ranks,
        sequence_ranks: &sequence_ranks,
        sequence_score,
        score,
    });
    RankedAssessment {
        rank,
        false_positive_count: false_positive_ranks.len(),
        score,
        details: ranked_details(
            score,
            &all_ranks,
            &sequence_ranks,
            forbidden_penalty,
            &failures,
        ),
        failures,
    }
}

pub fn hit_matches_any(hit: &Value, patterns: &[Value]) -> bool {
    patterns.iter().any(|pattern| hit_matches(hit, pattern))
}

fn hit_matches(hit: &Value, pattern: &Value) -> bool {
    for key in [
        "path",
        "relative_path",
        "file_name",
        "extension",
        "status",
        "edge_resolution_state",
        "repository_alias",
        "source_scope",
    ] {
        if let Some(expected) = pattern.get(key).and_then(Value::as_str) {
            if hit.get(key).and_then(Value::as_str) != Some(expected) {
                return false;
            }
        }
    }
    if let Some(expected) = pattern.get("line_start").and_then(Value::as_i64) {
        let start = hit
            .get("line_range")
            .and_then(|range| range.get("start"))
            .and_then(Value::as_i64)
            .unwrap_or(-1);
        let end = hit
            .get("line_range")
            .and_then(|range| range.get("end"))
            .and_then(Value::as_i64)
            .unwrap_or(-1);
        if !(start <= expected && expected <= end || start == expected) {
            return false;
        }
    }
    if let Some(expected) = pattern.get("edge_target_hint").and_then(Value::as_str) {
        if !hit
            .get("edge_target_hint")
            .and_then(Value::as_str)
            .unwrap_or("")
            .contains(expected)
        {
            return false;
        }
    }
    for (key, hit_key) in [
        ("excerpt_contains", "excerpt"),
        ("content_contains", "content"),
    ] {
        if let Some(expected) = pattern.get(key).and_then(Value::as_str) {
            if !hit
                .get(hit_key)
                .and_then(Value::as_str)
                .unwrap_or("")
                .contains(expected)
            {
                return false;
            }
        }
    }
    if let Some(expected) = pattern.get("retriever_source").and_then(Value::as_str) {
        let sources = hit
            .get("retriever_sources")
            .and_then(Value::as_array)
            .map(Vec::as_slice)
            .unwrap_or(&[]);
        if !sources
            .iter()
            .any(|source| source.as_str() == Some(expected))
        {
            return false;
        }
    }
    true
}

pub fn score_evaluation(
    observation: &EvaluationObservation,
    previous_run: Option<&Value>,
) -> ScoreBreakdown {
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
    let stability = stability_score(&observation.gates, observation.generated_diff);
    let score = weighted_score(
        foundational_capability,
        competitive_capability,
        semantic_vector,
        research_judge,
        performance,
        stability,
    );
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
        previous_run,
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
    previous_run: Option<&Value>,
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
    let Some(previous) = previous_run else {
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
    if bug_fix_priority_improved(improvements)
        || current.score > previous_number(previous, "score") + SCORE_EPSILON
        || pareto_improved(current, previous)
    {
        return reasons;
    }
    reasons.push("candidate did not improve score or tracked objectives beyond epsilon".to_owned());
    reasons
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

fn first_expected_rank(hits: &[Value], expected: &[Value]) -> Option<usize> {
    hits.iter()
        .enumerate()
        .find_map(|(index, hit)| hit_matches_any(hit, expected).then_some(index + 1))
}

fn first_match_ranks(hits: &[Value], patterns: &[Value]) -> Vec<Option<usize>> {
    patterns
        .iter()
        .map(|pattern| {
            hits.iter()
                .enumerate()
                .find_map(|(index, hit)| hit_matches(hit, pattern).then_some(index + 1))
        })
        .collect()
}

struct RankedFailureInput<'a> {
    case: &'a Value,
    expected: &'a [Value],
    rank: Option<usize>,
    max_rank: usize,
    false_positive_ranks: &'a [usize],
    all_ranks: &'a [Option<usize>],
    sequence_ranks: &'a [Option<usize>],
    sequence_score: Option<f64>,
    score: f64,
}

fn ranked_failures(input: RankedFailureInput<'_>) -> Vec<String> {
    let mut failures = Vec::new();
    if !input.expected.is_empty() && input.rank.is_none_or(|value| value > input.max_rank) {
        failures.push(format!("rank={:?} max_rank={}", input.rank, input.max_rank));
    }
    if !input.false_positive_ranks.is_empty()
        && !input
            .case
            .get("forbidden_rank_penalty_only")
            .and_then(Value::as_bool)
            .unwrap_or(false)
    {
        failures.push(format!(
            "false_positives={}",
            input.false_positive_ranks.len()
        ));
    }
    if !input.all_ranks.is_empty()
        && input
            .case
            .get("require_expected_all")
            .and_then(Value::as_bool)
            .unwrap_or(true)
        && input.all_ranks.iter().any(Option::is_none)
    {
        failures.push(format!(
            "expected_all={}/{}",
            matched_count(input.all_ranks),
            input.all_ranks.len()
        ));
    }
    if !input.sequence_ranks.is_empty()
        && input
            .case
            .get("require_expected_sequence")
            .and_then(Value::as_bool)
            .unwrap_or(true)
        && input.sequence_score.is_some_and(|value| value < 1.0)
    {
        failures.push(format!(
            "expected_sequence={}/{}",
            matched_count(input.sequence_ranks),
            input.sequence_ranks.len()
        ));
    }
    if let Some(min_score) = input.case.get("min_score").and_then(Value::as_f64) {
        if input.score < min_score {
            failures.push(format!("score={:.3} min_score={min_score:.3}", input.score));
        }
    }
    failures
}

fn ranked_details(
    score: f64,
    all_ranks: &[Option<usize>],
    sequence_ranks: &[Option<usize>],
    forbidden_penalty: f64,
    failures: &[String],
) -> String {
    let mut details = vec![format!("score={score:.3}")];
    if !all_ranks.is_empty() {
        details.push(format!(
            "expected_all={}/{}",
            matched_count(all_ranks),
            all_ranks.len()
        ));
    }
    if !sequence_ranks.is_empty() {
        details.push(format!(
            "expected_sequence={}/{}",
            matched_count(sequence_ranks),
            sequence_ranks.len()
        ));
    }
    if forbidden_penalty > 0.0 {
        details.push(format!("forbidden_penalty={forbidden_penalty:.3}"));
    }
    if !failures.is_empty() {
        details.push(format!("failures={}", failures.join("; ")));
    }
    details.join(" ")
}

fn ranked_case_score(
    rank: Option<usize>,
    has_primary_expected: bool,
    all_score: Option<f64>,
    sequence_score: Option<f64>,
    forbidden_penalty: f64,
) -> f64 {
    let rank_score = if has_primary_expected {
        rank.map(|value| 1.0 / value.max(1) as f64).unwrap_or(0.0)
    } else {
        1.0
    };
    let mut components = vec![rank_score];
    if let Some(score) = all_score {
        components.push(score);
    }
    if let Some(score) = sequence_score {
        components.push(score);
    }
    clamp(average(&components, 1.0) - forbidden_penalty)
}

fn coverage_score(ranks: &[Option<usize>]) -> f64 {
    if ranks.is_empty() {
        return 1.0;
    }
    matched_count(ranks) as f64 / ranks.len() as f64
}

fn sequence_quality_score(ranks: &[Option<usize>]) -> f64 {
    if ranks.is_empty() {
        return 1.0;
    }
    let matched = ranks.iter().flatten().copied().collect::<Vec<_>>();
    let coverage = matched.len() as f64 / ranks.len() as f64;
    if matched.len() <= 1 {
        return coverage;
    }
    let ordered_pairs = matched.windows(2).filter(|pair| pair[0] <= pair[1]).count();
    coverage * ordered_pairs as f64 / (matched.len() - 1) as f64
}

fn ranked_forbidden_penalty(ranks: &[usize], weight: f64) -> f64 {
    ranks
        .iter()
        .map(|rank| weight / (*rank).max(1) as f64)
        .sum::<f64>()
        .min(0.5)
}

fn matched_count(ranks: &[Option<usize>]) -> usize {
    ranks.iter().filter(|rank| rank.is_some()).count()
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

fn stability_score(gates: &[GateObservation], generated_diff: bool) -> f64 {
    if !generated_diff {
        return 0.0;
    }
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
