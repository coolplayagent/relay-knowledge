use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::PathBuf,
    sync::{Arc, Condvar, Mutex},
    time::Instant,
};

use serde_json::Value;

use crate::{
    cases::{array_field, number_or, object_field, objects_by_repository, string_or},
    command::{CommandResult, CommandSpec, inherited_env, run_command},
    config::{Config, JobPlan},
    history::HistoryPaths,
    scoring::{CaseObservation, EvaluationObservation, GateObservation, MetricObservation},
};

mod fixtures;
mod judge;
mod quality;
mod workloads;

use fixtures::{prepare_repository_path, write_fixture_file};
use judge::{JudgeEvalInput, evaluate_research_judge_suite};
use quality::run_quality_gate_stages;
use workloads::{
    WorkloadSelection, evaluate_agent_workflows, evaluate_cli_contract_cases,
    evaluate_file_fixtures, evaluate_registration_cases, evaluate_repository,
    evaluate_repository_sets, evaluate_semantic_vector_suite, evaluation_home, is_guardrail_case,
    relay_knowledge_binary, repository_in_profile, select_repository_cases_for_profile,
    selected_repository_set_member_names, semantic_vector_suite_for_selection,
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
include!("runtime/finish.rs");
include!("runtime/concurrency.rs");
include!("runtime/reporting.rs");

#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;
