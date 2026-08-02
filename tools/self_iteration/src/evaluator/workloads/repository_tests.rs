use std::{
    collections::BTreeMap,
    fs,
    path::PathBuf,
    sync::{Arc, Mutex},
};

use super::{
    cold_index_completion_validation, evaluate_repository, incremental_index_completion_validation,
};
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

#[test]
fn cold_index_validation_rejects_cached_noop_measurements() {
    let config = serde_json::json!({"cold_index_min_file_count": 1024});
    let warm_payload = serde_json::json!({
        "summary": {"progress": {"parsed_file_count": 0}},
        "status": {"indexed_file_count": 1024}
    });
    let cold_payload = serde_json::json!({
        "task": {"state": "succeeded"},
        "status": {"indexed_file_count": 1024}
    });

    assert!(
        !cold_index_completion_validation("fixture", &config, &warm_payload)
            .expect("validation")
            .passed()
    );
    assert!(
        cold_index_completion_validation("fixture", &config, &cold_payload)
            .expect("validation")
            .passed()
    );
}

#[test]
fn incremental_index_validation_enforces_delta_work_and_head() {
    let config = serde_json::json!({
        "incremental_max_blob_reads": 2,
        "incremental_max_parsed_files": 2
    });
    let payload = serde_json::json!({
        "summary": {
            "resolved_commit_sha": "head-sha",
            "changed_path_count": 3,
            "progress": {"blob_read_count": 2, "parsed_file_count": 2}
        }
    });

    assert!(
        incremental_index_completion_validation("fixture", &config, 3, "head-sha", &payload)
            .passed()
    );
    assert!(
        !incremental_index_completion_validation("fixture", &config, 3, "other", &payload).passed()
    );
}
