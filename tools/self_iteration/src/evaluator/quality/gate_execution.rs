use std::{path::Path, time::Instant};

use crate::{
    command::{CommandResult, CommandSpec},
    scoring::{GateObservation, MetricObservation},
};

use super::super::{Limiter, parallel_map, run_limited};
use super::{
    QualityGate, QualityGateStage,
    gate_policy::{quality_budget_ms, quality_gate_stages},
};

pub(in crate::evaluator) fn run_quality_gate_stages(
    profile: &str,
    workspace: &Path,
    limiter: &Limiter,
    commands: &mut Vec<CommandResult>,
    gates: &mut Vec<GateObservation>,
    metrics: &mut Vec<MetricObservation>,
) -> bool {
    let stages = quality_gate_stages(profile);
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
        for result in run_quality_gate_stage(stage, workspace, limiter) {
            stage_gate_count += 1;
            metrics.push(MetricObservation {
                name: format!("{}_ms", result.name),
                value: result.duration_ms as f64,
                budget: quality_budget_ms(&result.name),
                lower_is_better: true,
                key: matches!(
                    result.name.as_str(),
                    "cargo_build_release" | "cargo_build_debug"
                ),
            });
            gates.push(GateObservation::from_command(&result));
            stage_passed &= result.passed();
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
