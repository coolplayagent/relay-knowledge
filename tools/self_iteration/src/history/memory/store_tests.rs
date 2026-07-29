use std::{
    fs,
    time::{SystemTime, UNIX_EPOCH},
};

use serde_json::json;

use super::*;
use crate::history;

#[test]
fn memory_index_round_trip_is_atomic_and_sorted_for_prompts() {
    let workspace = temp_workspace("memory-store");
    let paths = history::HistoryPaths::new(&workspace);
    paths.ensure().expect("history paths");
    let items = vec![
        json!({"id": "old", "run_id": "run-1", "created_at": "1"}),
        json!({"id": "manual", "run_id": "manual-evaluate-2", "created_at": "3"}),
        json!({"id": "new", "run_id": "run-2", "created_at": "2"}),
    ];

    write_memory_index(&paths, &items).expect("write index");

    assert_eq!(load_memory_index(&paths), items);
    assert_eq!(
        sorted_memory_items(&paths)
            .iter()
            .map(|item| item["id"].as_str().expect("id"))
            .collect::<Vec<_>>(),
        vec!["new", "old"]
    );
    assert!(!paths.memory_index.with_extension("jsonl.tmp").exists());
    fs::remove_dir_all(workspace).expect("remove workspace");
}

#[test]
fn memory_index_ignores_invalid_and_non_object_lines() {
    let workspace = temp_workspace("memory-store-invalid");
    let paths = history::HistoryPaths::new(&workspace);
    paths.ensure().expect("history paths");
    fs::write(&paths.memory_index, "{\"id\":\"valid\"}\ninvalid\n[]\n").expect("write fixture");

    assert_eq!(load_memory_index(&paths), vec![json!({"id": "valid"})]);
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
