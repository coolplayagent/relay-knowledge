use std::{path::Path, time::Instant};

use crate::{
    command::{CommandResult, CommandSpec},
    config::Config,
    scoring::{GateObservation, MetricObservation},
};

use super::super::runtime::{
    concurrency::{parallel_map, run_limited},
    contracts::Limiter,
};
use super::{
    QualityGate, QualityGateStage,
    gate_policy::{quality_budget_ms, quality_gate_stages},
};

pub(in crate::evaluator) fn run_quality_gate_stages(
    config: &Config,
    workspace: &Path,
    limiter: &Limiter,
    commands: &mut Vec<CommandResult>,
    gates: &mut Vec<GateObservation>,
    metrics: &mut Vec<MetricObservation>,
) -> bool {
    let stages = quality_gate_stages(&config.profile, config.product_binary_profile());
    run_quality_gate_plan(
        stages,
        |stage| run_quality_gate_stage(stage, workspace, limiter),
        commands,
        gates,
        metrics,
    )
}

fn run_quality_gate_plan(
    stages: Vec<QualityGateStage>,
    mut run_stage: impl FnMut(QualityGateStage) -> Vec<CommandResult>,
    commands: &mut Vec<CommandResult>,
    gates: &mut Vec<GateObservation>,
    metrics: &mut Vec<MetricObservation>,
) -> bool {
    let stage_count = stages.len();
    for (stage_index, stage) in stages.into_iter().enumerate() {
        let stage_started = Instant::now();
        let stage_label = quality_gate_stage_label(&stage);
        eprintln!(
            "[self-iterate] quality stage {}/{} start {}",
            stage_index + 1,
            stage_count,
            stage_label
        );
        let mut stage_passed = true;
        let mut stage_gate_count = 0usize;
        for result in run_stage(stage) {
            stage_gate_count += 1;
            metrics.push(MetricObservation {
                name: format!("{}_ms", result.name),
                value: result.duration_ms as f64,
                budget: quality_budget_ms(&result.name),
                lower_is_better: true,
                key: matches!(
                    result.name.as_str(),
                    "cargo_build_release"
                        | "cargo_build_debug"
                        | "code_index_persistence_performance_suite"
                ),
            });
            let mut observation = GateObservation::from_command(&result);
            if result.name == "canonical_call_query_work_budget" {
                match canonical_work_metrics(&result.stdout) {
                    Ok(work_metrics) => {
                        let in_budget = work_metrics
                            .iter()
                            .all(|metric| metric.value <= metric.budget.unwrap_or_default());
                        observation.passed &= in_budget;
                        if !in_budget {
                            observation.message =
                                "Canonical call SQL VM-step budget exceeded".to_owned();
                        }
                        metrics.extend(work_metrics);
                    }
                    Err(error) => {
                        observation.passed = false;
                        observation.message = error;
                    }
                }
            }
            stage_passed &= observation.passed;
            gates.push(observation);
            commands.push(result);
        }
        eprintln!(
            "[self-iterate] quality stage {}/{} done passed={} duration_ms={} gates={}",
            stage_index + 1,
            stage_count,
            stage_passed,
            stage_started.elapsed().as_millis(),
            stage_gate_count
        );
        if !stage_passed {
            eprintln!("[self-iterate] quality gates failed; skipping evaluation workload");
            return false;
        }
    }
    true
}

fn canonical_work_metrics(stdout: &str) -> Result<Vec<MetricObservation>, String> {
    const NAMES: [&str; 2] = [
        "canonical_call_callers_vm_steps",
        "canonical_call_callees_vm_steps",
    ];
    const MAX_VM_STEPS: u64 = 150_000;
    let mut observations = std::collections::BTreeMap::new();
    for line in stdout.lines() {
        let Some((_, json)) = line.split_once("SELF_ITERATION_METRIC ") else {
            continue;
        };
        let metric: serde_json::Value = serde_json::from_str(json)
            .map_err(|error| format!("Invalid SQL work metric: {error}"))?;
        let name = metric
            .get("name")
            .and_then(serde_json::Value::as_str)
            .filter(|name| NAMES.contains(name))
            .ok_or("Unexpected SQL work metric name")?;
        let value = metric
            .get("value")
            .and_then(serde_json::Value::as_u64)
            .filter(|value| *value > 0)
            .ok_or("SQL work metric must be a positive integer")?;
        if metric.get("budget").and_then(serde_json::Value::as_u64) != Some(MAX_VM_STEPS) {
            return Err("SQL work metric budget does not match the harness contract".to_owned());
        }
        if observations
            .insert(
                name.to_owned(),
                MetricObservation {
                    name: name.to_owned(),
                    value: value as f64,
                    budget: Some(MAX_VM_STEPS as f64),
                    lower_is_better: true,
                    key: true,
                },
            )
            .is_some()
        {
            return Err(format!("Duplicate SQL work metric: {name}"));
        }
    }
    if observations.len() != NAMES.len() {
        return Err(
            "Missing callers/callees SQL work metrics; the performance test must execute"
                .to_owned(),
        );
    }
    Ok(observations.into_values().collect())
}

fn quality_gate_stage_label(stage: &QualityGateStage) -> String {
    match stage {
        QualityGateStage::Parallel(gates) => {
            format!("parallel gates={}", quality_gate_names(gates))
        }
        QualityGateStage::Rails(rails) => {
            let rails = rails
                .iter()
                .enumerate()
                .map(|(index, rail)| format!("rail{}={}", index + 1, quality_gate_names(rail)))
                .collect::<Vec<_>>()
                .join("; ");
            format!("rails {rails}")
        }
    }
}

fn quality_gate_names(gates: &[QualityGate]) -> String {
    gates
        .iter()
        .map(|gate| gate.name)
        .collect::<Vec<_>>()
        .join(",")
}

fn run_quality_gate_stage(
    stage: QualityGateStage,
    workspace: &Path,
    limiter: &Limiter,
) -> Vec<CommandResult> {
    match stage {
        QualityGateStage::Parallel(gates) => run_parallel_quality_gates(gates, workspace, limiter),
        QualityGateStage::Rails(rails) => run_quality_gate_rails(rails, workspace, limiter),
    }
}

fn run_parallel_quality_gates(
    gates: Vec<QualityGate>,
    workspace: &Path,
    limiter: &Limiter,
) -> Vec<CommandResult> {
    let jobs = gates.len();
    let workspace = workspace.to_path_buf();
    let limiter = limiter.clone();
    let mut indexed_results = parallel_map(
        gates.into_iter().enumerate().collect(),
        jobs,
        move |(index, gate)| {
            let result = run_limited(
                &limiter,
                CommandSpec::new(
                    gate.name,
                    gate.command,
                    &workspace,
                    None,
                    gate.timeout_seconds,
                ),
            );
            (index, result)
        },
    );
    indexed_results.sort_by_key(|(index, _)| *index);
    indexed_results
        .into_iter()
        .map(|(_, result)| result)
        .collect()
}

fn run_quality_gate_rails(
    rails: Vec<Vec<QualityGate>>,
    workspace: &Path,
    limiter: &Limiter,
) -> Vec<CommandResult> {
    let jobs = rails.len();
    let workspace = workspace.to_path_buf();
    let limiter = limiter.clone();
    let mut indexed_rails = parallel_map(
        rails.into_iter().enumerate().collect(),
        jobs,
        move |(rail_index, rail)| {
            let mut rail_results = Vec::new();
            for gate in rail {
                let result = run_limited(
                    &limiter,
                    CommandSpec::new(
                        gate.name,
                        gate.command,
                        &workspace,
                        None,
                        gate.timeout_seconds,
                    ),
                );
                let passed = result.passed();
                rail_results.push(result);
                if !passed {
                    break;
                }
            }
            (rail_index, rail_results)
        },
    );
    indexed_rails.sort_by_key(|(rail_index, _)| *rail_index);
    indexed_rails
        .into_iter()
        .flat_map(|(_, results)| results)
        .collect()
}

#[cfg(test)]
#[path = "gate_execution_tests.rs"]
mod tests;
