mod agent_workflow;
mod case_scoring;
mod cli_cases;
mod file_evaluation;
mod file_fixture;
mod repository;
mod repository_scoring;
mod repository_set;
mod selection;
mod semantic_vector;
mod semantic_vector_evaluation;

use crate::{
    command::CommandResult,
    scoring::{CaseObservation, GateObservation, MetricObservation},
};

#[derive(Debug, Clone)]
pub(in crate::evaluator) struct FileReport {
    pub(in crate::evaluator) commands: Vec<CommandResult>,
    pub(in crate::evaluator) cases: Vec<CaseObservation>,
    pub(in crate::evaluator) metrics: Vec<MetricObservation>,
}

#[derive(Debug, Clone)]
pub(in crate::evaluator) struct RegistrationCaseReport {
    pub(in crate::evaluator) commands: Vec<CommandResult>,
    pub(in crate::evaluator) cases: Vec<CaseObservation>,
    pub(in crate::evaluator) gates: Vec<GateObservation>,
}

#[derive(Debug, Clone)]
pub(in crate::evaluator) struct CliContractReport {
    pub(in crate::evaluator) commands: Vec<CommandResult>,
    pub(in crate::evaluator) cases: Vec<CaseObservation>,
    pub(in crate::evaluator) gates: Vec<GateObservation>,
}

pub(super) use agent_workflow::evaluate_agent_workflows;
pub(super) use cli_cases::{evaluate_cli_contract_cases, evaluate_registration_cases};
pub(super) use file_evaluation::evaluate_file_fixtures;
pub(super) use repository::evaluate_repository;
pub(super) use repository_set::{evaluate_repository_sets, selected_repository_set_member_names};
pub(super) use selection::{
    WorkloadSelection, evaluation_home, relay_knowledge_binary, repository_in_profile,
    select_repository_cases_for_profile, semantic_vector_suite_for_selection,
};
pub(super) use semantic_vector_evaluation::evaluate_semantic_vector_suite;
