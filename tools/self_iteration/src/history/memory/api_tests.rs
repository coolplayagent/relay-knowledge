use std::{
    fs,
    time::{SystemTime, UNIX_EPOCH},
};

use serde_json::{Value, json};

use super::*;
use crate::history;

#[test]
fn write_run_memory_records_repeated_rejection_cluster() {
    let workspace = temp_workspace("memory-api-cluster");
    let paths = history::HistoryPaths::new(&workspace);
    paths.ensure().expect("history paths");
    let previous = rejected_record("previous", "1");
    history::append_run(&paths, &previous).expect("previous run");
    let current = rejected_record("current", "2");

    write_run_memory(&paths, &current).expect("memory");
    let index = fs::read_to_string(&paths.memory_index).expect("index");

    assert!(index.contains("repeated_rejection_cluster"));
    assert!(index.contains("current-repeated_rejection_cluster"));
    assert!(progressive_memory_index(&paths, 1).contains("older memory item(s) omitted"));
    fs::remove_dir_all(workspace).expect("remove workspace");
}

#[test]
fn rejection_review_explains_missing_scored_history() {
    let workspace = temp_workspace("memory-api-empty");
    let paths = history::HistoryPaths::new(&workspace);

    let review = rejection_recovery_memory_review(&paths, 3);

    assert_eq!(
        review,
        "No scored self-iteration run yet; no rejection recovery memory review required."
    );
    fs::remove_dir_all(workspace).expect("remove workspace");
}

fn rejected_record(run_id: &str, timestamp: &str) -> Value {
    json!({
        "run_id": run_id,
        "timestamp": timestamp,
        "accepted": false,
        "score": 0.79,
        "foundational_capability": 1.0,
        "competitive_capability": 0.8,
        "semantic_vector": 0.0,
        "stability": 1.0,
        "reject_reasons": ["candidate did not improve score or tracked objectives beyond epsilon"],
        "improvements": [{"kind": "metric", "name": "relay_teams_query_p95_ms", "previous": 8000.0, "current": 7000.0}],
        "degradations": [{"kind": "score_component", "name": "score", "previous": 0.8, "current": 0.79}],
        "optimization_plan": {"changed_paths": ["src/query.rs"]},
        "patch": {"path": "/tmp/current.patch"},
        "report": "/tmp/current.json",
        "gates": [],
    })
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
