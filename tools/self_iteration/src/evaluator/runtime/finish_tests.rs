use std::{fs, path::PathBuf, time::Instant};

use super::{FinishInput, finish};
use crate::{
    config::{Config, JobPlan},
    evaluator::{runtime::workdir::EvaluationHome, workloads::WorkloadSelection},
    history::HistoryPaths,
};

#[test]
fn finish_serializes_empty_observation_and_parallelism_contract() {
    let config = Config::parse(vec!["evaluate".to_owned()]).expect("config should parse");
    let job_plan = JobPlan::resolve(&config);
    let selection = WorkloadSelection::new(&config);
    let workspace = temporary_workspace();
    let paths = HistoryPaths::new(&workspace);
    paths.ensure().expect("history paths");
    let evaluation_home =
        EvaluationHome::prepare(&paths, "run-finish", false).expect("evaluation home");
    let run_home = evaluation_home.path().to_path_buf();

    let run = finish(FinishInput {
        config: &config,
        generated_diff: false,
        gates: Vec::new(),
        cases: Vec::new(),
        metrics: Vec::new(),
        commands: Vec::new(),
        repo_reports: Vec::new(),
        run_home: run_home.clone(),
        job_plan,
        selection,
        started: Instant::now(),
    })
    .expect("finish should serialize");

    assert!(run.observation.gates.is_empty());
    assert_eq!(run.report["cached_home"], false);
    assert_eq!(run.report["run_scoped_home"], true);
    assert_eq!(run.report["product_binary_profile"], "release");
    assert!(
        run.report["product_binary_path"]
            .as_str()
            .is_some_and(|path| path.ends_with("target/release/relay-knowledge"))
    );
    evaluation_home
        .complete_result(Ok(()))
        .expect("evaluation cleanup");
    assert!(!run_home.exists());
    assert!(
        run.report["parallelism"]["global_jobs"]
            .as_u64()
            .is_some_and(|jobs| jobs >= 1)
    );
    fs::remove_dir_all(workspace).expect("workspace cleanup");
}

fn temporary_workspace() -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "relay-knowledge-finish-test-{}",
        std::process::id()
    ));
    if root.exists() {
        fs::remove_dir_all(&root).expect("stale work root cleanup");
    }
    fs::create_dir_all(root.join(".git")).expect("workspace git root");
    root
}
