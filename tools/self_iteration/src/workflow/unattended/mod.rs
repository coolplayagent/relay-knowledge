use serde::{Deserialize, Serialize};

use crate::config::{Config, EvaluationCategory, Strategy};

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

struct UnattendedAttemptInput<'a> {
    config: &'a Config,
    paths: &'a crate::history::HistoryPaths,
    cases_config: &'a serde_json::Value,
    state: &'a mut UnattendedState,
    kind: LayerAttemptKind,
    category: EvaluationCategory,
    attempt_index: usize,
    macro_trigger: Option<&'a str>,
}

struct UnattendedEvaluationInput<'a> {
    config: &'a Config,
    paths: &'a crate::history::HistoryPaths,
    run_id: &'a str,
    patch: &'a crate::candidate_git::PatchSnapshot,
    codex: Option<&'a crate::codex::CodexResult>,
    metadata: serde_json::Value,
    commit: bool,
    base_ref: &'a str,
}

#[derive(Default)]
struct MetadataLinks<'a> {
    parent_run_id: Option<&'a str>,
    promoted_from_run_id: Option<&'a str>,
    macro_trigger: Option<&'a str>,
    promotion_decision: Option<&'a str>,
}

struct MetadataPersistInput<'a> {
    config: &'a Config,
    paths: &'a crate::history::HistoryPaths,
    run_id: &'a str,
    patch: &'a crate::candidate_git::PatchSnapshot,
    codex: Option<&'a crate::codex::CodexResult>,
    evaluation: &'a crate::evaluator::EvaluationRun,
    commit: Option<&'a str>,
    metadata: &'a serde_json::Value,
}

mod attempt;
mod category_rotation;
mod configuration;
mod cycle;
mod deep_check;
mod evaluation;
mod lifecycle;
mod metadata;
mod outcome;
mod state;
mod triggers;

pub(super) use lifecycle::run_unattended_layered_loop;
