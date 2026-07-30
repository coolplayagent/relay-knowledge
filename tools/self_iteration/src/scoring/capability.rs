use serde_json::Value;

use super::{
    GateObservation, MetricObservation, RATIO_EPSILON, ScoreBaselines, ScoreComponents,
    change_detection::{previous_metrics, previous_number},
    score_math::{average, clamp},
};

const CAPABILITY_CEILING_MAX_BONUS: f64 = 0.06;

pub(super) fn weighted_score(
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

pub(super) fn capability_ceiling_bonus(
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

pub(super) fn performance_score(
    metrics: &[MetricObservation],
    previous_run: Option<&Value>,
) -> f64 {
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

pub(super) fn stability_score(gates: &[GateObservation]) -> f64 {
    if gates.is_empty() {
        return 1.0;
    }
    gates.iter().filter(|gate| gate.passed).count() as f64 / gates.len() as f64
}

pub(super) fn pareto_improved(current: ScoreComponents, previous: &Value) -> bool {
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

#[cfg(test)]
#[path = "capability_tests.rs"]
mod capability_tests;
