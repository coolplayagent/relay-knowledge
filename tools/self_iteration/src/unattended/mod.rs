use serde::{Deserialize, Serialize};

use crate::{
    PersistInput, apply_candidate_documentation_gate, candidate_git, cases, codex, command,
    config::{CategorySet, Config, EvaluationCategory, Strategy},
    evaluate_candidate_for_patch, evaluator, history, new_layer_run_id, number,
    persist_scored_run_with_score, print_score, scoring, sleep_seconds, unix_timestamp,
    write_adopted_optimization_document,
};

const UNATTENDED_ACCEPT_LIMIT: usize = 8;
const COMPETITIVE_GAP_EPSILON: f64 = 0.02;
const CATEGORY_ROTATION: [EvaluationCategory; 4] = [
    EvaluationCategory::Competitive,
    EvaluationCategory::SemanticVector,
    EvaluationCategory::Performance,
    EvaluationCategory::RepositorySets,
];

#[derive(Debug, Clone, Serialize, Deserialize)]
struct UnattendedState {
    strategy: String,
    started_at: u64,
    last_updated_at: u64,
    accepted_count: usize,
    cycle_count: usize,
    category_index: usize,
    consecutive_empty_candidates: usize,
    consecutive_promotion_failures: usize,
    competitive_promotion_failures: usize,
    last_deep_check_at: u64,
    completed: bool,
    completion_reason: Option<String>,
}

impl UnattendedState {
    fn new(now: u64) -> Self {
        Self {
            strategy: Strategy::UnattendedLayered.label().to_owned(),
            started_at: now,
            last_updated_at: now,
            accepted_count: 0,
            cycle_count: 0,
            category_index: 0,
            consecutive_empty_candidates: 0,
            consecutive_promotion_failures: 0,
            competitive_promotion_failures: 0,
            last_deep_check_at: now,
            completed: false,
            completion_reason: None,
        }
    }

    fn elapsed_seconds(&self, now: u64) -> u64 {
        now.saturating_sub(self.started_at)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LayerAttemptKind {
    Explore,
    MacroExplore,
}

impl LayerAttemptKind {
    fn label(self) -> &'static str {
        match self {
            Self::Explore => "explore",
            Self::MacroExplore => "macro_explore",
        }
    }

    fn timeout_seconds(self, config: &Config) -> u64 {
        match self {
            Self::Explore => config.explore_timeout_seconds,
            Self::MacroExplore => config.macro_explore_timeout_seconds,
        }
    }

    fn is_macro(self) -> bool {
        self == Self::MacroExplore
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LayeredCycleOutcome {
    Accepted,
    Rejected,
    EmptyCandidate,
    CodexTimeout,
    CodexFailed,
}

impl LayeredCycleOutcome {
    fn should_retry_explore(self) -> bool {
        matches!(
            self,
            Self::EmptyCandidate | Self::CodexTimeout | Self::CodexFailed
        )
    }
}

include!("lifecycle.rs");
include!("state.rs");
include!("cycle.rs");
include!("attempt.rs");
include!("evaluation.rs");
include!("configuration.rs");
include!("metadata.rs");
include!("category_rotation.rs");
include!("triggers.rs");
include!("deep_check.rs");
include!("outcome.rs");
