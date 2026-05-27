mod chunks;
mod dependencies;
mod imports;
mod language_nodes;
mod manual;
mod nodes;
#[path = "parser_c.rs"]
mod parser_c;
#[path = "parser_c_preprocessor.rs"]
mod parser_c_preprocessor;
#[path = "parser_cpp.rs"]
mod parser_cpp;
mod records;
mod recovery;
mod syntax;
mod text;

use std::collections::HashSet;

use crate::domain::{
    CodeFileDiagnostic, CodeImportRecord, CodeParseStatus, RepositoryCodeFileRecord,
    RepositoryCodeReferenceRecord, RepositoryCodeSymbolRecord,
};
use tree_sitter::Node;

use super::{
    CodeIndexError, SnapshotBuild,
    feature_flags::{FeatureFlagFileInput, extract_feature_flags},
    languages::{LanguageSpec, detect_language},
    stable_content_hash, stable_id,
};
use chunks::{add_file_chunk, chunks_for_symbols};
use dependencies::collect_dependencies;
pub(in crate::code) use dependencies::dependency_manifest_language_ids;
use imports::collect_imports;
use manual::collect_manual_nodes;
#[cfg(test)]
use manual::manual_definitions;
#[cfg(test)]
use nodes::push_children_reverse;
use records::records_from_captures;
#[cfg(test)]
use recovery::{
    c_family_typedef_like_function_signature, recoverable_c_family_error_line,
    recoverable_decorated_function_error_text, recoverable_decorated_type_error_text,
};
#[cfg(test)]
use syntax::parse_tree;
use syntax::{extract_tag_captures_safely, parse_tree_safely};
#[cfg(test)]
use text::MAX_TEXT_FILE_BYTES;
use text::{count_lines, validate_text_content};

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
        record_dependencies(build, path, &file_id, &content)?;
        record_feature_flags(build, path, &file_id, "unknown", &content)?;
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
        record_dependencies(build, path, &file_id, &content)?;
        record_feature_flags(build, path, &file_id, language.id, &content)?;
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
            record_feature_flags(
                build,
                input.path,
                input.file_id,
                input.language.id,
                input.content,
            )?;
            return Ok(());
        }
    };
    let root = parsed.root_node();
    let captures = match extract_tag_captures_safely(input.language, root, input.content) {
        Ok(captures) => captures,
        Err(error) => {
            record_tree_sitter_failure(build, &input, "query", &error);
            record_feature_flags(
                build,
                input.path,
                input.file_id,
                input.language.id,
                input.content,
            )?;
            return Ok(());
        }
    };
    let context = FileParseContext {
        build,
        path: input.path,
        file_id: input.file_id,
        language_id: input.language.id,
        content: input.content,
    };
    let mut output = FileParseOutput::new();
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
    let (parse_status, degraded_reason) =
        syntax_parse_status(input.language.id, root, input.content, &output, &imports);
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

    build.symbols.extend(output.symbols);
    build.references.extend(output.references);
    build.imports.extend(imports);
    record_dependencies(build, input.path, input.file_id, input.content)?;
    record_feature_flags(
        build,
        input.path,
        input.file_id,
        input.language.id,
        input.content,
    )?;
    build.chunks.extend(chunks);

    Ok(())
}

fn syntax_parse_status(
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
    if recovery::recoverable_c_family_parse(language_id, root, content, has_structured_facts) {
        return (CodeParseStatus::Parsed, None);
    }

    (
        CodeParseStatus::Partial,
        Some("tree-sitter produced error nodes; indexed syntax facts may be partial".to_owned()),
    )
}

fn record_dependencies(
    build: &mut SnapshotBuild,
    path: &str,
    file_id: &str,
    content: &str,
) -> Result<(), CodeIndexError> {
    let records = collect_dependencies(build, path, file_id, content)?;
    build.dependencies.extend(records);
    Ok(())
}

fn record_feature_flags(
    build: &mut SnapshotBuild,
    path: &str,
    file_id: &str,
    language_id: &str,
    content: &str,
) -> Result<(), CodeIndexError> {
    let records = extract_feature_flags(FeatureFlagFileInput {
        repository_id: &build.repository_id,
        source_scope: &build.source_scope,
        file_id,
        path,
        language_id,
        content,
    })
    .map_err(|error| CodeIndexError::InvalidInput(error.to_string()))?;
    build.feature_flags.extend(records);

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
    reference_keys: HashSet<ReferenceDedupKey>,
}

type ReferenceDedupKey = (String, String, u32, u32, u32);

impl FileParseOutput {
    fn new() -> Self {
        Self {
            symbols: Vec::new(),
            references: Vec::new(),
            reference_keys: HashSet::new(),
        }
    }
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
#[path = "parser_enum_tests.rs"]
mod enum_tests;

#[cfg(test)]
#[path = "parser_review_tests.rs"]
mod review_tests;

#[cfg(test)]
#[path = "parser_gcc_recovery_tests.rs"]
mod gcc_recovery_tests;

#[cfg(test)]
#[path = "parser_import_resolution_tests.rs"]
mod import_resolution_tests;
