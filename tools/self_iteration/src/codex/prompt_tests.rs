use std::{
    fs,
    time::{SystemTime, UNIX_EPOCH},
};

use serde_json::{Value, json};

use super::*;

#[test]
fn prompt_includes_direct_history_synthesis() {
    let workspace = temp_workspace("codex-prompt");
    let paths = HistoryPaths::new(&workspace);
    paths.ensure().expect("history paths");
    let runs = [
        json!({
            "run_id": "accepted",
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
            "reject_reasons": [],
            "improvements": [{"kind": "score_component", "name": "score", "previous": 0.7, "current": 0.8}],
            "degradations": [],
            "optimization_plan": {"changed_paths": ["src/query.rs"]}
        }),
        json!({
            "run_id": "rejected",
            "timestamp": "2",
            "profile": "fast",
            "accepted": false,
            "score": 0.79,
            "foundational_capability": 1.0,
            "competitive_capability": 0.8,
            "accuracy": 0.9,
            "semantic_vector": 0.0,
            "performance": 0.7,
            "stability": 1.0,
            "reject_reasons": ["candidate did not improve score or tracked objectives beyond epsilon"],
            "improvements": [{"kind": "metric", "name": "relay_teams_query_p95_ms", "previous": 8000.0, "current": 7000.0}],
            "degradations": [{"kind": "score_component", "name": "score", "previous": 0.8, "current": 0.79}],
            "optimization_plan": {"changed_paths": ["src/query.rs"]}
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

    let prompt = build_prompt(&paths, &workspace, "run-test", "fast", None);

    assert!(prompt.contains("Historical synthesis:"));
    assert!(prompt.contains("Latest scored baseline: rejected"));
    assert!(prompt.contains("Best accepted run: accepted"));
    assert!(prompt.contains("Local improvements that did not win"));
    assert!(prompt.contains("broader algorithmic change"));
    assert!(prompt.contains("external dependency target remains unresolved"));
    assert!(prompt.contains("dependency diagnostic"));
    assert!(prompt.contains("source-text evidence"));
    assert!(prompt.contains("If this machine does not"));
    assert!(prompt.contains("grep -RIn"));
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
