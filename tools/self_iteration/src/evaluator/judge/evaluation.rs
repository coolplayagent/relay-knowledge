use std::fs;

use serde_json::Value;

use crate::scoring::{CaseObservation, GateObservation};

use super::super::runtime::{contracts::RepoReport, reporting::repo_report};
use super::{
    JudgeEvalInput,
    backend::run_judge_backend,
    outcome::judge_outcome,
    prompt::{JudgePromptInput, build_judge_prompt},
    settings::{judge_settings, settings_summary},
};

pub(in crate::evaluator) fn evaluate_research_judge_suite(
    input: JudgeEvalInput<'_>,
) -> Result<RepoReport, String> {
    let settings = judge_settings(input.env);
    let mut report = repo_report(
        "research_judge",
        "judge".to_owned(),
        Vec::new(),
        Vec::new(),
        Vec::new(),
        settings_summary(&settings),
    );
    if !settings.enabled {
        report.gates.push(GateObservation {
            name: "research_judge".to_owned(),
            passed: true,
            duration_ms: 0,
            message: "judge skipped: backend disabled".to_owned(),
        });
        return Ok(report);
    }
    if let Some(error) = &settings.configuration_error {
        report.gates.push(GateObservation {
            name: "research_judge".to_owned(),
            passed: false,
            duration_ms: 0,
            message: format!("judge misconfigured: {error}"),
        });
        return Ok(report);
    }
    if !settings.missing.is_empty() {
        report.gates.push(GateObservation {
            name: "research_judge".to_owned(),
            passed: false,
            duration_ms: 0,
            message: format!(
                "judge misconfigured: missing {}",
                settings.missing.join(", ")
            ),
        });
        return Ok(report);
    }
    let prompt = build_judge_prompt(JudgePromptInput {
        workspace: input.workspace,
        suite: input.suite,
        generated_diff: input.generated_diff,
        candidate_diff: input.candidate_diff,
        gates: input.gates,
        cases: input.cases,
        metrics: input.metrics,
        repo_reports: input.repo_reports,
    });
    let prompt_file = input.run_home.join("judge-prompt.txt");
    fs::write(&prompt_file, &prompt)
        .map_err(|error| format!("failed to write {}: {error}", prompt_file.display()))?;
    let result = run_judge_backend(&input, &settings, &prompt_file, &prompt)?;
    let outcome = if result.passed() {
        judge_outcome(
            &format!("{}\n{}", result.stdout, result.stderr),
            input.suite,
        )
    } else {
        (false, false, 0.0, result.gate_message(), Value::Null)
    };
    report.gates.push(GateObservation {
        name: "research_judge".to_owned(),
        passed: outcome.0,
        duration_ms: result.duration_ms,
        message: outcome.3.clone(),
    });
    report.cases.push(CaseObservation {
        case_id: "research_judge".to_owned(),
        repository: "research_judge".to_owned(),
        passed: outcome.1,
        guardrail: false,
        rank: outcome.1.then_some(1),
        max_rank: 1,
        false_positive_count: 0,
        message: outcome.3,
        objective: "research_judge".to_owned(),
        score_override: Some(outcome.2),
    });
    report.index_summary = outcome.4;
    report.commands.push(result);
    Ok(report)
}

#[cfg(test)]
#[path = "evaluation_tests.rs"]
mod tests;
