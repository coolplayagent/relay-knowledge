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

fn canonical_metric_output(callers: u64, callees: u64) -> String {
    format!(
        "SELF_ITERATION_METRIC {{\"name\":\"canonical_call_callers_vm_steps\",\"value\":{callers},\"budget\":150000}}\nSELF_ITERATION_METRIC {{\"name\":\"canonical_call_callees_vm_steps\",\"value\":{callees},\"budget\":150000}}\n"
    )
}

#[test]
fn canonical_work_gate_rejects_old_scan_and_accepts_index_bounded_work() {
    for (callers, callees, expected) in [
        (31_500, 32_000, true),
        (1_045_800, 32_000, false),
        (31_500, 1_045_800, false),
    ] {
        let mut metrics = Vec::new();
        let mut gates = Vec::new();
        let mut commands = Vec::new();
        let passed = run_quality_gate_plan(
            vec![QualityGateStage::Parallel(vec![gate(
                "canonical_call_query_work_budget",
            )])],
            |_| {
                let mut result = result("canonical_call_query_work_budget", 0);
                result.stdout = canonical_metric_output(callers, callees);
                vec![result]
            },
            &mut commands,
            &mut gates,
            &mut metrics,
        );
        assert_eq!(passed, expected);
        assert_eq!(gates[0].passed, expected);
        let work = metrics
            .iter()
            .filter(|metric| metric.name.ends_with("_vm_steps"))
            .collect::<Vec<_>>();
        assert_eq!(work.len(), 2);
        assert!(work.iter().all(|metric| metric.key
            && metric.lower_is_better
            && metric.budget == Some(150_000.0)));
    }
}

#[test]
fn canonical_work_gate_fails_closed_for_missing_or_invalid_observations() {
    let good = canonical_metric_output(31_500, 32_000);
    for stdout in [
        String::new(),
        "running 0 tests".to_owned(),
        "SELF_ITERATION_METRIC invalid".to_owned(),
        good.lines().next().unwrap().to_owned(),
        format!("{good}{good}"),
        good.replace("150000", "2000000"),
        good.replace("31500", "0"),
        good.replace("31500", "-1"),
        good.replace("31500", "1.5"),
        good.replace("canonical_call_callers_vm_steps", "unknown"),
    ] {
        let mut metrics = Vec::new();
        let mut gates = Vec::new();
        let mut commands = Vec::new();
        assert!(!run_quality_gate_plan(
            vec![QualityGateStage::Parallel(vec![gate(
                "canonical_call_query_work_budget"
            )])],
            |_| {
                let mut result = result("canonical_call_query_work_budget", 0);
                result.stdout = stdout.clone();
                vec![result]
            },
            &mut commands,
            &mut gates,
            &mut metrics
        ));
        assert!(!gates[0].passed);
    }
    assert!(
        super::query_work_metrics(
            &format!("test canonical_call_query_work_budget ... {good}"),
            super::CANONICAL_WORK_METRICS
        )
        .is_ok()
    );
}

fn feature_flag_metric_output(narrow: u64, exhausted: u64) -> String {
    [
        ("feature_flag_narrow_vm_steps", narrow, 2_000_000),
        ("feature_flag_exhausted_vm_steps", exhausted, 4_097_000),
    ]
    .into_iter()
    .map(|(name, value, budget)| {
        format!(
            "SELF_ITERATION_METRIC {}\n",
            serde_json::json!({"name":name,"value":value,"budget":budget})
        )
    })
    .collect()
}

#[test]
fn feature_flag_work_gate_rejects_unbounded_seed_work_and_missing_evidence() {
    let good = feature_flag_metric_output(500_000, 4_097_000);
    for (stdout, expected) in [
        (good.clone(), true),
        (feature_flag_metric_output(2_000_001, 4_097_000), false),
        (feature_flag_metric_output(500_000, 9_299_000), false),
        (feature_flag_metric_output(0, 4_097_000), false),
        ("running 0 tests".to_owned(), false),
        (good.lines().next().unwrap().to_owned(), false),
        (format!("{good}{good}"), false),
        (good.replace("4097000", "9299000"), false),
        (canonical_metric_output(19_000, 19_000), false),
    ] {
        let mut metrics = Vec::new();
        let mut gates = Vec::new();
        let mut commands = Vec::new();
        let passed = run_quality_gate_plan(
            vec![QualityGateStage::Parallel(vec![gate(
                "feature_flag_query_work_budget",
            )])],
            |_| {
                let mut output = result("feature_flag_query_work_budget", 0);
                output.stdout = stdout.clone();
                vec![output]
            },
            &mut commands,
            &mut gates,
            &mut metrics,
        );
        assert_eq!(passed, expected, "{stdout}");
        assert_eq!(gates[0].passed, expected);
        if expected {
            for (name, budget) in super::FEATURE_FLAG_WORK_METRICS {
                let metric = metrics.iter().find(|m| m.name == *name).unwrap();
                assert!(metric.key && metric.lower_is_better);
                assert_eq!(metric.budget, Some(*budget as f64));
            }
        }
    }
}
