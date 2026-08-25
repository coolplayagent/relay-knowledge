use super::{quality_gate_names, quality_gate_stage_label, run_quality_gate_plan};
use crate::evaluator::quality::{QualityGate, QualityGateStage};
use crate::{command::CommandResult, scoring::GateObservation};

fn gate(name: &'static str) -> QualityGate {
    QualityGate {
        name,
        command: vec!["true".to_owned()],
        timeout_seconds: 1,
    }
}

fn result(name: &str, exit_code: i32) -> CommandResult {
    CommandResult {
        name: name.to_owned(),
        command: vec![name.to_owned()],
        exit_code,
        duration_ms: match name {
            "bm25_hierarchy_build" => 240_000,
            "bm25_hierarchy_suite" => 9_000,
            "code_index_persistence_performance_suite" => 100,
            _ => 1,
        },
        stdout: String::new(),
        stderr: String::new(),
    }
}

#[test]
fn code_index_persistence_measurement_is_reported_as_a_key_budgeted_metric() {
    let stages = vec![QualityGateStage::Parallel(vec![gate(
        "code_index_persistence_performance_suite",
    )])];
    let mut commands = Vec::new();
    let mut gates = Vec::<GateObservation>::new();
    let mut metrics = Vec::new();

    assert!(run_quality_gate_plan(
        stages,
        |stage| stage_results(stage, 0),
        &mut commands,
        &mut gates,
        &mut metrics,
    ));
    assert_eq!(metrics.len(), 1);
    assert_eq!(
        metrics[0].name,
        "code_index_persistence_performance_suite_ms"
    );
    assert_eq!(metrics[0].value, 100.0);
    assert_eq!(metrics[0].budget, Some(30_000.0));
    assert!(metrics[0].key);
}

fn stage_results(stage: QualityGateStage, exit_code: i32) -> Vec<CommandResult> {
    match stage {
        QualityGateStage::Parallel(gates) => gates,
        QualityGateStage::Rails(rails) => rails.into_iter().flatten().collect(),
    }
    .into_iter()
    .map(|gate| result(gate.name, exit_code))
    .collect()
}

#[test]
fn stage_labels_preserve_parallel_and_rail_topology() {
    let parallel = QualityGateStage::Parallel(vec![gate("fmt"), gate("check")]);
    let rails = QualityGateStage::Rails(vec![
        vec![gate("clippy"), gate("test")],
        vec![gate("harness_clippy"), gate("harness_test")],
    ]);

    assert_eq!(
        quality_gate_names(&[gate("fmt"), gate("check")]),
        "fmt,check"
    );
    assert_eq!(
        quality_gate_stage_label(&parallel),
        "parallel gates=fmt,check"
    );
    assert_eq!(
        quality_gate_stage_label(&rails),
        "rails rail1=clippy,test; rail2=harness_clippy,harness_test"
    );
}

#[test]
fn gate_plan_runs_bm25_build_before_the_isolated_measurement_stage() {
    let stages = vec![
        QualityGateStage::Parallel(vec![gate("bm25_hierarchy_build")]),
        QualityGateStage::Parallel(vec![gate("bm25_hierarchy_suite")]),
    ];
    let mut executed = Vec::new();
    let mut commands = Vec::new();
    let mut gates = Vec::<GateObservation>::new();
    let mut metrics = Vec::new();

    let passed = run_quality_gate_plan(
        stages,
        |stage| {
            executed.push(quality_gate_stage_label(&stage));
            stage_results(stage, 0)
        },
        &mut commands,
        &mut gates,
        &mut metrics,
    );

    assert!(passed);
    assert_eq!(
        executed,
        [
            "parallel gates=bm25_hierarchy_build",
            "parallel gates=bm25_hierarchy_suite"
        ]
    );
    assert_eq!(
        commands
            .iter()
            .map(|command| command.name.as_str())
            .collect::<Vec<_>>(),
        ["bm25_hierarchy_build", "bm25_hierarchy_suite"]
    );
    assert_eq!(metrics.len(), 2);
    assert_eq!(metrics[0].name, "bm25_hierarchy_build_ms");
    assert_eq!(metrics[0].value, 240_000.0);
    assert_eq!(metrics[0].budget, None);
    assert!(!metrics[0].key);
    assert_eq!(metrics[1].name, "bm25_hierarchy_suite_ms");
    assert_eq!(metrics[1].value, 9_000.0);
    assert_eq!(metrics[1].budget, Some(30_000.0));
    assert!(!metrics[1].key);
}

#[test]
fn gate_plan_does_not_measure_bm25_when_its_build_stage_fails() {
    let stages = vec![
        QualityGateStage::Parallel(vec![gate("bm25_hierarchy_build")]),
        QualityGateStage::Parallel(vec![gate("bm25_hierarchy_suite")]),
    ];
    let mut executed = Vec::new();
    let mut commands = Vec::new();
    let mut gates = Vec::<GateObservation>::new();
    let mut metrics = Vec::new();

    let passed = run_quality_gate_plan(
        stages,
        |stage| {
            let label = quality_gate_stage_label(&stage);
            let exit_code = if label.contains("bm25_hierarchy_build") {
                1
            } else {
                0
            };
            executed.push(label);
            stage_results(stage, exit_code)
        },
        &mut commands,
        &mut gates,
        &mut metrics,
    );

    assert!(!passed);
    assert_eq!(executed, ["parallel gates=bm25_hierarchy_build"]);
    assert_eq!(commands.len(), 1);
}
