use std::time::{SystemTime, UNIX_EPOCH};

use crate::{
    cases, codex, command,
    config::{Config, Mode, Strategy},
    evaluator,
    git_ops::{self, PatchSnapshot},
    history, research_plan,
    scoring::{self, EvaluationObservation, GateObservation},
    unattended,
};

include!("dispatch.rs");
include!("loop_control.rs");
include!("manual_evaluation.rs");
include!("generation_iteration.rs");
include!("candidate_evaluation.rs");
include!("documentation_gate.rs");
include!("persistence.rs");
include!("report_metadata.rs");
include!("adopted_documentation.rs");
include!("output.rs");
include!("run_identity.rs");

#[cfg(test)]
#[path = "run_identity_tests.rs"]
mod run_identity_tests;

#[cfg(test)]
#[path = "documentation_gate_tests.rs"]
mod documentation_gate_tests;
