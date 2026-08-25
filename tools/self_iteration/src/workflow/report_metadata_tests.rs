use std::{
    fs,
    time::{SystemTime, UNIX_EPOCH},
};

use serde_json::{Value, json};

use super::comparison_baseline;
use crate::history::HistoryPaths;

#[test]
fn comparison_metadata_separates_acceptance_floor_from_cross_product_diagnostic() {
    let workspace = temporary_workspace();
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

    let metadata = comparison_baseline(&paths, "fast", None, None).expect("metadata");

    assert_eq!(
        metadata["profile_best_scope"],
        "evaluation_profile_and_product_binary_profile_acceptance_floor"
    );
    assert_eq!(
        metadata["profile_best_accepted_run_id"],
        "current-fast-release"
    );
    assert_eq!(
        metadata["cross_product_profile_best_scope"],
        "evaluation_profile_diagnostic_only"
    );
    assert_eq!(
        metadata["cross_product_profile_best_accepted_run_id"],
        "legacy-fast-debug"
    );
    fs::remove_dir_all(workspace).expect("remove workspace");
}

fn temporary_workspace() -> std::path::PathBuf {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time")
        .as_nanos();
    let workspace = std::env::temp_dir().join(format!("report-metadata-{unique}"));
    fs::create_dir_all(workspace.join(".git")).expect("workspace");
    workspace
}
