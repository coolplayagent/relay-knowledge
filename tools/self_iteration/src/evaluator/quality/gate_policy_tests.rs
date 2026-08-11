use super::{quality_budget_ms, quality_gate_stages};
use crate::evaluator::quality::QualityGateStage;

#[test]
fn full_profile_quality_gates_run_in_dependency_stages() {
    let stages = quality_gate_stages("full");

    assert_eq!(stages.len(), 3);
    match &stages[0] {
        QualityGateStage::Parallel(gates) => {
            assert_eq!(
                gates.iter().map(|gate| gate.name).collect::<Vec<_>>(),
                vec![
                    "cargo_fmt_check",
                    "self_iteration_cargo_fmt_check",
                    "linux_glibc_compatibility_policy"
                ]
            );
        }
        QualityGateStage::Rails(_) => panic!("fmt gates should be parallel"),
    }
    match &stages[1] {
        QualityGateStage::Parallel(gates) => {
            assert_eq!(
                gates.iter().map(|gate| gate.name).collect::<Vec<_>>(),
                vec!["cargo_build_release", "self_iteration_cargo_build_release"]
            );
        }
        QualityGateStage::Rails(_) => panic!("build gates should be parallel"),
    }
    match &stages[2] {
        QualityGateStage::Rails(rails) => {
            let rail_names = rails
                .iter()
                .map(|rail| rail.iter().map(|gate| gate.name).collect::<Vec<_>>())
                .collect::<Vec<_>>();
            assert_eq!(
                rail_names,
                vec![
                    vec!["cargo_clippy", "bm25_hierarchy_suite", "cargo_test"],
                    vec!["self_iteration_cargo_clippy", "self_iteration_cargo_test"]
                ]
            );
        }
        QualityGateStage::Parallel(_) => panic!("clippy/test gates should use rails"),
    }
}

#[test]
fn fast_profile_skips_full_quality_gates() {
    let stages = quality_gate_stages("fast");

    assert_eq!(stages.len(), 3);
    let gate_names = stages
        .iter()
        .flat_map(|stage| match stage {
            QualityGateStage::Parallel(gates) => {
                gates.iter().map(|gate| gate.name).collect::<Vec<_>>()
            }
            QualityGateStage::Rails(rails) => rails
                .iter()
                .flat_map(|rail| rail.iter().map(|gate| gate.name))
                .collect::<Vec<_>>(),
        })
        .collect::<Vec<_>>();
    assert!(gate_names.contains(&"cargo_build_debug"));
    assert!(gate_names.contains(&"code_index_recovery_cases"));
    assert!(gate_names.contains(&"code_index_sqlite_lock_cases"));
    assert!(gate_names.contains(&"bm25_hierarchy_suite"));
    assert!(gate_names.contains(&"self_iteration_cargo_check"));
    assert!(gate_names.contains(&"linux_glibc_compatibility_policy"));
    assert!(gate_names.contains(&"skill_metadata_policy_cases"));
    assert!(!gate_names.contains(&"cargo_build_release"));
    assert!(!gate_names.contains(&"cargo_clippy"));
    assert!(!gate_names.contains(&"cargo_test"));
}

#[test]
fn quality_budgets_cover_key_builds_and_leave_unknown_gates_unbounded() {
    assert_eq!(quality_budget_ms("cargo_build_debug"), Some(90_000.0));
    assert_eq!(quality_budget_ms("cargo_build_release"), Some(180_000.0));
    assert_eq!(quality_budget_ms("bm25_hierarchy_suite"), Some(30_000.0));
    assert_eq!(quality_budget_ms("unknown"), None);
}

#[test]
fn bm25_hierarchy_gate_runs_the_named_deterministic_product_suite() {
    for profile in ["fast", "full", "exhaustive"] {
        let gate = quality_gate_stages(profile)
            .into_iter()
            .flat_map(|stage| match stage {
                QualityGateStage::Parallel(gates) => gates,
                QualityGateStage::Rails(rails) => rails.into_iter().flatten().collect(),
            })
            .find(|gate| gate.name == "bm25_hierarchy_suite")
            .expect("hierarchical BM25 gate should be selected");

        assert_eq!(
            gate.command,
            vec![
                "cargo",
                "test",
                "--lib",
                "--all-features",
                "bm25_hierarchy_suite",
                "--",
                "--nocapture"
            ]
        );
        assert_eq!(gate.timeout_seconds, 120);
    }
}
