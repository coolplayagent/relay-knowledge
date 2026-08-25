use std::{
    collections::BTreeMap,
    fs,
    path::PathBuf,
    sync::{Arc, Mutex},
};

use super::evaluate_file_fixtures;
use crate::evaluator::runtime::contracts::{EvalRuntime, Limiter};

#[test]
fn empty_fixture_configuration_creates_bounded_workspace_without_commands() {
    let run_home = std::env::temp_dir().join(format!(
        "relay-knowledge-file-workload-test-{}",
        std::process::id()
    ));
    let runtime = EvalRuntime {
        binary: PathBuf::from("relay-knowledge"),
        workspace: PathBuf::from("."),
        env: BTreeMap::new(),
        timeout: 1,
        limiter: Limiter::new(1),
        writer_lock: Arc::new(Mutex::new(())),
        query_jobs: 1,
        keep_workdirs: false,
    };

    let report = evaluate_file_fixtures(&runtime, &run_home, &serde_json::json!({}))
        .expect("empty fixture configuration should succeed");

    assert!(report.commands.is_empty());
    assert!(report.cases.is_empty());
    assert!(report.metrics.is_empty());
    assert!(run_home.join("file-fixtures").is_dir());
    fs::remove_dir_all(&run_home).expect("test fixture workspace should be removed");
}
