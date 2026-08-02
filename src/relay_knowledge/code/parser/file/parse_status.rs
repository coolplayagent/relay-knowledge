//! File parse-status decisions, persistence, and diagnostic materialization.

use tree_sitter::Node;

use crate::{
    code::{CodeIndexError, SnapshotBuild, config_files},
    domain::{CodeFileDiagnostic, CodeImportRecord, CodeParseStatus, RepositoryCodeFileRecord},
};

use super::contracts::{FileParseOutput, SyntaxFileInput};
use crate::code::parser::recovery;

pub(super) struct FileStatusInput<'a> {
    pub(super) path: &'a str,
    pub(super) file_id: &'a str,
    pub(super) language_id: &'a str,
    pub(super) blob_hash: &'a str,
    pub(super) byte_len: usize,
    pub(super) line_count: usize,
    pub(super) parse_status: CodeParseStatus,
    pub(super) is_generated: bool,
    pub(super) degraded_reason: Option<String>,
}

pub(super) fn record_file_status(build: &mut SnapshotBuild, input: FileStatusInput<'_>) {
    build.files.push(RepositoryCodeFileRecord {
        repository_id: build.repository_id.clone(),
        source_scope: build.source_scope.clone(),
        file_id: input.file_id.to_owned(),
        path: input.path.to_owned(),
        language_id: input.language_id.to_owned(),
        blob_hash: input.blob_hash.to_owned(),
        byte_len: input.byte_len,
        line_count: input.line_count,
        parse_status: input.parse_status,
        is_generated: input.is_generated,
        degraded_reason: input.degraded_reason.clone(),
    });

    if let Some(message) = input.degraded_reason {
        build.diagnostics.push(CodeFileDiagnostic {
            repository_id: build.repository_id.clone(),
            source_scope: build.source_scope.clone(),
            path: input.path.to_owned(),
            parse_status: input.parse_status,
            message,
        });
    }
}

pub(super) fn syntax_parse_status(
    language_id: &str,
    root: Node<'_>,
    content: &str,
    output: &FileParseOutput,
    imports: &[CodeImportRecord],
) -> (CodeParseStatus, Option<String>) {
    if !root.has_error() {
        return (CodeParseStatus::Parsed, None);
    }
    let has_structured_facts =
        !(output.symbols.is_empty() && output.references.is_empty() && imports.is_empty());
    if config_files::manual_parse_status(language_id, content) {
        return (CodeParseStatus::Parsed, None);
    }
    if recovery::recoverable_c_family_parse(language_id, root, content, has_structured_facts) {
        return (CodeParseStatus::Parsed, None);
    }
    if has_structured_facts && config_files::recoverable_parse_error(language_id, content) {
        return (CodeParseStatus::Parsed, None);
    }
    (
        CodeParseStatus::Partial,
        Some("tree-sitter produced error nodes; indexed syntax facts may be partial".to_owned()),
    )
}

pub(super) fn record_tree_sitter_failure(
    build: &mut SnapshotBuild,
    input: &SyntaxFileInput<'_>,
    stage: &str,
    error: &CodeIndexError,
) {
    record_file_status(
        build,
        FileStatusInput {
            path: input.path,
            file_id: input.file_id,
            language_id: input.language.id,
            blob_hash: input.blob_hash,
            byte_len: input.byte_len,
            line_count: input.line_count,
            parse_status: CodeParseStatus::Failed,
            is_generated: input.is_generated,
            degraded_reason: Some(tree_sitter_failure_message(stage, error)),
        },
    );
}

fn tree_sitter_failure_message(stage: &str, error: &CodeIndexError) -> String {
    match error {
        CodeIndexError::TreeSitter(message) => {
            format!("tree-sitter {stage} failed: {message}")
        }
        _ => error.to_string(),
    }
}

#[cfg(test)]
#[path = "parse_status_tests.rs"]
mod tests;
