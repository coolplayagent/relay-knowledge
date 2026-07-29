use std::{
    collections::BTreeMap,
    path::PathBuf,
    sync::{Arc, Condvar, Mutex},
};

use serde_json::Value;

use crate::{
    command::CommandResult,
    scoring::{CaseObservation, EvaluationObservation, GateObservation, MetricObservation},
};

#[derive(Debug, Clone)]
pub struct EvaluationRun {
    pub observation: EvaluationObservation,
    pub report: Value,
}

#[derive(Debug, Clone)]
pub(in crate::evaluator) struct RepoReport {
    pub(in crate::evaluator) repository: String,
    pub(in crate::evaluator) scope: String,
    pub(in crate::evaluator) commands: Vec<CommandResult>,
    pub(in crate::evaluator) gates: Vec<GateObservation>,
    pub(in crate::evaluator) cases: Vec<CaseObservation>,
    pub(in crate::evaluator) metrics: Vec<MetricObservation>,
    pub(in crate::evaluator) index_summary: Value,
}

#[derive(Debug, Clone)]
pub(in crate::evaluator) struct EvalRuntime {
    pub(in crate::evaluator) binary: PathBuf,
    pub(in crate::evaluator) workspace: PathBuf,
    pub(in crate::evaluator) env: BTreeMap<String, String>,
    pub(in crate::evaluator) timeout: u64,
    pub(in crate::evaluator) limiter: Limiter,
    pub(in crate::evaluator) writer_lock: Arc<Mutex<()>>,
    pub(in crate::evaluator) query_jobs: usize,
}

#[derive(Debug, Clone)]
pub(in crate::evaluator) struct Limiter {
    pub(super) inner: Arc<(Mutex<usize>, Condvar)>,
}
