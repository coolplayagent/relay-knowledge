//! Direct contracts for validated code graph records.

use super::*;

#[test]
fn rejects_invalid_ranges_and_paths() {
    let range_error = CodeRange::new(10, 10, 1, 1).expect_err("empty range should fail");
    let path_error = CodeFileRecord::new(CodeFileFields {
        source_scope: scope(),
        path: "../lib.rs".to_owned(),
        content_hash: "hash".to_owned(),
        language_id: "rust".to_owned(),
        parse_status: CodeParseStatus::Parsed,
        diagnostic: None,
        symbols: Vec::new(),
        references: Vec::new(),
        chunks: Vec::new(),
    })
    .expect_err("parent paths should fail");

    assert_eq!(range_error.field, "code_range");
    assert_eq!(path_error.field, "code_path");
}

#[test]
fn validates_status_specific_code_facts() {
    let failed_with_chunk = CodeFileRecord::new(CodeFileFields {
        source_scope: scope(),
        path: "src/lib.rs".to_owned(),
        content_hash: "hash".to_owned(),
        language_id: "rust".to_owned(),
        parse_status: CodeParseStatus::Failed,
        diagnostic: Some("parser failed".to_owned()),
        symbols: Vec::new(),
        references: Vec::new(),
        chunks: vec![chunk("chunk-1", scope(), "src/lib.rs")],
    })
    .expect_err("failed file facts should fail");
    let partial_without_diagnostic = CodeFileRecord::new(CodeFileFields {
        source_scope: scope(),
        path: "src/lib.rs".to_owned(),
        content_hash: "hash".to_owned(),
        language_id: "rust".to_owned(),
        parse_status: CodeParseStatus::Partial,
        diagnostic: None,
        symbols: Vec::new(),
        references: Vec::new(),
        chunks: Vec::new(),
    })
    .expect_err("partial diagnostics should be required");

    assert_eq!(failed_with_chunk.field, "parse_status");
    assert_eq!(partial_without_diagnostic.field, "parse_diagnostic");
}

#[test]
fn rejects_resolved_reference_without_target() {
    let error = CodeReferenceRecord::new(CodeReferenceFields {
        reference_id: "ref-1".to_owned(),
        source_scope: scope(),
        path: "src/lib.rs".to_owned(),
        symbol_text: "main".to_owned(),
        kind: CodeReferenceKind::Call,
        range: range(),
        resolution_state: CodeResolutionState::Resolved,
        target_symbol_id: None,
        extraction: extraction(),
    })
    .expect_err("target should be required");

    assert_eq!(error.field, "target_symbol_id");
}

#[test]
fn batch_rejects_duplicate_file_replacements() {
    let first = parsed_file("src/lib.rs").expect("file should validate");
    let second = parsed_file("src/lib.rs").expect("file should validate");
    let error = CodeGraphBatch::new(vec![first, second]).expect_err("duplicate should fail");

    assert_eq!(error.field, "code_file");
}

#[test]
fn chunk_deduplicates_linked_symbol_ids() {
    let chunk = CodeChunkRecord::new(
        "chunk-1",
        scope(),
        "src/lib.rs",
        "fn main() {}",
        range(),
        vec!["sym-1".to_owned(), "sym-1".to_owned()],
        Some(extraction()),
    )
    .expect("chunk should validate");

    assert_eq!(chunk.linked_symbol_ids, ["sym-1"]);
}

fn parsed_file(path: &str) -> Result<CodeFileRecord, DomainError> {
    let source_scope = scope();
    CodeFileRecord::new(CodeFileFields {
        source_scope: source_scope.clone(),
        path: path.to_owned(),
        content_hash: "hash".to_owned(),
        language_id: "rust".to_owned(),
        parse_status: CodeParseStatus::Parsed,
        diagnostic: None,
        symbols: vec![symbol("sym-1", source_scope.clone(), path)],
        references: Vec::new(),
        chunks: vec![chunk("chunk-1", source_scope, path)],
    })
}

fn symbol(id: &str, source_scope: SourceScope, path: &str) -> CodeSymbolRecord {
    CodeSymbolRecord::new(
        id,
        source_scope,
        path,
        "main",
        CodeSymbolKind::Function,
        range(),
        extraction(),
    )
    .expect("symbol should validate")
}

fn chunk(id: &str, source_scope: SourceScope, path: &str) -> CodeChunkRecord {
    CodeChunkRecord::new(
        id,
        source_scope,
        path,
        "fn main() {}",
        range(),
        Vec::new(),
        Some(extraction()),
    )
    .expect("chunk should validate")
}

fn extraction() -> CodeExtractionMetadata {
    CodeExtractionMetadata::new(
        "tree-sitter-rust@0.23",
        "rust-tags",
        "v1",
        "function_item",
        "definition.function",
    )
    .expect("extraction metadata should validate")
}

fn range() -> CodeRange {
    CodeRange::new(0, 12, 1, 1).expect("range should validate")
}

fn scope() -> SourceScope {
    SourceScope::parse("repo").expect("scope should parse")
}
