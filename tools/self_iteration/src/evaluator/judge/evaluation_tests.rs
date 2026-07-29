use std::{collections::BTreeMap, path::Path};

use super::evaluate_research_judge_suite;
use crate::evaluator::{JudgeEvalInput, Limiter};

#[test]
fn disabled_backend_records_an_observable_skip_without_running_a_command() {
    let env = BTreeMap::from([(
        "RELAY_KNOWLEDGE_JUDGE_BACKEND".to_owned(),
        "none".to_owned(),
    )]);
    let suite = serde_json::json!({});
    let limiter = Limiter::new(1);

    let report = evaluate_research_judge_suite(JudgeEvalInput {
        workspace: Path::new("."),
        run_home: Path::new("."),
        env: &env,
        suite: &suite,
        generated_diff: false,
        candidate_diff: "",
        gates: &[],
        cases: &[],
        metrics: &[],
        repo_reports: &[],
        limiter: &limiter,
    })
    .expect("disabled judge should produce a report");

    assert!(report.commands.is_empty());
    assert_eq!(report.gates.len(), 1);
    assert!(report.gates[0].passed);
    assert_eq!(report.gates[0].message, "judge skipped: backend disabled");
}
