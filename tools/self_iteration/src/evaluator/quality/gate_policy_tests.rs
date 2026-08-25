use super::{quality_budget_ms, quality_gate_stages};
use crate::config::ProductBinaryProfile;
use crate::evaluator::quality::QualityGateStage;

#[test]
fn full_profile_quality_gates_run_in_dependency_stages() {
    let stages = quality_gate_stages("full", Some(ProductBinaryProfile::Release));

    assert_eq!(stages.len(), 5);
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
        QualityGateStage::Parallel(gates) => {
            assert_eq!(
                gates.iter().map(|gate| gate.name).collect::<Vec<_>>(),
                vec!["bm25_hierarchy_build"]
            );
        }
        QualityGateStage::Rails(_) => panic!("BM25 build should have an isolated stage"),
    }
    match &stages[3] {
        QualityGateStage::Parallel(gates) => {
            assert_eq!(
                gates.iter().map(|gate| gate.name).collect::<Vec<_>>(),
                vec!["bm25_hierarchy_suite"]
            );
        }
        QualityGateStage::Rails(_) => panic!("BM25 measurement should have an isolated stage"),
    }
    match &stages[4] {
        QualityGateStage::Rails(rails) => {
            let rail_names = rails
                .iter()
                .map(|rail| rail.iter().map(|gate| gate.name).collect::<Vec<_>>())
                .collect::<Vec<_>>();
            assert_eq!(
                rail_names,
                vec![
                    vec!["cargo_clippy", "cargo_test"],
                    vec!["self_iteration_cargo_clippy", "self_iteration_cargo_test"]
                ]
            );
        }
        QualityGateStage::Parallel(_) => panic!("clippy/test gates should use rails"),
    }
}

#[test]
fn fast_profile_skips_full_quality_gates() {
    let stages = quality_gate_stages("fast", Some(ProductBinaryProfile::Release));

    assert_eq!(stages.len(), 6);
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
    assert!(gate_names.contains(&"cargo_build_release"));
    assert!(gate_names.contains(&"code_index_recovery_cases"));
    assert!(gate_names.contains(&"code_index_sqlite_lock_cases"));
    assert!(gate_names.contains(&"bm25_hierarchy_build"));
    assert!(gate_names.contains(&"bm25_hierarchy_suite"));
    assert!(gate_names.contains(&"code_index_persistence_performance_suite"));
    assert!(gate_names.contains(&"self_iteration_cargo_check"));
    assert!(gate_names.contains(&"linux_glibc_compatibility_policy"));
    assert!(gate_names.contains(&"skill_metadata_policy_cases"));
    assert!(!gate_names.contains(&"cargo_build_debug"));
    assert!(!gate_names.contains(&"cargo_clippy"));
    assert!(!gate_names.contains(&"cargo_test"));
}

#[test]
fn smoke_profile_does_not_build_a_product_binary() {
    let gate_names = quality_gate_stages("smoke", None)
        .into_iter()
        .flat_map(|stage| match stage {
            QualityGateStage::Parallel(gates) => gates,
            QualityGateStage::Rails(rails) => rails.into_iter().flatten().collect(),
        })
        .map(|gate| gate.name)
        .collect::<Vec<_>>();

    assert_eq!(
        gate_names,
        vec!["cargo_fmt_check", "self_iteration_cargo_fmt_check"]
    );
}

#[test]
fn quality_budgets_cover_key_builds_and_leave_unknown_gates_unbounded() {
    assert_eq!(quality_budget_ms("cargo_build_release"), Some(180_000.0));
    assert_eq!(quality_budget_ms("bm25_hierarchy_build"), None);
    assert_eq!(quality_budget_ms("bm25_hierarchy_suite"), Some(30_000.0));
    assert_eq!(
        quality_budget_ms("code_index_persistence_performance_suite"),
        Some(30_000.0)
    );
    assert_eq!(quality_budget_ms("unknown"), None);
}

#[test]
fn fast_code_index_persistence_measurement_is_a_bounded_isolated_stage() {
    let stages = quality_gate_stages("fast", Some(ProductBinaryProfile::Release));
    let stage = stages
        .iter()
        .find(|stage| stage_has_gate(stage, "code_index_persistence_performance_suite"))
        .expect("code-index persistence performance gate should be selected");
    let gate = only_parallel_gate(stage);

    assert_eq!(
        gate.command,
        vec![
            "cargo",
            "test",
            "--lib",
            "--all-features",
            "code_index_persistence_performance_suite",
            "--",
            "--nocapture"
        ]
    );
    assert_eq!(gate.timeout_seconds, 120);
}

#[test]
fn bm25_hierarchy_build_and_measurement_are_bounded_isolated_stages() {
    for profile in ["fast", "full", "exhaustive"] {
        let stages = quality_gate_stages(profile, Some(ProductBinaryProfile::Release));
        let build_stage = stages
            .iter()
            .position(|stage| stage_has_gate(stage, "bm25_hierarchy_build"))
            .expect("hierarchical BM25 build gate should be selected");
        let measurement_stage = stages
            .iter()
            .position(|stage| stage_has_gate(stage, "bm25_hierarchy_suite"))
            .expect("hierarchical BM25 measurement gate should be selected");
        assert_eq!(measurement_stage, build_stage + 1);

        let build = only_parallel_gate(&stages[build_stage]);
        assert_eq!(
            build.command,
            vec!["cargo", "test", "--lib", "--all-features", "--no-run"]
        );
        assert_eq!(build.timeout_seconds, 1200);

        let gate = only_parallel_gate(&stages[measurement_stage]);
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

fn stage_has_gate(stage: &QualityGateStage, expected: &str) -> bool {
    match stage {
        QualityGateStage::Parallel(gates) => gates.iter().any(|gate| gate.name == expected),
        QualityGateStage::Rails(rails) => rails.iter().flatten().any(|gate| gate.name == expected),
    }
}

fn only_parallel_gate(stage: &QualityGateStage) -> &crate::evaluator::quality::QualityGate {
    let QualityGateStage::Parallel(gates) = stage else {
        panic!("isolated measurement gates must not share a rail")
    };
    assert_eq!(
        gates.len(),
        1,
        "measurement stages must not share a Cargo lock"
    );
    &gates[0]
}

#[test]
fn product_build_gate_targets_only_the_selected_release_binary() {
    let gate = quality_gate_stages("fast", Some(ProductBinaryProfile::Release))
        .into_iter()
        .flat_map(|stage| match stage {
            QualityGateStage::Parallel(gates) => gates,
            QualityGateStage::Rails(rails) => rails.into_iter().flatten().collect(),
        })
        .find(|gate| gate.name == "cargo_build_release")
        .expect("release product build gate");

    assert_eq!(
        gate.command,
        vec!["cargo", "build", "--release", "--bin", "relay-knowledge"]
    );
}
