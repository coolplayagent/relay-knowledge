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
use imports::collect_imports;
use manual::collect_manual_nodes;
#[cfg(test)]
use manual::manual_definitions;
use nodes::push_children_reverse;
use records::records_from_captures;
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
    if recoverable_c_family_parse(language_id, root, content, output, imports) {
        return (CodeParseStatus::Parsed, None);
    }

    (
        CodeParseStatus::Partial,
        Some("tree-sitter produced error nodes; indexed syntax facts may be partial".to_owned()),
    )
}

fn recoverable_c_family_parse(
    language_id: &str,
    root: Node<'_>,
    content: &str,
    output: &FileParseOutput,
    imports: &[CodeImportRecord],
) -> bool {
    if !matches!(language_id, "c" | "cpp") {
        return false;
    }
    if output.symbols.is_empty() && output.references.is_empty() && imports.is_empty() {
        return false;
    }

    let mut saw_error = false;
    let mut stack = vec![root];
    while let Some(node) = stack.pop() {
        if syntax_error_node(node) {
            saw_error = true;
            if !recoverable_c_family_error(content, node) {
                return false;
            }
        }
        push_children_reverse(node, &mut stack);
    }

    saw_error
}

fn syntax_error_node(node: Node<'_>) -> bool {
    node.is_error() || node.is_missing() || node.kind() == "ERROR"
}

fn recoverable_c_family_error(content: &str, node: Node<'_>) -> bool {
    let range = nodes::syntax_range(node);
    if range.line_end.saturating_sub(range.line_start) > 2 {
        return false;
    }
    if recoverable_preprocessor_error(content, node, &range) {
        return true;
    }

    source_line(content, range.line_start).is_some_and(recoverable_c_family_error_line)
}

fn recoverable_preprocessor_error(
    content: &str,
    mut node: Node<'_>,
    range: &nodes::SyntaxRange,
) -> bool {
    let line_starts_with_directive = source_line(content, range.line_start)
        .is_some_and(|line| line.trim_start().starts_with('#'));
    loop {
        if node.kind().starts_with("preproc") {
            if matches!(
                node.kind(),
                "preproc_def" | "preproc_function_def" | "preproc_include" | "preproc_call"
            ) {
                let preprocessor_range = nodes::syntax_range(node);
                return preprocessor_range
                    .line_end
                    .saturating_sub(preprocessor_range.line_start)
                    <= 2;
            }
            return line_starts_with_directive;
        }
        let Some(parent) = node.parent() else {
            return false;
        };
        node = parent;
    }
}

fn source_line(content: &str, line_number: usize) -> Option<&str> {
    line_number
        .checked_sub(1)
        .and_then(|index| content.lines().nth(index))
}

fn recoverable_c_family_error_line(line: &str) -> bool {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return false;
    }
    if trimmed.starts_with('#') {
        return true;
    }
    if (trimmed.starts_with("template class ") || trimmed.starts_with("template struct "))
        && trimmed.contains('<')
        && trimmed.contains('>')
        && trimmed.ends_with(';')
    {
        return true;
    }
    if c_family_decorated_type_line(trimmed) {
        return true;
    }

    let Some(token) = trimmed
        .split(|character: char| !c_identifier_char(character))
        .next()
    else {
        return false;
    };
    c_family_macro_name(token) && trimmed.contains('(')
}

fn c_family_decorated_type_line(trimmed: &str) -> bool {
    let tokens = trimmed
        .split(|character: char| !(character.is_ascii_alphanumeric() || character == '_'))
        .filter(|token| !token.is_empty())
        .collect::<Vec<_>>();
    tokens.iter().enumerate().any(|(index, token)| {
        matches!(*token, "class" | "struct" | "enum" | "union")
            && (index
                .checked_sub(1)
                .and_then(|previous| tokens.get(previous))
                .is_some_and(|candidate| c_family_decorator_token(candidate))
                || tokens
                    .get(index + 1)
                    .is_some_and(|candidate| c_family_decorator_token(candidate)))
    })
}

fn c_family_decorator_token(token: &str) -> bool {
    token.starts_with("__")
        || token.ends_with("_API")
        || token.ends_with("_EXPORT")
        || token.ends_with("_EXPORTS")
        || (token
            .chars()
            .any(|character| character == '_' || character.is_ascii_uppercase())
            && token.chars().all(|character| {
                character == '_' || character.is_ascii_uppercase() || character.is_ascii_digit()
            }))
}

fn c_family_macro_name(token: &str) -> bool {
    !token.is_empty()
        && token
            .chars()
            .all(|character| character == '_' || character.is_ascii_uppercase())
        && token.chars().any(|character| character == '_')
}

fn c_identifier_char(character: char) -> bool {
    character == '_' || character.is_ascii_alphanumeric()
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
