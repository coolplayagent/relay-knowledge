use std::{
    collections::BTreeMap,
    fs,
    path::PathBuf,
    sync::{Arc, Mutex},
};

use super::{elastic_timeout_seconds, evaluate_repository, scoped_register_command};
use crate::evaluator::runtime::contracts::{EvalRuntime, Limiter};

#[test]
fn elastic_budget_extends_process_timeout_beyond_global_default() {
    let config = serde_json::json!({
        "index_budget_mode": "elastic",
        "baseline_file_count": 100,
        "expected_file_count": 80_000,
        "baseline_index_budget_ms": 10_000,
        "baseline_files_per_second": 80,
        "max_index_budget_ms": 2_000_000
    });
    assert_eq!(
        elastic_timeout_seconds(900, &config, "index_budget_ms"),
        1_030
    );
}

#[test]
fn scoped_registration_uses_only_explicit_registration_paths() {
    let config = serde_json::json!({
        "registration_path_filters": ["packages/app/src"],
        "path_filters": ["query/only"],
        "language_filters": ["vue"]
    });

    assert_eq!(
        scoped_register_command(
            std::path::Path::new("relay-knowledge"),
            std::path::Path::new("/work/project"),
            Some("frontend"),
            &config,
        ),
        vec![
            "relay-knowledge",
            "repo",
            "register",
            "/work/project",
            "--alias",
            "frontend",
            "--path",
            "packages/app/src",
            "--format",
            "json",
        ]
    );
}

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
        keep_workdirs: false,
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
