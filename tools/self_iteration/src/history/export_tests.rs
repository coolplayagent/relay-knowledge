use std::{
    fs,
    time::{SystemTime, UNIX_EPOCH},
};

use serde_json::{Value, json};

use super::*;
use crate::history::HistoryPaths;

#[test]
fn export_history_separates_score_acceptance_from_adoption() {
    let workspace = temp_workspace("history-export");
    let paths = HistoryPaths::new(&workspace);
    paths.ensure().expect("history paths");
    let runs = [
        json!({
            "run_id": "run-1",
            "timestamp": "1",
            "profile": "fast",
            "accepted": true,
            "score_accepted": true,
            "committed": true,
            "adoption_status": "committed",
            "score": 0.8,
            "foundational_capability": 1.0,
            "competitive_capability": 0.8,
            "accuracy": 0.9,
            "semantic_vector": 0.0,
            "performance": 0.8,
            "stability": 1.0,
            "commit": "abc1234",
            "patch": {"path": "/tmp/run-1.patch", "sha256": "sha", "bytes": 42},
            "report": "/tmp/run-1.json",
            "reject_reasons": []
        }),
        json!({
            "run_id": "manual-evaluate-2",
            "timestamp": "2",
            "profile": "fast",
            "accepted": false,
            "score_accepted": true,
            "committed": false,
            "adoption_status": "would_accept",
            "score": 0.81,
            "foundational_capability": 1.0,
            "competitive_capability": 0.8,
            "accuracy": 0.9,
            "semantic_vector": 0.0,
            "performance": 0.81,
            "stability": 1.0,
            "commit": null,
            "patch": {"path": "/tmp/manual-evaluate-2.patch", "sha256": "sha2", "bytes": 43},
            "report": "/tmp/manual-evaluate-2.json",
            "reject_reasons": []
        }),
        json!({
            "run_id": "run-3",
            "timestamp": "3",
            "profile": "fast",
            "accepted": false,
            "score": 0.79,
            "foundational_capability": 1.0,
            "competitive_capability": 0.8,
            "accuracy": 0.9,
            "semantic_vector": 0.0,
            "performance": 0.79,
            "stability": 1.0,
            "commit": null,
            "patch": {"path": "/tmp/run-3.patch", "sha256": "sha3", "bytes": 44},
            "report": "/tmp/run-3.json",
            "reject_reasons": ["candidate did not improve score"]
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

    export_history(&paths).expect("export");
    let csv = fs::read_to_string(&paths.score_csv).expect("csv");
    let svg = fs::read_to_string(&paths.score_svg).expect("svg");

    assert!(csv.contains("mode,accepted,score_accepted,committed,adoption_status"));
    assert!(csv.contains("run-1,1,fast,loop,true,true,true,committed"));
    assert!(csv.contains("manual-evaluate-2,2,fast,evaluate,false,true,false,would_accept"));
    assert!(csv.contains("/tmp/manual-evaluate-2.patch"));
    assert!(svg.contains("accepted commit"));
    assert!(svg.contains("would accept evaluation"));
    assert!(svg.contains("#16a34a"));
    assert!(svg.contains("#f59e0b"));
    assert!(svg.contains("#dc2626"));
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
