use super::{quality_gate_names, quality_gate_stage_label};
use crate::evaluator::quality::{QualityGate, QualityGateStage};

fn gate(name: &'static str) -> QualityGate {
    QualityGate {
        name,
        command: vec!["true".to_owned()],
        timeout_seconds: 1,
    }
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
