use std::{
    fs,
    time::{SystemTime, UNIX_EPOCH},
};

use serde_json::{Value, json};

use super::*;
use crate::history::HistoryPaths;

#[test]
fn profile_best_accepted_ignores_category_focus() {
    let workspace = temp_workspace("history-profile-best");
    let paths = HistoryPaths::new(&workspace);
    paths.ensure().expect("history paths");
    let runs = [
        json!({
            "run_id": "run-competitive",
            "timestamp": "1",
            "profile": "fast",
            "product_binary_profile": "release",
            "category_focus": "competitive",
            "accepted": true,
            "committed": true,
            "commit": "competitive123",
            "score": 0.84
        }),
        json!({
            "run_id": "run-semantic",
            "timestamp": "2",
            "profile": "fast",
            "product_binary_profile": "release",
            "category_focus": "semantic_vector",
            "accepted": true,
            "committed": true,
            "commit": "semantic123",
            "score": 0.95
        }),
        json!({
            "run_id": "run-performance",
            "timestamp": "3",
            "profile": "fast",
            "product_binary_profile": "release",
            "category_focus": "performance",
            "accepted": false,
            "committed": false,
            "score": 0.99
        }),
        json!({
            "run_id": "run-full",
            "timestamp": "4",
            "profile": "full",
            "product_binary_profile": "release",
            "category_focus": "competitive",
            "accepted": true,
            "committed": true,
            "commit": "full123",
            "score": 0.98
        }),
    ];
    fs::write(
        &paths.runs_jsonl,
        runs.iter()
            .map(Value::to_string)
            .collect::<Vec<_>>()
            .join("\n"),
    )
    .expect("runs");

    let best = best_accepted_run_for_profile(&paths, "fast")
        .expect("history")
        .expect("profile best");

    assert_eq!(
        best.get("run_id").and_then(Value::as_str),
        Some("run-semantic")
    );
    fs::remove_dir_all(workspace).expect("remove workspace");
}

#[test]
fn profile_best_acceptance_floor_excludes_legacy_fast_debug_scores() {
    let workspace = temp_workspace("history-profile-product-binary");
    let paths = HistoryPaths::new(&workspace);
    paths.ensure().expect("history paths");
    let runs = [
        json!({
            "run_id": "legacy-fast-debug",
            "timestamp": "1",
            "profile": "fast",
            "accepted": true,
            "committed": true,
            "commit": "debug123",
            "score": 0.99
        }),
        json!({
            "run_id": "current-fast-release",
            "timestamp": "2",
            "profile": "fast",
            "product_binary_profile": "release",
            "accepted": true,
            "committed": true,
            "commit": "release123",
            "score": 0.90
        }),
    ];
    fs::write(
        &paths.runs_jsonl,
        runs.iter()
            .map(Value::to_string)
            .collect::<Vec<_>>()
            .join("\n"),
    )
    .expect("runs");

    let acceptance_floor = best_accepted_run_for_profile(&paths, "fast")
        .expect("history")
        .expect("release profile best");
    let diagnostic = best_accepted_run_for_profile_across_product_binaries(&paths, "fast")
        .expect("history")
        .expect("cross-product diagnostic best");

    assert_eq!(acceptance_floor["run_id"], "current-fast-release");
    assert_eq!(diagnostic["run_id"], "legacy-fast-debug");
    fs::remove_dir_all(workspace).expect("remove workspace");
}

fn temp_workspace(prefix: &str) -> std::path::PathBuf {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time")
        .as_nanos();
    let workspace = std::env::temp_dir().join(format!("{prefix}-{unique}"));
    fs::create_dir_all(workspace.join(".git")).expect("workspace");
    workspace
}
