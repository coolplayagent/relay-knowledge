use std::{
    fs,
    path::PathBuf,
    sync::{Arc, Mutex},
};

use super::*;
use crate::command::inherited_env;
use crate::evaluator::runtime::contracts::{EvalRuntime, Limiter};

#[test]
fn git_file_count_observation_uses_a_bounded_runtime_timeout() {
    assert_eq!(git_file_count_timeout_seconds(0), 1);
    assert_eq!(git_file_count_timeout_seconds(30), 30);
    assert_eq!(git_file_count_timeout_seconds(300), 120);
}

#[test]
fn git_file_count_observation_treats_plain_directories_as_filesystem_sources() {
    let root = std::env::temp_dir().join(format!(
        "relay-knowledge-filesystem-observation-test-{}",
        std::process::id()
    ));
    if root.exists() {
        fs::remove_dir_all(&root).expect("stale filesystem fixture cleanup");
    }
    fs::create_dir_all(&root).expect("filesystem fixture root");
    fs::write(root.join("notes.txt"), "filesystem source\n").expect("filesystem fixture file");
    let runtime = git_observation_runtime();
    let config = serde_json::json!({"index_budget_ms": 1000});

    let (effective, observation) =
        observed_git_file_count(&runtime, "filesystem", &config, &root, "HEAD");

    assert!(observation.passed());
    assert_eq!(effective, config);
    let evidence: serde_json::Value =
        serde_json::from_str(&observation.stdout).expect("filesystem evidence JSON");
    assert_eq!(evidence["source_kind"], "filesystem");
    assert!(evidence["observed_git_file_count"].is_null());
    fs::remove_dir_all(root).expect("filesystem fixture cleanup");
}

#[test]
fn git_file_count_observation_does_not_hide_broken_git_metadata() {
    let root = std::env::temp_dir().join(format!(
        "relay-knowledge-broken-git-observation-test-{}",
        std::process::id()
    ));
    if root.exists() {
        fs::remove_dir_all(&root).expect("stale broken-Git fixture cleanup");
    }
    fs::create_dir_all(root.join(".git")).expect("broken Git metadata fixture");

    let (_, observation) = observed_git_file_count(
        &git_observation_runtime(),
        "broken_git",
        &serde_json::json!({}),
        &root,
        "HEAD",
    );

    assert!(!observation.passed());
    assert!(observation.stderr.contains("not a git repository"));
    fs::remove_dir_all(root).expect("broken-Git fixture cleanup");
}

#[test]
fn cold_index_validation_rejects_cached_noop_measurements() {
    let config = serde_json::json!({"cold_index_min_file_count": 1024});
    let expectation = index_expectation(1024);
    let warm_payload = serde_json::json!({
        "summary": {"progress": {"parsed_file_count": 0}},
        "status": {"indexed_file_count": 1024, "state": "fresh", "stale": false}
    });
    let cold_payload = completed_index_payload(1024);

    assert!(
        !cold_index_completion_validation("fixture", &config, &expectation, &warm_payload).passed()
    );
    assert!(
        cold_index_completion_validation("fixture", &config, &expectation, &cold_payload).passed()
    );
}

#[test]
fn shared_leveldb_or_fixture_index_still_requires_strict_cold_terminal_evidence() {
    let config = serde_json::json!({});
    let expectation = index_expectation(4);
    let complete = completed_index_payload(4);
    let mut no_task = complete.clone();
    no_task
        .as_object_mut()
        .expect("index payload object")
        .remove("task");

    assert!(
        cold_index_completion_validation("shared_fixture", &config, &expectation, &complete)
            .passed()
    );
    assert!(
        !cold_index_completion_validation("shared_fixture", &config, &expectation, &no_task)
            .passed(),
        "a shared repository in a fresh run must not accept a no-task fast path"
    );
}

#[test]
fn cold_index_validation_requires_every_durable_terminal_state() {
    let config = serde_json::json!({"cold_index_min_file_count": 1024});
    let expectation = index_expectation(1024);
    let complete = completed_index_payload(1024);

    let mut retrying = complete.clone();
    retrying["task"]["state"] = serde_json::json!("retrying");
    let mut finalizing = complete.clone();
    finalizing["checkpoint"]["state"] = serde_json::json!("finalizing");
    let mut stale = complete.clone();
    stale["status"]["stale"] = serde_json::json!(true);

    assert!(
        !cold_index_completion_validation("fixture", &config, &expectation, &retrying).passed(),
        "parsed files must not turn a retrying task into success"
    );
    assert!(
        !cold_index_completion_validation("fixture", &config, &expectation, &finalizing).passed(),
        "a finalizing checkpoint is not completed"
    );
    assert!(
        !cold_index_completion_validation("fixture", &config, &expectation, &stale).passed(),
        "stale repository status is not terminal success"
    );
    assert!(cold_index_completion_validation("fixture", &config, &expectation, &complete).passed());
}

#[test]
fn cold_index_validation_rejects_completed_checkpoint_with_partial_counts() {
    let config = serde_json::json!({"cold_index_min_file_count": 1});
    let expectation = index_expectation(93601);
    let mut partial = completed_index_payload(93601);
    partial["checkpoint"]["committed_file_count"] = serde_json::json!(57451);
    partial["status"]["indexed_file_count"] = serde_json::json!(57451);

    let validation = cold_index_completion_validation("fixture", &config, &expectation, &partial);

    assert!(!validation.passed());
    assert!(validation.stderr.contains("93601"));
    assert!(validation.stderr.contains("57451"));
}

#[test]
fn isolated_repository_implicitly_requires_nonempty_terminal_cold_index() {
    let config = serde_json::json!({"isolated_index_home": true});
    let expectation = index_expectation(0);
    let empty = serde_json::json!({
        "task": {"state": "succeeded"},
        "checkpoint": {
            "state": "completed",
            "total_path_count": 0,
            "committed_file_count": 0
        },
        "status": {"indexed_file_count": 0, "state": "fresh", "stale": false}
    });

    assert!(!cold_index_completion_validation("fixture", &config, &expectation, &empty).passed());
}

#[test]
fn incremental_index_validation_enforces_delta_work_and_head() {
    let config = serde_json::json!({
        "incremental_max_blob_reads": 2,
        "incremental_max_parsed_files": 2
    });
    let expectation = index_expectation(1024);
    let mut payload = completed_index_payload(1024);
    payload
        .as_object_mut()
        .expect("payload object")
        .remove("checkpoint");
    payload["summary"] = serde_json::json!({
        "repository_id": "repository-id",
        "source_scope": "scope-id",
        "base_resolved_commit_sha": "base-sha",
        "resolved_commit_sha": "commit-sha",
        "tree_hash": "tree-hash",
        "indexed_file_count": 1024,
        "changed_path_count": 3,
        "progress": {"blob_read_count": 2, "parsed_file_count": 2}
    });
    payload["task"]["mode"] = serde_json::json!({
        "incremental": {"base_ref": "base-sha", "head_ref": "HEAD"}
    });
    payload["task"]["task_id"] = serde_json::json!("task-id");

    assert!(
        incremental_index_completion_validation(
            "fixture",
            &config,
            3,
            "base-sha",
            &expectation,
            &payload,
        )
        .passed()
    );
    let mut matching_checkpoint = payload.clone();
    matching_checkpoint["checkpoint"] = serde_json::json!({
        "repository_id": "repository-id",
        "source_scope": "scope-id",
        "state": "completed",
        "total_path_count": 1024,
        "committed_file_count": 1024,
        "incremental_summary": {
            "task_id": "task-id",
            "base_resolved_commit_sha": "base-sha",
            "changed_path_count": 3,
            "blob_read_count": 2,
            "parsed_file_count": 2,
        },
    });
    assert!(
        incremental_index_completion_validation(
            "fixture",
            &config,
            3,
            "base-sha",
            &expectation,
            &matching_checkpoint,
        )
        .passed()
    );
    let mut delta_only_checkpoint = matching_checkpoint.clone();
    delta_only_checkpoint["checkpoint"]["total_path_count"] = serde_json::json!(2);
    delta_only_checkpoint["checkpoint"]["committed_file_count"] = serde_json::json!(2);
    assert!(
        !incremental_index_completion_validation(
            "fixture",
            &config,
            3,
            "base-sha",
            &expectation,
            &delta_only_checkpoint,
        )
        .passed(),
        "a durable clone checkpoint must prove the complete target scope"
    );
    let mut wrong_receipt = matching_checkpoint.clone();
    wrong_receipt["checkpoint"]["incremental_summary"]["task_id"] =
        serde_json::json!("previous-task");
    assert!(
        !incremental_index_completion_validation(
            "fixture",
            &config,
            3,
            "base-sha",
            &expectation,
            &wrong_receipt,
        )
        .passed(),
        "incremental metrics must belong to the task that published the target"
    );
    let mut stale_checkpoint = matching_checkpoint;
    stale_checkpoint["checkpoint"]["source_scope"] = serde_json::json!("old-scope");
    assert!(
        !incremental_index_completion_validation(
            "fixture",
            &config,
            3,
            "base-sha",
            &expectation,
            &stale_checkpoint,
        )
        .passed(),
        "an optional checkpoint for another scope must not be accepted"
    );
    let mut partial_checkpoint = stale_checkpoint;
    partial_checkpoint["checkpoint"]["source_scope"] = serde_json::json!("scope-id");
    partial_checkpoint["checkpoint"]["committed_file_count"] = serde_json::json!(1023);
    assert!(
        !incremental_index_completion_validation(
            "fixture",
            &config,
            3,
            "base-sha",
            &expectation,
            &partial_checkpoint,
        )
        .passed(),
        "an optional incremental checkpoint must be terminal and self-consistent"
    );
    let mut wrong_identity = payload.clone();
    wrong_identity["status"]["last_indexed_commit"] = serde_json::json!("other");
    assert!(
        !incremental_index_completion_validation(
            "fixture",
            &config,
            3,
            "base-sha",
            &expectation,
            &wrong_identity,
        )
        .passed()
    );
}

#[test]
fn scope_preview_expectation_rejects_declared_count_conflict() {
    let payload = scope_preview_payload(1024);
    let conflict = IndexExpectation::from_preview(
        "fixture",
        &serde_json::json!({"expected_file_count": 1023, "observed_git_file_count": 1025}),
        "HEAD",
        &payload,
    )
    .expect_err("declared count mismatch must be a hard validation failure");
    let expectation = IndexExpectation::from_preview(
        "fixture",
        &serde_json::json!({"expected_file_count": 1024, "observed_git_file_count": 1025}),
        "HEAD",
        &payload,
    )
    .expect("selected count may differ from raw Git count when scope excludes paths");

    assert!(!conflict.passed());
    assert!(conflict.stderr.contains("declared_matches_selected"));
    assert_eq!(expectation.selected_file_count, 1024);
    assert_eq!(expectation.observed_git_file_count, Some(1025));
    assert_eq!(expectation.declared_expected_file_count, Some(1024));
}

#[test]
fn scope_preview_allows_expanded_gitlink_files_above_parent_tree_count() {
    let expectation = IndexExpectation::from_preview(
        "fixture",
        &serde_json::json!({"observed_git_file_count": 1023}),
        "HEAD",
        &scope_preview_payload(1024),
    )
    .expect("product scope preview may expand authorized Git submodule entries");

    assert_eq!(expectation.observed_git_file_count, Some(1023));
    assert_eq!(expectation.selected_file_count, 1024);
}

#[test]
fn cold_index_identity_validation_rejects_each_cross_surface_mismatch() {
    let config = serde_json::json!({"cold_index_min_file_count": 1});
    let expectation = index_expectation(4);
    let complete = completed_index_payload(4);
    let mismatches = [
        ("/scope/scope_id", serde_json::json!("wrong-scope")),
        (
            "/scope/repository_id",
            serde_json::json!("wrong-repository"),
        ),
        ("/scope/alias", serde_json::json!("wrong-alias")),
        ("/scope/requested_ref", serde_json::json!("wrong-ref")),
        (
            "/scope/resolved_commit_sha",
            serde_json::json!("wrong-commit"),
        ),
        ("/scope/tree_hash", serde_json::json!("wrong-tree")),
        ("/scope/indexed_file_count", serde_json::json!(3)),
        ("/scope/path_filters", serde_json::json!(["wrong"])),
        ("/scope/language_filters", serde_json::json!(["wrong"])),
        ("/task/repository_id", serde_json::json!("wrong-repository")),
        ("/task/alias", serde_json::json!("wrong-alias")),
        ("/task/ref_selector", serde_json::json!("wrong-ref")),
        (
            "/task/resolved_commit_sha",
            serde_json::json!("wrong-commit"),
        ),
        ("/task/tree_hash", serde_json::json!("wrong-tree")),
        ("/task/source_scope", serde_json::json!("wrong-scope")),
        ("/task/path_filters", serde_json::json!(["wrong"])),
        ("/task/language_filters", serde_json::json!(["wrong"])),
        (
            "/summary/repository_id",
            serde_json::json!("wrong-repository"),
        ),
        ("/summary/source_scope", serde_json::json!("wrong-scope")),
        (
            "/summary/resolved_commit_sha",
            serde_json::json!("wrong-commit"),
        ),
        ("/summary/tree_hash", serde_json::json!("wrong-tree")),
        ("/summary/indexed_file_count", serde_json::json!(3)),
        (
            "/checkpoint/repository_id",
            serde_json::json!("wrong-repository"),
        ),
        ("/checkpoint/source_scope", serde_json::json!("wrong-scope")),
        (
            "/status/repository_id",
            serde_json::json!("wrong-repository"),
        ),
        ("/status/alias", serde_json::json!("wrong-alias")),
        (
            "/status/last_indexed_scope_id",
            serde_json::json!("wrong-scope"),
        ),
        (
            "/status/last_indexed_commit",
            serde_json::json!("wrong-commit"),
        ),
        ("/status/tree_hash", serde_json::json!("wrong-tree")),
        ("/status/path_filters", serde_json::json!(["wrong"])),
        ("/status/language_filters", serde_json::json!(["wrong"])),
        ("/status/indexed_file_count", serde_json::json!(3)),
    ];

    for (pointer, replacement) in mismatches {
        let mut payload = complete.clone();
        *payload.pointer_mut(pointer).expect("identity field") = replacement;
        assert!(
            !cold_index_completion_validation("fixture", &config, &expectation, &payload).passed(),
            "identity mismatch at {pointer} must fail"
        );
    }
}

fn git_observation_runtime() -> EvalRuntime {
    EvalRuntime {
        binary: PathBuf::from("relay-knowledge"),
        workspace: PathBuf::from("."),
        env: inherited_env(),
        timeout: 5,
        limiter: Limiter::new(1),
        writer_lock: Arc::new(Mutex::new(())),
        query_jobs: 1,
        keep_workdirs: false,
    }
}

fn index_expectation(selected_file_count: u64) -> IndexExpectation {
    IndexExpectation {
        scope_id: "scope-id".to_owned(),
        repository_id: "repository-id".to_owned(),
        alias: "fixture-alias".to_owned(),
        requested_ref: "HEAD".to_owned(),
        resolved_commit_sha: "commit-sha".to_owned(),
        tree_hash: "tree-hash".to_owned(),
        path_filters: Vec::new(),
        language_filters: Vec::new(),
        selected_file_count,
        observed_git_file_count: Some(selected_file_count),
        declared_expected_file_count: Some(selected_file_count),
    }
}

fn scope_preview_payload(selected_file_count: u64) -> serde_json::Value {
    serde_json::json!({
        "scope": {
            "scope_id": "scope-id",
            "repository_id": "repository-id",
            "alias": "fixture-alias",
            "requested_ref": "HEAD",
            "resolved_commit_sha": "commit-sha",
            "tree_hash": "tree-hash",
            "indexed_file_count": selected_file_count,
            "path_filters": [],
            "language_filters": [],
        },
        "preview": {
            "repository_id": "repository-id",
            "alias": "fixture-alias",
            "requested_ref": "HEAD",
            "resolved_commit_sha": "commit-sha",
            "tree_hash": "tree-hash",
            "selected_file_count": selected_file_count,
        }
    })
}

fn completed_index_payload(selected_file_count: u64) -> serde_json::Value {
    serde_json::json!({
        "scope": {
            "scope_id": "scope-id",
            "repository_id": "repository-id",
            "alias": "fixture-alias",
            "requested_ref": "HEAD",
            "resolved_commit_sha": "commit-sha",
            "tree_hash": "tree-hash",
            "indexed_file_count": selected_file_count,
            "path_filters": [],
            "language_filters": [],
        },
        "summary": {
            "repository_id": "repository-id",
            "source_scope": "scope-id",
            "resolved_commit_sha": "commit-sha",
            "tree_hash": "tree-hash",
            "indexed_file_count": selected_file_count,
            "progress": {"parsed_file_count": selected_file_count},
        },
        "task": {
            "repository_id": "repository-id",
            "alias": "fixture-alias",
            "ref_selector": "HEAD",
            "resolved_commit_sha": "commit-sha",
            "tree_hash": "tree-hash",
            "source_scope": "scope-id",
            "path_filters": [],
            "language_filters": [],
            "mode": "full",
            "state": "succeeded",
        },
        "checkpoint": {
            "repository_id": "repository-id",
            "source_scope": "scope-id",
            "state": "completed",
            "total_path_count": selected_file_count,
            "committed_file_count": selected_file_count,
        },
        "status": {
            "repository_id": "repository-id",
            "alias": "fixture-alias",
            "last_indexed_scope_id": "scope-id",
            "last_indexed_commit": "commit-sha",
            "tree_hash": "tree-hash",
            "path_filters": [],
            "language_filters": [],
            "indexed_file_count": selected_file_count,
            "state": "fresh",
            "stale": false,
        }
    })
}
