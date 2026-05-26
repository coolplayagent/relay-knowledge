use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use super::{DomainError, SourceScope, error::required_text};

/// Parser status for a repository file at a graph version.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CodeParseStatus {
    Parsed,
    Partial,
    TextOnly,
    Failed,
}

impl CodeParseStatus {
    /// Stable storage and API representation.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Parsed => "parsed",
            Self::Partial => "partial",
            Self::TextOnly => "text_only",
            Self::Failed => "failed",
        }
    }
}

/// Symbol definition category extracted from tree-sitter captures.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CodeSymbolKind {
    Function,
    Method,
    Class,
    Interface,
    Module,
    Type,
    Constant,
    Field,
    Variable,
    EnumMember,
}

impl CodeSymbolKind {
    /// Stable storage and API representation.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Function => "function",
            Self::Method => "method",
            Self::Class => "class",
            Self::Interface => "interface",
            Self::Module => "module",
            Self::Type => "type",
            Self::Constant => "constant",
            Self::Field => "field",
            Self::Variable => "variable",
            Self::EnumMember => "enum_member",
        }
    }
}

/// Reference category extracted from tree-sitter captures.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CodeReferenceKind {
    Call,
    Type,
    Import,
    Implementation,
}

impl CodeReferenceKind {
    /// Stable storage and API representation.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Call => "call",
            Self::Type => "type",
            Self::Import => "import",
            Self::Implementation => "implementation",
        }
    }
}

/// Resolution certainty for syntax-level code references.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CodeResolutionState {
    Unresolved,
    Ambiguous,
    Resolved,
}

impl CodeResolutionState {
    /// Stable storage and API representation.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Unresolved => "unresolved",
            Self::Ambiguous => "ambiguous",
            Self::Resolved => "resolved",
        }
    }
}

/// Inclusive source line range and half-open byte range in a repository file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct CodeRange {
    pub start_byte: u32,
    pub end_byte: u32,
    pub start_line: u32,
    pub end_line: u32,
}

impl CodeRange {
    /// Validates a non-empty byte range with one-based line coordinates.
    pub fn new(
        start_byte: u32,
        end_byte: u32,
        start_line: u32,
        end_line: u32,
    ) -> Result<Self, DomainError> {
        if end_byte <= start_byte {
            return Err(DomainError::invalid(
                "code_range",
                "end byte must be greater than start byte",
            ));
        }
        if start_line == 0 {
            return Err(DomainError::invalid(
                "code_range",
                "start line must be one-based",
            ));
        }
        if end_line < start_line {
            return Err(DomainError::invalid(
                "code_range",
                "end line must not be before start line",
            ));
        }

        Ok(Self {
            start_byte,
            end_byte,
            start_line,
            end_line,
        })
    }
}

/// Tree-sitter query metadata attached to extracted code facts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CodeExtractionMetadata {
    pub grammar_version: String,
    pub query_name: String,
    pub query_version: String,
    pub node_kind: String,
    pub capture_kind: String,
}

impl CodeExtractionMetadata {
    /// Validates extractor metadata needed to diagnose grammar/query drift.
    pub fn new(
        grammar_version: impl Into<String>,
        query_name: impl Into<String>,
        query_version: impl Into<String>,
        node_kind: impl Into<String>,
        capture_kind: impl Into<String>,
    ) -> Result<Self, DomainError> {
        Ok(Self {
            grammar_version: validated_text("grammar_version", grammar_version)?,
            query_name: validated_text("query_name", query_name)?,
            query_version: validated_text("query_version", query_version)?,
            node_kind: validated_text("node_kind", node_kind)?,
            capture_kind: validated_text("capture_kind", capture_kind)?,
        })
    }
}

/// Versioned syntax-level symbol definition.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CodeSymbolRecord {
    pub symbol_id: String,
    pub source_scope: SourceScope,
    pub path: String,
    pub name: String,
    pub kind: CodeSymbolKind,
    pub range: CodeRange,
    pub extraction: CodeExtractionMetadata,
}

impl CodeSymbolRecord {
    /// Validates a symbol definition extracted from a single source file.
    pub fn new(
        symbol_id: impl Into<String>,
        source_scope: SourceScope,
        path: impl Into<String>,
        name: impl Into<String>,
        kind: CodeSymbolKind,
        range: CodeRange,
        extraction: CodeExtractionMetadata,
    ) -> Result<Self, DomainError> {
        Ok(Self {
            symbol_id: validated_text("symbol_id", symbol_id)?,
            source_scope,
            path: validated_repo_path(path)?,
            name: validated_text("symbol_name", name)?,
            kind,
            range,
            extraction,
        })
    }
}

/// Versioned syntax-level reference or dependency edge candidate.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CodeReferenceRecord {
    pub reference_id: String,
    pub source_scope: SourceScope,
    pub path: String,
    pub symbol_text: String,
    pub kind: CodeReferenceKind,
    pub range: CodeRange,
    pub resolution_state: CodeResolutionState,
    pub target_symbol_id: Option<String>,
    pub extraction: CodeExtractionMetadata,
}

/// Input fields for a syntax-level reference.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodeReferenceFields {
    pub reference_id: String,
    pub source_scope: SourceScope,
    pub path: String,
    pub symbol_text: String,
    pub kind: CodeReferenceKind,
    pub range: CodeRange,
    pub resolution_state: CodeResolutionState,
    pub target_symbol_id: Option<String>,
    pub extraction: CodeExtractionMetadata,
}

impl CodeReferenceRecord {
    /// Validates a reference without upgrading unresolved syntax to certainty.
    pub fn new(fields: CodeReferenceFields) -> Result<Self, DomainError> {
        let target_symbol_id = fields
            .target_symbol_id
            .map(|id| validated_text("target_symbol_id", id))
            .transpose()?;
        if fields.resolution_state == CodeResolutionState::Resolved && target_symbol_id.is_none() {
            return Err(DomainError::invalid(
                "target_symbol_id",
                "resolved references must include a target symbol",
            ));
        }

        Ok(Self {
            reference_id: validated_text("reference_id", fields.reference_id)?,
            source_scope: fields.source_scope,
            path: validated_repo_path(fields.path)?,
            symbol_text: validated_text("symbol_text", fields.symbol_text)?,
            kind: fields.kind,
            range: fields.range,
            resolution_state: fields.resolution_state,
            target_symbol_id,
            extraction: fields.extraction,
        })
    }
}

/// Versioned retrievable code chunk.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CodeChunkRecord {
    pub chunk_id: String,
    pub source_scope: SourceScope,
    pub path: String,
    pub content: String,
    pub range: CodeRange,
    pub linked_symbol_ids: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub extraction: Option<CodeExtractionMetadata>,
}

impl CodeChunkRecord {
    /// Validates a retrievable code chunk and deduplicates linked symbols.
    pub fn new(
        chunk_id: impl Into<String>,
        source_scope: SourceScope,
        path: impl Into<String>,
        content: impl Into<String>,
        range: CodeRange,
        linked_symbol_ids: Vec<String>,
        extraction: Option<CodeExtractionMetadata>,
    ) -> Result<Self, DomainError> {
        let mut deduped = Vec::new();
        for symbol_id in linked_symbol_ids {
            let symbol_id = validated_text("linked_symbol_id", symbol_id)?;
            if !deduped.contains(&symbol_id) {
                deduped.push(symbol_id);
            }
        }

        Ok(Self {
            chunk_id: validated_text("chunk_id", chunk_id)?,
            source_scope,
            path: validated_repo_path(path)?,
            content: validated_text("chunk_content", content)?,
            range,
            linked_symbol_ids: deduped,
            extraction,
        })
    }
}

/// Parser output for one repository file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CodeFileRecord {
    pub source_scope: SourceScope,
    pub path: String,
    pub content_hash: String,
    pub language_id: String,
    pub parse_status: CodeParseStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub diagnostic: Option<String>,
    pub symbols: Vec<CodeSymbolRecord>,
    pub references: Vec<CodeReferenceRecord>,
    pub chunks: Vec<CodeChunkRecord>,
}

/// Input fields for one parsed repository file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodeFileFields {
    pub source_scope: SourceScope,
    pub path: String,
    pub content_hash: String,
    pub language_id: String,
    pub parse_status: CodeParseStatus,
    pub diagnostic: Option<String>,
    pub symbols: Vec<CodeSymbolRecord>,
    pub references: Vec<CodeReferenceRecord>,
    pub chunks: Vec<CodeChunkRecord>,
}

impl CodeFileRecord {
    /// Validates parser output and keeps extracted facts scoped to this file.
    pub fn new(fields: CodeFileFields) -> Result<Self, DomainError> {
        let path = validated_repo_path(fields.path)?;
        let diagnostic = fields
            .diagnostic
            .map(|value| validated_text("parse_diagnostic", value))
            .transpose()?;
        validate_parse_status(
            fields.parse_status,
            diagnostic.as_deref(),
            &fields.symbols,
            &fields.references,
            &fields.chunks,
        )?;
        validate_nested_facts(
            &fields.source_scope,
            &path,
            &fields.symbols,
            &fields.references,
            &fields.chunks,
        )?;

        Ok(Self {
            source_scope: fields.source_scope,
            path,
            content_hash: validated_text("content_hash", fields.content_hash)?,
            language_id: validated_text("language_id", fields.language_id)?,
            parse_status: fields.parse_status,
            diagnostic,
            symbols: fields.symbols,
            references: fields.references,
            chunks: fields.chunks,
        })
    }
}

/// Atomic code graph mutation batch.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CodeGraphBatch {
    pub files: Vec<CodeFileRecord>,
}

impl CodeGraphBatch {
    /// Creates a non-empty batch with at most one replacement per scope/path.
    pub fn new(files: Vec<CodeFileRecord>) -> Result<Self, DomainError> {
        if files.is_empty() {
            return Err(DomainError::invalid(
                "code_files",
                "must include at least one file",
            ));
        }

        let mut keys = BTreeSet::new();
        for file in &files {
            let key = (file.source_scope.as_str(), file.path.as_str());
            if !keys.insert(key) {
                return Err(DomainError::invalid(
                    "code_file",
                    "scope and path must be unique within a mutation batch",
                ));
            }
        }

        Ok(Self { files })
    }
}

/// Receipt returned after code graph facts commit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CodeGraphCommitReceipt {
    pub graph_version: super::GraphVersion,
    pub file_count: usize,
    pub symbol_count: usize,
    pub reference_count: usize,
    pub chunk_count: usize,
}

/// Parse-status counts surfaced by graph diagnostics.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CodeParseStatusCounts {
    pub parsed: usize,
    pub partial: usize,
    pub text_only: usize,
    pub failed: usize,
}

fn validate_parse_status(
    status: CodeParseStatus,
    diagnostic: Option<&str>,
    symbols: &[CodeSymbolRecord],
    references: &[CodeReferenceRecord],
    chunks: &[CodeChunkRecord],
) -> Result<(), DomainError> {
    match status {
        CodeParseStatus::Parsed => Ok(()),
        CodeParseStatus::Partial => {
            if diagnostic.is_none() {
                return Err(DomainError::invalid(
                    "parse_diagnostic",
                    "partial parses must include a diagnostic",
                ));
            }
            Ok(())
        }
        CodeParseStatus::TextOnly => {
            if !symbols.is_empty() || !references.is_empty() {
                return Err(DomainError::invalid(
                    "parse_status",
                    "text-only files cannot include syntax facts",
                ));
            }
            Ok(())
        }
        CodeParseStatus::Failed => {
            if diagnostic.is_none() {
                return Err(DomainError::invalid(
                    "parse_diagnostic",
                    "failed parses must include a diagnostic",
                ));
            }
            if !symbols.is_empty() || !references.is_empty() || !chunks.is_empty() {
                return Err(DomainError::invalid(
                    "parse_status",
                    "failed files cannot include extracted code facts",
                ));
            }
            Ok(())
        }
    }
}

fn validate_nested_facts(
    scope: &SourceScope,
    path: &str,
    symbols: &[CodeSymbolRecord],
    references: &[CodeReferenceRecord],
    chunks: &[CodeChunkRecord],
) -> Result<(), DomainError> {
    for symbol in symbols {
        validate_scope_and_path(scope, path, &symbol.source_scope, &symbol.path)?;
    }
    for reference in references {
        validate_scope_and_path(scope, path, &reference.source_scope, &reference.path)?;
    }
    for chunk in chunks {
        validate_scope_and_path(scope, path, &chunk.source_scope, &chunk.path)?;
    }

    Ok(())
}

fn validate_scope_and_path(
    expected_scope: &SourceScope,
    expected_path: &str,
    actual_scope: &SourceScope,
    actual_path: &str,
) -> Result<(), DomainError> {
    if expected_scope != actual_scope || expected_path != actual_path {
        return Err(DomainError::invalid(
            "code_file",
            "nested code facts must match the file scope and path",
        ));
    }

    Ok(())
}

fn validated_repo_path(value: impl Into<String>) -> Result<String, DomainError> {
    let path = validated_text("code_path", value)?;
    if path.starts_with('/') || path.starts_with('\\') {
        return Err(DomainError::invalid(
            "code_path",
            "must be repository-relative",
        ));
    }
    if path
        .split(['/', '\\'])
        .any(|component| component.is_empty() || component == "." || component == "..")
    {
        return Err(DomainError::invalid(
            "code_path",
            "must not contain empty, current, or parent components",
        ));
    }

    Ok(path)
}

fn validated_text(field: &'static str, value: impl Into<String>) -> Result<String, DomainError> {
    let text = required_text(field, value)?;
    if text.contains('\0') {
        return Err(DomainError::invalid(field, "must not contain NUL bytes"));
    }

    Ok(text)
}

#[cfg(test)]
mod tests {
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
}
