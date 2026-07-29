use std::{
    collections::BTreeMap,
    fs,
    path::PathBuf,
    sync::{Arc, Mutex},
};

use super::evaluate_repository;
use crate::evaluator::runtime::contracts::{EvalRuntime, Limiter};

#[test]
fn repository_workload_rejects_non_full_scope_before_running_product_commands() {
    let root = std::env::temp_dir().join(format!(
        "relay-knowledge-repository-workload-test-{}",
        std::process::id()
    ));
    fs::create_dir_all(&root).expect("test repository path should be created");
    let runtime = EvalRuntime {
        binary: PathBuf::from("relay-knowledge"),
        workspace: PathBuf::from("."),
        env: BTreeMap::new(),
        timeout: 1,
        limiter: Limiter::new(1),
        writer_lock: Arc::new(Mutex::new(())),
        query_jobs: 1,
    };
    let config = serde_json::json!({
        "path": root,
        "scope": "partial"
    });

    let report = evaluate_repository(
        &runtime,
        std::path::Path::new("."),
        "fixture",
        &config,
        Vec::new(),
        Vec::new(),
    )
    .expect("scope validation should produce a report");

    assert_eq!(report.commands.len(), 1);
    assert_eq!(report.commands[0].exit_code, 1);
    assert!(
        report.commands[0]
            .stderr
            .contains("must use full scope=all")
    );
    fs::remove_dir_all(&root).expect("test repository path should be removed");
}
