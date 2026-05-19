mod chunks;
mod imports;
mod language_nodes;
mod manual;
mod nodes;
#[path = "parser_c.rs"]
mod parser_c;
#[path = "parser_cpp.rs"]
mod parser_cpp;
mod records;
mod syntax;
mod text;

use crate::domain::{
    CodeFileDiagnostic, CodeParseStatus, RepositoryCodeFileRecord, RepositoryCodeReferenceRecord,
    RepositoryCodeSymbolRecord,
};

use super::{
    CodeIndexError, SnapshotBuild,
    languages::{LanguageSpec, detect_language},
    stable_content_hash, stable_id,
};
use chunks::{add_file_chunk, chunks_for_symbols};
use imports::collect_imports;
use manual::collect_manual_nodes;
#[cfg(test)]
use manual::manual_definitions;
use records::records_from_captures;
#[cfg(test)]
use syntax::parse_tree;
use syntax::{extract_tag_captures_safely, parse_tree_safely};
#[cfg(test)]
use text::MAX_TEXT_FILE_BYTES;
use text::{count_lines, validate_text_content};

#[cfg(test)]
use nodes::push_children_reverse;

pub(in crate::code) fn parse_indexed_file(
    build: &mut SnapshotBuild,
    path: &str,
    bytes: &[u8],
) -> Result<(), CodeIndexError> {
    let blob_hash = stable_content_hash(bytes);
    let file_id = stable_id(
        "file",
        [&build.repository_id, &build.source_scope, path, &blob_hash],
    );
    let language = detect_language(path);
    let line_count = count_lines(bytes);
    let (parse_status, degraded_reason, content) = validate_text_content(path, bytes, language)?;

    let Some(content) = content else {
        record_file_status(
            build,
            FileStatusInput {
                path,
                file_id: &file_id,
                language_id: language.map_or("unknown", |spec| spec.id),
                blob_hash: &blob_hash,
                byte_len: bytes.len(),
                line_count,
                parse_status,
                degraded_reason,
            },
        );
        return Ok(());
    };
    let Some(language) = language else {
        record_file_status(
            build,
            FileStatusInput {
                path,
                file_id: &file_id,
                language_id: "unknown",
                blob_hash: &blob_hash,
                byte_len: bytes.len(),
                line_count,
                parse_status,
                degraded_reason,
            },
        );
        add_file_chunk(build, path, &file_id, "unknown", &content)?;
        return Ok(());
    };
    if parse_status == CodeParseStatus::TextOnly {
        record_file_status(
            build,
            FileStatusInput {
                path,
                file_id: &file_id,
                language_id: language.id,
                blob_hash: &blob_hash,
                byte_len: bytes.len(),
                line_count,
                parse_status,
                degraded_reason,
            },
        );
        add_file_chunk(build, path, &file_id, language.id, &content)?;
        return Ok(());
    }

    parse_syntax_file(
        build,
        SyntaxFileInput {
            path,
            file_id: &file_id,
            language,
            blob_hash: &blob_hash,
            byte_len: bytes.len(),
            line_count,
            content: &content,
        },
    )
}

struct FileStatusInput<'a> {
    path: &'a str,
    file_id: &'a str,
    language_id: &'a str,
    blob_hash: &'a str,
    byte_len: usize,
    line_count: usize,
    parse_status: CodeParseStatus,
    degraded_reason: Option<String>,
}

fn record_file_status(build: &mut SnapshotBuild, input: FileStatusInput<'_>) {
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

struct SyntaxFileInput<'a> {
    path: &'a str,
    file_id: &'a str,
    language: LanguageSpec,
    blob_hash: &'a str,
    byte_len: usize,
    line_count: usize,
    content: &'a str,
}

fn parse_syntax_file(
    build: &mut SnapshotBuild,
    input: SyntaxFileInput<'_>,
) -> Result<(), CodeIndexError> {
    let parsed = match parse_tree_safely(input.language, input.content) {
        Ok(parsed) => parsed,
        Err(error) => {
            record_tree_sitter_failure(build, &input, "parse", &error);
            return Ok(());
        }
    };
    let root = parsed.root_node();
    let captures = match extract_tag_captures_safely(input.language, root, input.content) {
        Ok(captures) => captures,
        Err(error) => {
            record_tree_sitter_failure(build, &input, "query", &error);
            return Ok(());
        }
    };
    let (parse_status, degraded_reason) = if root.has_error() {
        (
            CodeParseStatus::Partial,
            Some(
                "tree-sitter produced error nodes; indexed syntax facts may be partial".to_owned(),
            ),
        )
    } else {
        (CodeParseStatus::Parsed, None)
    };
    record_file_status(
        build,
        FileStatusInput {
            path: input.path,
            file_id: input.file_id,
            language_id: input.language.id,
            blob_hash: input.blob_hash,
            byte_len: input.byte_len,
            line_count: input.line_count,
            parse_status,
            degraded_reason,
        },
    );
    let context = FileParseContext {
        build,
        path: input.path,
        file_id: input.file_id,
        language_id: input.language.id,
        content: input.content,
    };
    let mut output = FileParseOutput {
        symbols: Vec::new(),
        references: Vec::new(),
    };
    records_from_captures(&context, captures, &mut output)?;
    collect_manual_nodes(&context, root, &mut output)?;
    let imports = collect_imports(
        build,
        input.path,
        input.file_id,
        input.language.id,
        input.content,
        root,
    )?;
    let chunks = chunks_for_symbols(
        build,
        input.path,
        input.file_id,
        input.language.id,
        input.content,
        &output.symbols,
    )?;

    build.symbols.extend(output.symbols);
    build.references.extend(output.references);
    build.imports.extend(imports);
    build.chunks.extend(chunks);

    Ok(())
}

fn record_tree_sitter_failure(
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

struct FileParseContext<'a> {
    build: &'a SnapshotBuild,
    path: &'a str,
    file_id: &'a str,
    language_id: &'a str,
    content: &'a str,
}

struct FileParseOutput {
    symbols: Vec<RepositoryCodeSymbolRecord>,
    references: Vec<RepositoryCodeReferenceRecord>,
}

#[cfg(test)]
#[path = "parser_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "parser_exported_value_tests.rs"]
mod exported_value_tests;

#[cfg(test)]
#[path = "parser_identity_tests.rs"]
mod identity_tests;

#[cfg(test)]
#[path = "parser_c_tests.rs"]
mod c_tests;

#[cfg(test)]
#[path = "parser_import_resolution_tests.rs"]
mod import_resolution_tests;
