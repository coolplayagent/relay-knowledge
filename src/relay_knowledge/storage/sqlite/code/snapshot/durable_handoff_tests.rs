use super::{encoded_summary, require_terminal_control_budget};
use crate::{
    domain::{CodeIndexResourceBudget, CodeIndexSnapshot, CodePathTombstone},
    storage::sqlite::code::snapshot::durable_clone::CloneCompletion,
};

#[test]
fn terminal_handoff_counts_tombstones_and_owner_cleanup_against_one_writer_quantum() {
    let mut snapshot = snapshot();
    let completion = completion();
    let budget = CodeIndexResourceBudget::new(8, 4_096, 9).expect("budget should validate");
    require_terminal_control_budget(&snapshot, &completion, 1, budget)
        .expect("owner cleanup and fixed controls fit the exact row boundary");

    snapshot.tombstones.push(CodePathTombstone {
        repository_id: snapshot.repository_id.clone(),
        source_scope: snapshot.source_scope.clone(),
        old_path: "old.rs".to_owned(),
        new_path: Some("new.rs".to_owned()),
        base_ref: "base".to_owned(),
        head_ref: "head".to_owned(),
    });
    let error = require_terminal_control_budget(&snapshot, &completion, 1, budget)
        .expect_err("one extra tombstone must cross the frozen row boundary");
    assert!(error.to_string().contains("terminal control surface"));
}

#[test]
fn summary_rejects_a_delta_without_an_immutable_base_identity() {
    let mut snapshot = snapshot();
    snapshot.base_resolved_commit_sha = None;

    let error = encoded_summary(&snapshot, "task", 1)
        .expect_err("a durable delta receipt must remain bound to its base commit");
    assert!(error.to_string().contains("no base commit"));
    assert!(error.to_string().contains("scope"));
}

fn snapshot() -> CodeIndexSnapshot {
    CodeIndexSnapshot {
        repository_id: "repo".to_owned(),
        source_scope: "scope".to_owned(),
        base_resolved_commit_sha: Some("base".to_owned()),
        resolved_commit_sha: "head".to_owned(),
        tree_hash: "tree".to_owned(),
        path_filters: Vec::new(),
        language_filters: Vec::new(),
        full_replace: false,
        changed_path_count: 0,
        skipped_unchanged_count: 0,
        deleted_paths: Vec::new(),
        tombstones: Vec::new(),
        files: Vec::new(),
        symbols: Vec::new(),
        references: Vec::new(),
        imports: Vec::new(),
        calls: Vec::new(),
        dependencies: Vec::new(),
        feature_flags: Vec::new(),
        framework_nodes: Vec::new(),
        framework_edges: Vec::new(),
        routes: Vec::new(),
        chunks: Vec::new(),
        workspaces: Vec::new(),
        diagnostics: Vec::new(),
    }
}

fn completion() -> CloneCompletion {
    CloneCompletion {
        task_id: "task".to_owned(),
        checkpoint_state: "clone_complete".to_owned(),
        cloned_file_count: 0,
        cloned_symbol_count: 0,
        cloned_reference_count: 0,
        cloned_chunk_count: 0,
        base_source_fact_row_upper_bound: 1,
        completed_page_ordinal: 1,
        terminal_cleanup_rows: 1,
        terminal_cleanup_bytes: 64,
    }
}
