mod gate_execution;
mod gate_policy;

pub(super) use gate_execution::run_quality_gate_stages;

#[derive(Debug, Clone)]
struct QualityGate {
    name: &'static str,
    command: Vec<String>,
    timeout_seconds: u64,
}

#[derive(Debug, Clone)]
enum QualityGateStage {
    Parallel(Vec<QualityGate>),
    Rails(Vec<Vec<QualityGate>>),
}
