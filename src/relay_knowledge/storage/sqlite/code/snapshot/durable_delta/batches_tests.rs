use super::DeltaBatchPlan;
use crate::domain::{
    CodeFileDiagnostic, CodeIndexResourceBudget, CodeIndexSnapshot, CodeParseStatus,
    RepositoryCodeChunkRecord, RepositoryCodeFileRecord, RepositoryCodeRange,
};

#[test]
fn plan_keeps_every_file_owned_fact_in_one_deterministic_batch() {
    let mut snapshot = snapshot(&["a.rs", "b.rs"]);
    snapshot.chunks = vec![chunk("a.rs"), chunk("b.rs")];
    snapshot.diagnostics = vec![diagnostic("b.rs")];
    let budget = CodeIndexResourceBudget::new(1, 1_024, 100).expect("budget");
    let plan = DeltaBatchPlan::new(&snapshot, budget).expect("plan should partition");

    assert_eq!(plan.len(), 2);
    let first = plan.batch(0, 4).expect("first batch");
    let second = plan.batch(1, 5).expect("second batch");
    assert_eq!(first.batch_index, 4);
    assert_eq!(first.files[0].path, "a.rs");
    assert_eq!(first.chunks[0].path, "a.rs");
    assert!(first.diagnostics.is_empty());
    assert_eq!(second.files[0].path, "b.rs");
    assert_eq!(second.chunks[0].path, "b.rs");
    assert_eq!(second.diagnostics[0].path, "b.rs");
}

#[test]
fn plan_rejects_an_indivisible_file_outside_the_frozen_byte_or_row_budget() {
    let mut snapshot = snapshot(&["large.rs", "next.rs"]);
    snapshot.files[0].byte_len = 8_192;
    snapshot.chunks = vec![chunk("large.rs"), chunk("large.rs"), chunk("next.rs")];
    let byte_budget = CodeIndexResourceBudget::new(8, 16, 100).expect("byte budget");
    let byte_error = DeltaBatchPlan::new(&snapshot, byte_budget)
        .err()
        .expect("one oversized file must fail byte admission");
    assert!(byte_error.to_string().contains("large.rs"));
    assert!(byte_error.to_string().contains("frozen writer quantum"));

    snapshot.files[0].byte_len = 8;
    let row_budget = CodeIndexResourceBudget::new(8, 1_024, 2).expect("row budget");
    let row_error = DeltaBatchPlan::new(&snapshot, row_budget)
        .err()
        .expect("one oversized file must fail row admission");
    assert!(row_error.to_string().contains("large.rs"));
    assert!(row_error.to_string().contains("frozen writer quantum"));
}

#[test]
fn plan_rejects_facts_without_a_file_owner() {
    let mut snapshot = snapshot(&["owned.rs"]);
    snapshot.diagnostics.push(diagnostic("orphan.rs"));

    let error = DeltaBatchPlan::new(&snapshot, CodeIndexResourceBudget::default())
        .err()
        .expect("orphan fact must fail closed");
    assert!(error.to_string().contains("orphan.rs"));
    assert!(error.to_string().contains("no file owner"));
}

#[test]
fn plan_rejects_duplicate_file_owners_before_partitioning() {
    let snapshot = snapshot(&["duplicate.rs", "duplicate.rs"]);

    let error = DeltaBatchPlan::new(&snapshot, CodeIndexResourceBudget::default())
        .err()
        .expect("duplicate file ownership must fail closed");
    assert!(error.to_string().contains("duplicate file path"));
    assert!(error.to_string().contains("duplicate.rs"));
}

#[test]
fn batch_rejects_an_ordinal_outside_the_frozen_plan() {
    let snapshot = snapshot(&["only.rs"]);
    let plan = DeltaBatchPlan::new(&snapshot, CodeIndexResourceBudget::default())
        .expect("single file should produce one batch");

    let error = plan
        .batch(plan.len(), 2)
        .expect_err("ordinal at the plan length is out of bounds");
    assert!(error.to_string().contains("ordinal 1"));
    assert!(error.to_string().contains("1-batch plan"));
}

fn snapshot(paths: &[&str]) -> CodeIndexSnapshot {
    CodeIndexSnapshot {
        repository_id: "repo".to_owned(),
        source_scope: "scope".to_owned(),
        base_resolved_commit_sha: Some("base".to_owned()),
        resolved_commit_sha: "worktree:base:0123456789abcdef".to_owned(),
        tree_hash: "worktree:0123456789abcdef".to_owned(),
        path_filters: Vec::new(),
        language_filters: Vec::new(),
        full_replace: false,
        changed_path_count: paths.len(),
        skipped_unchanged_count: 0,
        deleted_paths: Vec::new(),
        tombstones: Vec::new(),
        files: paths.iter().map(|path| file(path)).collect(),
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

fn file(path: &str) -> RepositoryCodeFileRecord {
    RepositoryCodeFileRecord {
        repository_id: "repo".to_owned(),
        source_scope: "scope".to_owned(),
        file_id: format!("file:{path}"),
        path: path.to_owned(),
        language_id: "rust".to_owned(),
        blob_hash: format!("blob:{path}"),
        byte_len: 8,
        line_count: 1,
        parse_status: CodeParseStatus::Parsed,
        is_generated: false,
        degraded_reason: None,
    }
}

fn chunk(path: &str) -> RepositoryCodeChunkRecord {
    RepositoryCodeChunkRecord {
        repository_id: "repo".to_owned(),
        source_scope: "scope".to_owned(),
        chunk_id: format!("chunk:{path}"),
        file_id: format!("file:{path}"),
        path: path.to_owned(),
        language_id: "rust".to_owned(),
        content: path.to_owned(),
        byte_range: range(),
        line_range: range(),
        symbol_snapshot_id: None,
    }
}

fn diagnostic(path: &str) -> CodeFileDiagnostic {
    CodeFileDiagnostic {
        repository_id: "repo".to_owned(),
        source_scope: "scope".to_owned(),
        path: path.to_owned(),
        parse_status: CodeParseStatus::Partial,
        message: "fixture".to_owned(),
    }
}

fn range() -> RepositoryCodeRange {
    RepositoryCodeRange::new("fixture", 0, 1).expect("range")
}
