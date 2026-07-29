use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{Arc, Condvar, Mutex},
    time::Instant,
};

use serde_json::Value;

use crate::{
    cases::{
        array_field, number_or, object_field, objects_by_repository, string_field, string_or,
        string_vec,
    },
    command::{CommandResult, CommandSpec, inherited_env, run_command},
    config::{CategorySet, Config, EvaluationCategory, JobPlan},
    history::HistoryPaths,
    scoring::{
        CaseObservation, EvaluationObservation, GateObservation, MetricObservation,
        RankedAssessment, array_field as score_array_field, assess_ranked_hits, hit_matches_any,
    },
};

#[derive(Debug, Clone)]
pub struct EvaluationRun {
    pub observation: EvaluationObservation,
    pub report: Value,
}

#[derive(Debug, Clone)]
struct RepoReport {
    repository: String,
    scope: String,
    commands: Vec<CommandResult>,
    gates: Vec<GateObservation>,
    cases: Vec<CaseObservation>,
    metrics: Vec<MetricObservation>,
    index_summary: Value,
}

#[derive(Debug, Clone)]
struct FileReport {
    commands: Vec<CommandResult>,
    cases: Vec<CaseObservation>,
    metrics: Vec<MetricObservation>,
}

#[derive(Debug, Clone)]
struct RegistrationCaseReport {
    commands: Vec<CommandResult>,
    cases: Vec<CaseObservation>,
    gates: Vec<GateObservation>,
}

#[derive(Debug, Clone)]
struct CliContractReport {
    commands: Vec<CommandResult>,
    cases: Vec<CaseObservation>,
    gates: Vec<GateObservation>,
}

#[derive(Debug, Clone)]
struct EvalRuntime {
    binary: PathBuf,
    workspace: PathBuf,
    env: BTreeMap<String, String>,
    timeout: u64,
    limiter: Limiter,
    writer_lock: Arc<Mutex<()>>,
    query_jobs: usize,
}

#[derive(Debug, Clone)]
struct QualityGate {
    name: &'static str,
    command: Vec<String>,
    timeout_seconds: u64,
}

#[derive(Debug, Clone)]
enum QualityGateStage {
    Parallel(Vec<QualityGate>),
    Rails(Vec<Vec<QualityGate>>),
}

#[derive(Debug, Clone)]
struct Limiter {
    inner: Arc<(Mutex<usize>, Condvar)>,
}

struct Permit {
    inner: Arc<(Mutex<usize>, Condvar)>,
}

impl Limiter {
    fn new(limit: usize) -> Self {
        Self {
            inner: Arc::new((Mutex::new(limit.max(1)), Condvar::new())),
        }
    }

    fn acquire(&self) -> Permit {
        let (lock, condvar) = &*self.inner;
        let mut available = lock.lock().expect("limiter lock should not be poisoned");
        while *available == 0 {
            available = condvar
                .wait(available)
                .expect("limiter lock should not be poisoned");
        }
        *available -= 1;
        Permit {
            inner: Arc::clone(&self.inner),
        }
    }
}

impl Drop for Permit {
    fn drop(&mut self) {
        let (lock, condvar) = &*self.inner;
        let mut available = lock.lock().expect("limiter lock should not be poisoned");
        *available += 1;
        condvar.notify_one();
    }
}

include!("runtime/orchestration.rs");
include!("workloads/repository.rs");
include!("workloads/file_evaluation.rs");
include!("workloads/semantic_vector_evaluation.rs");
include!("runtime/finish.rs");
include!("quality/gate_execution.rs");
include!("runtime/concurrency.rs");
include!("workloads/repository_set.rs");
include!("workloads/agent_workflow.rs");
include!("quality/gate_policy.rs");
include!("workloads/selection.rs");
include!("workloads/cli_cases.rs");
include!("workloads/repository_scoring.rs");
include!("fixtures/repository.rs");
include!("fixtures/c_and_cpp.rs");
include!("fixtures/cross_language.rs");
include!("fixtures/common_languages.rs");
include!("fixtures/additional_languages.rs");
include!("fixtures/nonstandard_layout.rs");
include!("fixtures/software_global.rs");
include!("workloads/file_fixture.rs");
include!("workloads/semantic_vector.rs");
include!("judge/evaluation.rs");
include!("judge/settings.rs");
include!("judge/prompt.rs");
include!("judge/backend.rs");
include!("judge/outcome.rs");
include!("runtime/reporting.rs");

#[cfg(test)]
#[path = "workloads/repository_set_tests.rs"]
mod repository_set_tests;

#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;
