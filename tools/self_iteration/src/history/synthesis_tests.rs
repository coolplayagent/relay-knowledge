use std::{
    fs,
    time::{SystemTime, UNIX_EPOCH},
};

use serde_json::json;

use super::*;

#[test]
fn synthesis_groups_rejections_and_degradation_hotspots() {
    let workspace = temp_workspace("history-synthesis");
    let paths = HistoryPaths::new(&workspace);
    paths.ensure().expect("history paths");
    let runs = [
        json!({
            "run_id": "accepted-1",
            "timestamp": "1",
            "profile": "fast",
            "accepted": true,
            "score_accepted": true,
            "committed": true,
            "commit": "abc1234",
            "score": 0.8,
            "foundational_capability": 1.0,
            "competitive_capability": 0.8,
            "accuracy": 0.9,
            "semantic_vector": 0.0,
            "performance": 0.8,
            "stability": 1.0,
            "improvements": [{"kind": "score_component", "name": "score", "previous": 0.7, "current": 0.8}],
            "degradations": [],
            "reject_reasons": [],
            "optimization_plan": {"changed_paths": ["src/a.rs"]}
        }),
        rejected("rejected-1", "2", 0.79),
        rejected("rejected-2", "3", 0.78),
    ];
    fs::write(
        &paths.runs_jsonl,
        runs.iter()
            .map(Value::to_string)
            .collect::<Vec<_>>()
            .join("\n"),
    )
    .expect("runs");

    let digest = synthesize_history(&paths, "fast");

    assert!(digest.contains("Latest scored baseline: rejected-2"));
    assert!(digest.contains("Best accepted run: accepted-1"));
    assert!(digest.contains("candidate did not improve score"));
    assert!(digest.contains("x2"));
    assert!(digest.contains("metric:relay_teams_index_ms x2"));
    assert!(digest.contains("Local improvements that did not win"));
}

#[test]
fn synthesis_has_hard_prompt_budget() {
    let workspace = temp_workspace("history-synthesis-cap");
    let paths = HistoryPaths::new(&workspace);
    paths.ensure().expect("history paths");
    let long_path = format!("src/{}.rs", "very_long_directory_name/".repeat(500));
    let runs = [
        json!({
            "run_id": "accepted",
            "timestamp": "0",
            "profile": "fast",
            "accepted": true,
            "score_accepted": true,
            "committed": true,
            "commit": "abc1234",
            "score": 0.8,
            "foundational_capability": 1.0,
            "competitive_capability": 0.8,
            "accuracy": 0.9,
            "semantic_vector": 0.0,
            "performance": 0.8,
            "stability": 1.0,
            "reject_reasons": [],
            "improvements": [{"kind": "score_component", "name": "score", "previous": 0.7, "current": 0.8}],
            "degradations": [],
            "optimization_plan": {"changed_paths": [long_path, "src/a.rs", "src/b.rs"]}
        }),
        rejected("rejected-1", "1", 0.79),
        rejected("rejected-2", "2", 0.78),
    ];
    fs::write(
        &paths.runs_jsonl,
        runs.iter()
            .map(Value::to_string)
            .collect::<Vec<_>>()
            .join("\n"),
    )
    .expect("runs");

    let digest = synthesize_history(&paths, "fast");

    assert!(digest.len() <= SYNTHESIS_CHAR_LIMIT + 140);
    assert!(digest.contains("History synthesis truncated"));
}

fn rejected(run_id: &str, timestamp: &str, score: f64) -> Value {
    json!({
        "run_id": run_id,
        "timestamp": timestamp,
        "profile": "fast",
        "accepted": false,
        "score": score,
        "foundational_capability": 1.0,
        "competitive_capability": 0.8,
        "accuracy": 0.9,
        "semantic_vector": 0.0,
        "performance": 0.7,
        "stability": 1.0,
        "reject_reasons": ["candidate did not improve score or tracked objectives beyond epsilon"],
        "improvements": [{"kind": "metric", "name": "relay_teams_query_p95_ms", "previous": 8000.0, "current": 7000.0}],
        "degradations": [
            {"kind": "metric", "name": "relay_teams_index_ms", "previous": 2000.0, "current": 5000.0},
            {"kind": "score_component", "name": "score", "previous": 0.8, "current": score}
        ],
        "optimization_plan": {"changed_paths": ["src/query.rs"]}
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
