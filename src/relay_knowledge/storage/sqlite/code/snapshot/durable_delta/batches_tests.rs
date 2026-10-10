use super::{DURABLE_BATCH_CONTROL_ROW_COUNT, DeltaBatchPlan, batch_control_bytes, file_surfaces};
use crate::domain::{
    CodeFileDiagnostic, CodeIndexResourceBudget, CodeIndexSnapshot, CodeParseStatus,
    RepositoryCodeChunkRecord, RepositoryCodeFileRecord, RepositoryCodeRange,
    RepositoryCodeSymbolRecord,
};

#[test]
fn plan_keeps_every_file_owned_fact_in_one_deterministic_batch() {
    let mut snapshot = snapshot(&["a.rs", "b.rs"]);
    snapshot.chunks = vec![chunk("a.rs"), chunk("b.rs")];
    snapshot.diagnostics = vec![diagnostic("b.rs")];
    let budget = CodeIndexResourceBudget::new(1, 32_768, 100).expect("budget");
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
fn plan_rejects_an_owned_serialized_surface_even_when_source_bytes_fit() {
    let mut snapshot = snapshot(&["owned.rs"]);
    snapshot.symbols = vec![symbol("owned.rs")];
    snapshot.symbols[0].doc_comment = Some("x".repeat(2_000));
    let budget = CodeIndexResourceBudget::new(8, 4_096, 100).expect("budget");

    let error = DeltaBatchPlan::new(&snapshot, budget)
        .err()
        .expect("serialized owned records must count toward byte admission");
    assert!(error.to_string().contains("owned.rs"));
    assert!(error.to_string().contains("owned fact surface"));
}

#[test]
fn code_index_task_delta_admits_reference_facts_before_paged_search_finalization() {
    let mut snapshot = snapshot(&["references.rs"]);
    snapshot.references = (0..100)
        .map(|index| reference("references.rs", index))
        .collect();
    let budget = CodeIndexResourceBudget::new(8, 1_000_000, 110).unwrap();
    let plan = DeltaBatchPlan::new(&snapshot, budget)
        .expect("unpublished reference facts fit without future search rows");
    assert_eq!(plan.len(), 1);
    assert_eq!(plan.batch(0, 1).unwrap().references, snapshot.references);

    let too_small = CodeIndexResourceBudget::new(8, 1_000_000, 100).unwrap();
    assert!(DeltaBatchPlan::new(&snapshot, too_small).is_err());
    let byte_limited = CodeIndexResourceBudget::new(8, 1_024, 110).unwrap();
    assert!(DeltaBatchPlan::new(&snapshot, byte_limited).is_err());
}

#[test]
fn code_index_task_delta_admits_large_documents_by_their_persisted_projection() {
    let mut delta = snapshot(&["document.json"]);
    let mut symbol = symbol("document.json");
    symbol.signature = "json document value ".repeat(10_000);
    let mut content = chunk("document.json");
    content.content = symbol.signature.clone();
    delta.symbols.push(symbol);
    delta.chunks.push(content);
    let budget = CodeIndexResourceBudget::new(8, 1_000_000, 100).unwrap();
    let plan =
        DeltaBatchPlan::new(&delta, budget).expect("actual projection fits the original budget");
    assert_eq!(plan.len(), 1);
    let batch = plan.batch(0, 1).unwrap();
    assert_eq!(batch.symbols, delta.symbols);
    assert_eq!(batch.chunks, delta.chunks);
    let too_small = CodeIndexResourceBudget::new(8, 100_000, 100).unwrap();
    assert!(DeltaBatchPlan::new(&delta, too_small).is_err());
}

fn reference(path: &str, index: usize) -> crate::domain::RepositoryCodeReferenceRecord {
    crate::domain::RepositoryCodeReferenceRecord {
        repository_id: "repo".into(),
        source_scope: "scope".into(),
        reference_id: format!("reference:{index}"),
        file_id: format!("file:{path}"),
        path: path.into(),
        name: format!("target_{index}"),
        kind: "usage".into(),
        target_symbol_snapshot_id: None,
        target_hint: None,
        resolution_state: "unresolved".into(),
        confidence_basis_points: 0,
        confidence_tier: "unknown".into(),
        byte_range: range(),
        line_range: range(),
    }
}

#[test]
fn code_index_task_delta_keeps_legacy_membership_and_validates_deferred_call_owners() {
    let mut delta = snapshot(&["a.rs", "b.rs"]);
    delta.references = (0..20)
        .flat_map(|i| [reference("a.rs", i), reference("b.rs", i)])
        .collect();
    let budget = CodeIndexResourceBudget::new(8, 1_000_000, 100).unwrap();
    let plan = DeltaBatchPlan::new(&delta, budget).unwrap();
    assert_eq!(plan.len(), 2, "do not regroup a previously accepted plan");
    assert_eq!(plan.batch(0, 2).unwrap().files[0].path, "a.rs");
    assert_eq!(plan.batch(1, 3).unwrap().files[0].path, "b.rs");

    delta.calls = (0..200)
        .map(|i| crate::domain::CodeCallRecord {
            byte_range: Some(range()),
            repository_id: "repo".into(),
            source_scope: "scope".into(),
            call_id: format!("call:{i}"),
            file_id: "file:a.rs".into(),
            path: "a.rs".into(),
            caller_symbol_snapshot_id: None,
            caller_name: None,
            callee_symbol_snapshot_id: None,
            callee_name: "callee".into(),
            target_hint: None,
            resolution_state: "unresolved".into(),
            confidence_basis_points: 0,
            confidence_tier: "unknown".into(),
            line_range: range(),
        })
        .collect();
    let plan = DeltaBatchPlan::new(&delta, budget).unwrap();
    assert_eq!(plan.len(), 1, "call rows are built by bounded finalization");
    assert_eq!(plan.batch(0, 1).unwrap().references.len(), 40);
    delta.calls[0].path = "orphan.rs".into();
    let error = DeltaBatchPlan::new(&delta, budget).err().unwrap();
    assert!(error.to_string().contains("no file owner"));
}

#[test]
fn plan_reserves_every_mandatory_control_row_and_its_bytes() {
    let snapshot = snapshot(&["owned.rs"]);
    let surface = file_surfaces(&snapshot, true)
        .expect("surface should measure")
        .remove("owned.rs")
        .expect("file should own one surface");
    let control_bytes = batch_control_bytes(&snapshot).expect("controls should measure");
    let row_budget = CodeIndexResourceBudget::new(
        8,
        control_bytes + surface.bytes,
        DURABLE_BATCH_CONTROL_ROW_COUNT,
    )
    .expect("row budget");
    DeltaBatchPlan::new(&snapshot, row_budget)
        .err()
        .expect("the file row cannot consume a reserved control row");

    let byte_budget = CodeIndexResourceBudget::new(
        8,
        control_bytes + surface.bytes - 1,
        DURABLE_BATCH_CONTROL_ROW_COUNT + surface.rows,
    )
    .expect("byte budget");
    DeltaBatchPlan::new(&snapshot, byte_budget)
        .err()
        .expect("the file surface cannot consume reserved control bytes");

    let exact_budget = CodeIndexResourceBudget::new(
        8,
        control_bytes + surface.bytes,
        DURABLE_BATCH_CONTROL_ROW_COUNT + surface.rows,
    )
    .expect("exact budget");
    assert_eq!(
        DeltaBatchPlan::new(&snapshot, exact_budget)
            .expect("the exact complete batch surface should fit")
            .len(),
        1
    );
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

fn symbol(path: &str) -> RepositoryCodeSymbolRecord {
    RepositoryCodeSymbolRecord {
        type_owner: None,
        repository_id: "repo".to_owned(),
        source_scope: "scope".to_owned(),
        symbol_snapshot_id: format!("symbol:{path}"),
        canonical_symbol_id: format!("canonical:{path}"),
        file_id: format!("file:{path}"),
        path: path.to_owned(),
        language_id: "rust".to_owned(),
        name: "owned".to_owned(),
        qualified_name: "owned".to_owned(),
        kind: "function".to_owned(),
        signature: "fn owned()".to_owned(),
        doc_comment: None,
        byte_range: range(),
        line_range: range(),
        symbol_role: None,
    }
}

fn diagnostic(path: &str) -> CodeFileDiagnostic {
    CodeFileDiagnostic {
        io: None,
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

#[test]
fn source_io_diagnostic_only_delta_paths_are_bounded_without_fabricated_files() {
    use crate::domain::{
        CodePathIoAction, CodePathIoDiagnostic, CodePathIoErrorKind, CodePathIoOperation,
        CodePathKind,
    };
    let mut delta = snapshot(&[]);
    for path in ["a.rs", "blocked"] {
        let mut d = diagnostic(path);
        d.parse_status = CodeParseStatus::Failed;
        d.io = Some(CodePathIoDiagnostic {
            action: CodePathIoAction::Skipped,
            path_kind: if path == "blocked" {
                CodePathKind::Directory
            } else {
                CodePathKind::File
            },
            operation: CodePathIoOperation::Read,
            error_kind: CodePathIoErrorKind::Unsupported,
            raw_os_error: Some(1),
        });
        delta.diagnostics.push(d);
    }
    let plan =
        DeltaBatchPlan::new(&delta, CodeIndexResourceBudget::new(1, 32768, 100).unwrap()).unwrap();
    assert_eq!(plan.len(), 2);
    for index in 0..2 {
        let batch = plan.batch(index, index + 1).unwrap();
        assert!(batch.files.is_empty());
        assert_eq!(batch.processed_paths().len(), 1);
        assert_eq!(batch.diagnostics.len(), 1);
    }
}
