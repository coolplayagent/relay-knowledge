use std::{
    fs,
    time::{SystemTime, UNIX_EPOCH},
};

use serde_json::{Value, json};

use super::*;

#[test]
fn rejected_summary_includes_changes_and_score_delta() {
    let record = rejected_record("current", "2");

    let summary = primary_summary("rejected_attempt", &record);

    assert!(summary.contains("Score delta: -0.010000"));
    assert!(summary.contains("Changed paths: src/query.rs"));
    assert!(summary.contains("Top improvements: metric:relay_teams_query_p95_ms"));
    assert!(summary.contains("Top degradations: score_component:score"));
}

#[test]
fn accepted_summary_lists_protected_floors() {
    let record = json!({
        "run_id": "accepted",
        "accepted": true,
        "score": 0.8,
        "foundational_capability": 1.0,
        "competitive_capability": 0.8,
        "semantic_vector": 0.0,
        "stability": 1.0,
        "improvements": [{"kind": "score_component", "name": "score", "previous": 0.7, "current": 0.8}],
        "degradations": [],
        "optimization_plan": {"changed_paths": ["src/query.rs"]},
    });

    let summary = primary_summary("accepted_optimization", &record);

    assert!(summary.contains("Protected floors: foundational=1.0"));
    assert!(summary.contains("Key improvements: score_component:score"));
    assert!(summary.contains("Known degradations: none recorded"));
}

#[test]
fn repeated_rejection_cluster_memory_is_recorded() {
    let workspace = temp_workspace("memory-cluster");
    let paths = history::HistoryPaths::new(&workspace);
    paths.ensure().expect("history paths");
    let previous = rejected_record("previous", "1");
    history::append_run(&paths, &previous).expect("previous run");
    let current = rejected_record("current", "2");

    write_run_memory(&paths, &current).expect("memory");
    let index = fs::read_to_string(&paths.memory_index).expect("index");

    assert!(index.contains("repeated_rejection_cluster"));
    assert!(index.contains("current-repeated_rejection_cluster"));
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
