use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
};

use super::*;
use crate::evaluator::runtime::contracts::Limiter;

#[test]
fn cleanup_runs_after_success_and_keeps_evaluation_writer_bound() {
    let root = temporary_root("cleanup-success");
    let runtime = test_runtime(false);
    let shared_writer = runtime.writer_lock.clone();
    let isolation = RepositoryIsolation::prepare(
        &runtime,
        &root,
        "large_repo",
        &serde_json::json!({"isolated_index_home": true}),
    )
    .expect("isolation should be prepared");
    let home = PathBuf::from(
        isolation
            .runtime
            .env
            .get("RELAY_KNOWLEDGE_HOME")
            .expect("isolated home should be exported"),
    );
    assert!(Arc::ptr_eq(&shared_writer, &isolation.runtime.writer_lock));
    fs::write(home.join("report-input.json"), "{}").expect("repository evidence");

    assert_eq!(isolation.complete(Ok("collected")), Ok("collected"));
    assert!(!home.exists());
    fs::remove_dir_all(root).expect("temporary root cleanup");
}

#[test]
fn cleanup_runs_after_error_and_preserves_original_context() {
    let root = temporary_root("cleanup-error");
    let isolation = RepositoryIsolation::prepare(
        &test_runtime(false),
        &root,
        "large_repo",
        &serde_json::json!({"isolated_index_home": true}),
    )
    .expect("isolation should be prepared");
    let home = isolation.home.clone().expect("isolated home");

    let error = isolation
        .complete::<()>(Err("repository evaluation failed".to_owned()))
        .expect_err("evaluation error should survive cleanup");

    assert_eq!(error, "repository evaluation failed");
    assert!(!home.exists());
    fs::remove_dir_all(root).expect("temporary root cleanup");
}

#[test]
fn keep_workdirs_preserves_isolated_home() {
    let root = temporary_root("cleanup-keep");
    let isolation = RepositoryIsolation::prepare(
        &test_runtime(true),
        &root,
        "large_repo",
        &serde_json::json!({"isolated_index_home": true}),
    )
    .expect("isolation should be prepared");
    let home = isolation.home.clone().expect("isolated home");

    isolation.complete(Ok(())).expect("kept workdir");

    assert!(home.is_dir());
    fs::remove_dir_all(root).expect("temporary root cleanup");
}

#[test]
fn cleanup_refuses_unsafe_or_symlinked_intermediate_paths() {
    let root = temporary_root("cleanup-path-safety");
    let isolation_root = root.join("isolated-index-homes");
    let outside = root.join("outside");
    fs::create_dir(&outside).expect("outside directory");
    let name_error = isolated_repository_home(&isolation_root, "../outside")
        .expect_err("path traversal must be rejected");
    let outside_error = remove_isolated_repository_home(&root, &isolation_root, &outside)
        .expect_err("outside cleanup must be rejected");
    assert!(name_error.contains("safe path component"));
    assert!(outside_error.contains("unsafe isolated repository home"));

    create_directory_symlink(&outside, &isolation_root);
    fs::write(outside.join("sentinel"), "keep").expect("outside sentinel");
    let linked_home = isolation_root.join("large_repo");
    let linked_error = remove_isolated_repository_home(&root, &isolation_root, &linked_home)
        .expect_err("linked root must be rejected");
    assert!(linked_error.contains("non-symlink directory"));
    assert_eq!(
        fs::read_to_string(outside.join("sentinel")).expect("outside sentinel survives"),
        "keep"
    );
    fs::remove_file(isolation_root).expect("test symlink cleanup");
    fs::remove_dir_all(root).expect("temporary root cleanup");
}

#[test]
fn cleanup_failure_is_appended_without_masking_evaluation_error() {
    let root = temporary_root("cleanup-failure-context");
    let isolation = RepositoryIsolation::prepare(
        &test_runtime(false),
        &root,
        "large_repo",
        &serde_json::json!({"isolated_index_home": true}),
    )
    .expect("isolation should be prepared");
    let home = isolation.home.clone().expect("isolated home");
    fs::remove_dir(&home).expect("empty home removal");
    fs::write(&home, "not a directory").expect("cleanup failure fixture");

    let error = isolation
        .complete::<()>(Err("original evaluation error".to_owned()))
        .expect_err("both evaluation and cleanup should fail");

    assert!(error.starts_with("original evaluation error;"));
    assert!(error.contains("non-directory isolated repository home"));
    fs::remove_file(home).expect("cleanup failure fixture removal");
    fs::remove_dir_all(root).expect("temporary root cleanup");
}

fn test_runtime(keep_workdirs: bool) -> EvalRuntime {
    EvalRuntime {
        binary: PathBuf::from("relay-knowledge"),
        workspace: PathBuf::from("."),
        env: BTreeMap::new(),
        timeout: 1,
        limiter: Limiter::new(1),
        writer_lock: Arc::new(Mutex::new(())),
        query_jobs: 1,
        keep_workdirs,
    }
}

fn temporary_root(label: &str) -> PathBuf {
    static NEXT_ID: AtomicU64 = AtomicU64::new(0);
    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    let root = std::env::temp_dir().join(format!(
        "relay-knowledge-repository-isolation-{label}-{}-{id}",
        std::process::id()
    ));
    fs::create_dir_all(&root).expect("temporary root creation");
    root
}

#[cfg(unix)]
fn create_directory_symlink(target: &Path, link: &Path) {
    std::os::unix::fs::symlink(target, link).expect("directory symlink");
}

#[cfg(windows)]
fn create_directory_symlink(target: &Path, link: &Path) {
    std::os::windows::fs::symlink_dir(target, link).expect("directory symlink");
}
