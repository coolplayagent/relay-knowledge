use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::command::CommandResult;

const RATIO_EPSILON: f64 = 0.005;

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
            return score_math::clamp(score);
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

mod capability;
mod case_fields;
mod change_detection;
mod decision;
mod evaluation;
mod ranked;
mod score_math;

pub use case_fields::array_field;
pub use evaluation::score_evaluation;
pub use ranked::{RankedAssessment, assess_ranked_hits, hit_matches_any};

#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;
