use std::time::{SystemTime, UNIX_EPOCH};

use crate::{
    candidate_git::{self, PatchSnapshot},
    cases, codex, command,
    config::{Config, Mode, Strategy},
    evaluator, history, research_plan,
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
include!("pacing.rs");
