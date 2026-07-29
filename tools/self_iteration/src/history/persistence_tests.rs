use std::{
    fs,
    time::{SystemTime, UNIX_EPOCH},
};

use serde_json::json;

use super::*;

#[test]
fn report_and_run_writes_create_history_directories() {
    let workspace = temp_workspace("history-persistence");
    let paths = HistoryPaths::new(&workspace);
    let report = json!({"run_id": "run-1", "score": 0.8});
    let record = json!({"run_id": "run-1", "score": 0.8});

    let report_path = write_report(&paths, "run-1", &report).expect("report");
    append_run(&paths, &record).expect("append run");

    assert_eq!(
        serde_json::from_str::<serde_json::Value>(
            &fs::read_to_string(report_path).expect("read report")
        )
        .expect("parse report"),
        report
    );
    assert_eq!(
        fs::read_to_string(&paths.runs_jsonl).expect("read runs"),
        format!("{record}\n")
    );
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
