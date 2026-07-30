use std::{path::PathBuf, time::Instant};

use super::{FinishInput, finish};
use crate::{
    config::{Config, JobPlan},
    evaluator::workloads::WorkloadSelection,
};

#[test]
fn finish_serializes_empty_observation_and_parallelism_contract() {
    let config = Config::parse(vec!["evaluate".to_owned()]).expect("config should parse");
    let job_plan = JobPlan::resolve(&config);
    let selection = WorkloadSelection::new(&config);

    let run = finish(FinishInput {
        config: &config,
        generated_diff: false,
        gates: Vec::new(),
        cases: Vec::new(),
        metrics: Vec::new(),
        commands: Vec::new(),
        repo_reports: Vec::new(),
        run_home: PathBuf::from("."),
        cached_home: true,
        job_plan,
        selection,
        started: Instant::now(),
    })
    .expect("finish should serialize");

    assert!(run.observation.gates.is_empty());
    assert_eq!(run.report["cached_home"], true);
    assert!(
        run.report["parallelism"]["global_jobs"]
            .as_u64()
            .is_some_and(|jobs| jobs >= 1)
    );
}
