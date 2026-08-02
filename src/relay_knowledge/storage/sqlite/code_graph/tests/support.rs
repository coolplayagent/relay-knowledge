//! Shared records for code-graph adapter tests across physical owners.

use crate::domain::{
    CodeChunkRecord, CodeExtractionMetadata, CodeFileFields, CodeFileRecord, CodeParseStatus,
    CodeRange, CodeReferenceFields, CodeReferenceKind, CodeReferenceRecord, CodeResolutionState,
    CodeSymbolKind, CodeSymbolRecord, SourceScope,
};

pub(in crate::storage::sqlite::code_graph) fn parsed_file(
    scope: &str,
    path: &str,
    symbol_id: &str,
) -> CodeFileRecord {
    let source_scope = SourceScope::parse(scope).expect("scope should parse");
    let extraction = extraction();
    let symbol = CodeSymbolRecord::new(
        symbol_id,
        source_scope.clone(),
        path,
        "main",
        CodeSymbolKind::Function,
        range(0, 12),
        extraction.clone(),
    )
    .expect("symbol should validate");
    let reference = CodeReferenceRecord::new(CodeReferenceFields {
        reference_id: format!("ref-{symbol_id}"),
        source_scope: source_scope.clone(),
        path: path.to_owned(),
        symbol_text: "main".to_owned(),
        kind: CodeReferenceKind::Call,
        range: range(3, 7),
        resolution_state: CodeResolutionState::Resolved,
        target_symbol_id: Some(symbol_id.to_owned()),
        extraction: extraction.clone(),
    })
    .expect("reference should validate");
    let chunk = CodeChunkRecord::new(
        format!("chunk-{symbol_id}"),
        source_scope.clone(),
        path,
        "fn main() {}",
        range(0, 12),
        vec![symbol_id.to_owned()],
        Some(extraction),
    )
    .expect("chunk should validate");

    CodeFileRecord::new(CodeFileFields {
        source_scope,
        path: path.to_owned(),
        content_hash: format!("hash-{symbol_id}"),
        language_id: "rust".to_owned(),
        parse_status: CodeParseStatus::Parsed,
        diagnostic: None,
        symbols: vec![symbol],
        references: vec![reference],
        chunks: vec![chunk],
    })
    .expect("file should validate")
}

pub(in crate::storage::sqlite::code_graph) fn extraction() -> CodeExtractionMetadata {
    CodeExtractionMetadata::new(
        "tree-sitter-rust@0.23",
        "rust-tags",
        "v1",
        "function_item",
        "definition.function",
    )
    .expect("extraction should validate")
}

pub(in crate::storage::sqlite::code_graph) fn range(start: u32, end: u32) -> CodeRange {
    CodeRange::new(start, end, 1, 1).expect("range should validate")
}
