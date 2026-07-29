use std::{collections::BTreeMap, path::Path};

use serde_json::Value;

use crate::scoring::{CaseObservation, GateObservation, MetricObservation};

use super::{Limiter, RepoReport};

mod backend;
mod evaluation;
mod outcome;
mod prompt;
mod settings;

pub(in crate::evaluator) use evaluation::evaluate_research_judge_suite;

pub(in crate::evaluator) struct JudgeEvalInput<'a> {
    pub(in crate::evaluator) workspace: &'a Path,
    pub(in crate::evaluator) run_home: &'a Path,
    pub(in crate::evaluator) env: &'a BTreeMap<String, String>,
    pub(in crate::evaluator) suite: &'a Value,
    pub(in crate::evaluator) generated_diff: bool,
    pub(in crate::evaluator) candidate_diff: &'a str,
    pub(in crate::evaluator) gates: &'a [GateObservation],
    pub(in crate::evaluator) cases: &'a [CaseObservation],
    pub(in crate::evaluator) metrics: &'a [MetricObservation],
    pub(in crate::evaluator) repo_reports: &'a [RepoReport],
    pub(in crate::evaluator) limiter: &'a Limiter,
}
