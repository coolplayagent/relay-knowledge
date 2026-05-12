use std::{collections::BTreeSet, path::Path};

use tree_sitter::{Language, Node, Parser, Query, QueryCursor, StreamingIterator};

use crate::domain::{
    RepositoryCodeChunkRecord, CodeFileDiagnostic, RepositoryCodeFileRecord, CodeImportRecord, CodeParseStatus,
    RepositoryCodeRange, RepositoryCodeReferenceRecord, RepositoryCodeSymbolRecord,
};

use super::{CodeIndexError, SnapshotBuild, stable_content_hash, stable_id};

const MAX_TEXT_FILE_BYTES: usize = 512 * 1024;

pub(super) fn language_id(path: &str) -> Option<&'static str> {
    detect_language(path).map(|language| language.id)
}

pub(super) fn parse_indexed_file(
    build: &mut SnapshotBuild,
    path: &str,
    bytes: &[u8],
) -> Result<(), CodeIndexError> {
    let blob_hash = stable_content_hash(bytes);
    let file_id = stable_id("file", [&build.repository_id, path, &blob_hash]);
    let language = detect_language(path);
    let line_count = count_lines(bytes);
    let (parse_status, degraded_reason, content) = validate_text_content(path, bytes, language)?;
    build.files.push(RepositoryCodeFileRecord {
        repository_id: build.repository_id.clone(),
        file_id: file_id.clone(),
        path: path.to_owned(),
        language_id: language.map_or("unknown", |spec| spec.id).to_owned(),
        blob_hash,
        byte_len: bytes.len(),
        line_count,
        parse_status,
        degraded_reason: degraded_reason.clone(),
    });

    if let Some(message) = degraded_reason {
        build.diagnostics.push(CodeFileDiagnostic {
            repository_id: build.repository_id.clone(),
            path: path.to_owned(),
            parse_status,
            message,
        });
    }

    let Some(content) = content else {
        return Ok(());
    };
    let Some(language) = language else {
        add_file_chunk(build, path, &file_id, "unknown", &content)?;
        return Ok(());
    };
    if parse_status == CodeParseStatus::TextOnly {
        add_file_chunk(build, path, &file_id, language.id, &content)?;
        return Ok(());
    }

    let parsed = parse_tree(language, &content)?;
    let captures = extract_tag_captures(language, parsed.root_node(), &content)?;
    let context = FileParseContext {
        build,
        path,
        file_id: &file_id,
        language_id: language.id,
        content: &content,
    };
    let mut output = FileParseOutput {
        symbols: Vec::new(),
        references: Vec::new(),
    };
    records_from_captures(&context, captures, &mut output)?;
    collect_manual_nodes(&context, parsed.root_node(), &mut output)?;
    let imports = collect_imports(build, path, &file_id, &content, parsed.root_node())?;
    let chunks = chunks_for_symbols(
        build,
        path,
        &file_id,
        language.id,
        &content,
        &output.symbols,
    )?;

    build.symbols.extend(output.symbols);
    build.references.extend(output.references);
    build.imports.extend(imports);
    build.chunks.extend(chunks);

    Ok(())
}

#[derive(Clone, Copy)]
struct LanguageSpec {
    id: &'static str,
    language: fn() -> Language,
    tags_query: &'static str,
}

fn detect_language(path: &str) -> Option<LanguageSpec> {
    let extension = Path::new(path).extension()?.to_str()?;
    match extension {
        "rs" => Some(LanguageSpec {
            id: "rust",
            language: || tree_sitter_rust::LANGUAGE.into(),
            tags_query: tree_sitter_rust::TAGS_QUERY,
        }),
        "py" => Some(LanguageSpec {
            id: "python",
            language: || tree_sitter_python::LANGUAGE.into(),
            tags_query: tree_sitter_python::TAGS_QUERY,
        }),
        "ts" => Some(LanguageSpec {
            id: "typescript",
            language: || tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
            tags_query: tree_sitter_typescript::TAGS_QUERY,
        }),
        "tsx" => Some(LanguageSpec {
            id: "tsx",
            language: || tree_sitter_typescript::LANGUAGE_TSX.into(),
            tags_query: tree_sitter_typescript::TAGS_QUERY,
        }),
        _ => None,
    }
}

fn validate_text_content(
    path: &str,
    bytes: &[u8],
    language: Option<LanguageSpec>,
) -> Result<(CodeParseStatus, Option<String>, Option<String>), CodeIndexError> {
    if bytes.contains(&0) {
        return Ok((
            CodeParseStatus::TextOnly,
            Some("binary file skipped; lexical code retrieval is unavailable".to_owned()),
            None,
        ));
    }

    let mut degraded_reasons = Vec::new();
    let content_bytes = if bytes.len() > MAX_TEXT_FILE_BYTES {
        degraded_reasons.push(format!(
            "file exceeds {MAX_TEXT_FILE_BYTES} byte code index budget"
        ));
        truncate_bytes_at_utf8_boundary(bytes, MAX_TEXT_FILE_BYTES)
    } else {
        bytes
    };
    let content = match String::from_utf8(content_bytes.to_vec()) {
        Ok(content) => content,
        Err(_) => {
            degraded_reasons.push(format!(
                "{path} is not valid UTF-8; using lossy text fallback"
            ));
            String::from_utf8_lossy(content_bytes).into_owned()
        }
    };
    if language.is_none() {
        degraded_reasons
            .push("tree-sitter grammar is not configured for this file extension".to_owned());
    }

    if !degraded_reasons.is_empty() {
        return Ok((
            CodeParseStatus::TextOnly,
            Some(degraded_reasons.join("; ")),
            Some(content),
        ));
    }

    Ok((CodeParseStatus::Parsed, None, Some(content)))
}

fn truncate_bytes_at_utf8_boundary(bytes: &[u8], max_bytes: usize) -> &[u8] {
    let end = bytes.len().min(max_bytes);
    match std::str::from_utf8(&bytes[..end]) {
        Ok(_) => &bytes[..end],
        Err(error) if error.error_len().is_none() => &bytes[..error.valid_up_to()],
        Err(_) => &bytes[..end],
    }
}

fn parse_tree(language: LanguageSpec, content: &str) -> Result<tree_sitter::Tree, CodeIndexError> {
    let mut parser = Parser::new();
    parser
        .set_language(&(language.language)())
        .map_err(|error| CodeIndexError::TreeSitter(error.to_string()))?;
    parser
        .parse(content, None)
        .ok_or_else(|| CodeIndexError::TreeSitter("parser returned no tree".to_owned()))
}

#[derive(Debug, Clone)]
struct TagCapture {
    name: String,
    capture_kind: String,
    name_node: SyntaxRange,
    target_node: SyntaxRange,
}

#[derive(Debug, Clone)]
struct SyntaxRange {
    byte_start: usize,
    byte_end: usize,
    line_start: usize,
    line_end: usize,
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

fn extract_tag_captures(
    language: LanguageSpec,
    root: Node<'_>,
    content: &str,
) -> Result<Vec<TagCapture>, CodeIndexError> {
    let query = Query::new(&(language.language)(), language.tags_query)
        .map_err(|error| CodeIndexError::TreeSitter(error.to_string()))?;
    let capture_names = query.capture_names().to_vec();
    let mut cursor = QueryCursor::new();
    let mut matches = cursor.matches(&query, root, content.as_bytes());
    let mut captures = Vec::new();

    while {
        matches.advance();
        matches.get().is_some()
    } {
        let query_match = matches.get().expect("match is present");
        let mut name_capture = None;
        let mut primary_capture = None;
        for capture in query_match.captures {
            let capture_name = capture_names[capture.index as usize];
            if capture_name == "name" {
                name_capture = Some(capture.node);
            } else if capture_name.starts_with("definition.")
                || capture_name.starts_with("reference.")
            {
                primary_capture = Some((capture_name.to_owned(), capture.node));
            }
        }
        if let (Some(name_node), Some((capture_kind, target_node))) =
            (name_capture, primary_capture)
        {
            captures.push(TagCapture {
                name: node_text(content, name_node),
                capture_kind,
                name_node: syntax_range(name_node),
                target_node: syntax_range(target_node),
            });
        }
    }

    Ok(captures)
}

fn records_from_captures(
    context: &FileParseContext<'_>,
    captures: Vec<TagCapture>,
    output: &mut FileParseOutput,
) -> Result<(), CodeIndexError> {
    let mut seen_symbols = BTreeSet::new();
    let mut seen_references = BTreeSet::new();
    for capture in captures {
        if capture.capture_kind.starts_with("definition.") {
            let kind = capture.capture_kind.trim_start_matches("definition.");
            let key = (
                capture.name.clone(),
                capture.target_node.line_start,
                kind.to_owned(),
            );
            if seen_symbols.insert(key) {
                output.symbols.push(symbol_record(
                    context,
                    &capture.name,
                    kind,
                    &capture.target_node,
                )?);
            }
        } else if capture.capture_kind.starts_with("reference.") {
            let kind = capture.capture_kind.trim_start_matches("reference.");
            let key = (
                capture.name.clone(),
                capture.name_node.line_start,
                kind.to_owned(),
            );
            if seen_references.insert(key) {
                output.references.push(reference_record(
                    context,
                    &capture.name,
                    kind,
                    &capture.name_node,
                )?);
            }
        }
    }

    Ok(())
}

fn collect_manual_nodes(
    context: &FileParseContext<'_>,
    root: Node<'_>,
    output: &mut FileParseOutput,
) -> Result<(), CodeIndexError> {
    let mut cursor = root.walk();
    for node in root.children(&mut cursor) {
        collect_manual_node(context, node, output)?;
    }

    Ok(())
}

fn collect_manual_node(
    context: &FileParseContext<'_>,
    node: Node<'_>,
    output: &mut FileParseOutput,
) -> Result<(), CodeIndexError> {
    if let Some((name, kind, range)) = manual_definition(context.content, node) {
        if !output.symbols.iter().any(|symbol| {
            symbol.name == name
                && symbol.path == context.path
                && symbol.line_range.start == range.line_start as u32
        }) {
            output
                .symbols
                .push(symbol_record(context, &name, kind, &range)?);
        }
    }
    if let Some((name, range)) = manual_call(context.content, node) {
        if !output.references.iter().any(|reference| {
            reference.name == name
                && reference.path == context.path
                && reference.line_range.start == range.line_start as u32
        }) {
            output
                .references
                .push(reference_record(context, &name, "call", &range)?);
        }
    }

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_manual_node(context, child, output)?;
    }

    Ok(())
}

fn manual_definition(content: &str, node: Node<'_>) -> Option<(String, &'static str, SyntaxRange)> {
    let kind = match node.kind() {
        "function_item" | "function_definition" | "function_declaration" => "function",
        "method_definition" | "method_signature" => "method",
        "struct_item" | "enum_item" | "class_definition" | "class_declaration" => "class",
        "interface_declaration" | "trait_item" => "interface",
        "type_item" | "type_alias_declaration" => "type",
        _ => return None,
    };
    let name = node.child_by_field_name("name")?;

    Some((node_text(content, name), kind, syntax_range(node)))
}

fn manual_call(content: &str, node: Node<'_>) -> Option<(String, SyntaxRange)> {
    match node.kind() {
        "call_expression" | "call" | "new_expression" => {
            let function = node
                .child_by_field_name("function")
                .or_else(|| node.child_by_field_name("constructor"))
                .or_else(|| node.child(0))?;
            last_identifier_text(content, function).map(|name| (name, syntax_range(function)))
        }
        _ => None,
    }
}

fn collect_imports(
    build: &SnapshotBuild,
    path: &str,
    file_id: &str,
    content: &str,
    root: Node<'_>,
) -> Result<Vec<CodeImportRecord>, CodeIndexError> {
    let mut imports = Vec::new();
    collect_import_node(build, path, file_id, content, root, &mut imports)?;

    Ok(imports)
}

fn collect_import_node(
    build: &SnapshotBuild,
    path: &str,
    file_id: &str,
    content: &str,
    node: Node<'_>,
    imports: &mut Vec<CodeImportRecord>,
) -> Result<(), CodeIndexError> {
    if matches!(
        node.kind(),
        "use_declaration" | "import_statement" | "import_from_statement"
    ) {
        let module = compact_whitespace(&node_text(content, node));
        let range = syntax_range(node);
        imports.push(CodeImportRecord {
            repository_id: build.repository_id.clone(),
            import_id: stable_id(
                "import",
                [
                    path,
                    &module,
                    &range.line_start.to_string(),
                    &range.line_end.to_string(),
                ],
            ),
            file_id: file_id.to_owned(),
            path: path.to_owned(),
            module,
            line_range: RepositoryCodeRange::new("line_range", range.line_start, range.line_end)
                .map_err(|error| CodeIndexError::InvalidInput(error.to_string()))?,
        });
    }

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_import_node(build, path, file_id, content, child, imports)?;
    }

    Ok(())
}

fn chunks_for_symbols(
    build: &SnapshotBuild,
    path: &str,
    file_id: &str,
    language_id: &str,
    content: &str,
    symbols: &[RepositoryCodeSymbolRecord],
) -> Result<Vec<RepositoryCodeChunkRecord>, CodeIndexError> {
    let mut chunks = Vec::new();
    for symbol in symbols {
        let start = symbol.byte_range.start as usize;
        let end = symbol.byte_range.end as usize;
        let excerpt = content.get(start..end).unwrap_or(&symbol.signature).trim();
        chunks.push(RepositoryCodeChunkRecord {
            repository_id: build.repository_id.clone(),
            chunk_id: stable_id("chunk", [path, &symbol.symbol_snapshot_id, excerpt]),
            file_id: file_id.to_owned(),
            path: path.to_owned(),
            language_id: language_id.to_owned(),
            content: excerpt.to_owned(),
            byte_range: symbol.byte_range.clone(),
            line_range: symbol.line_range.clone(),
            symbol_snapshot_id: Some(symbol.symbol_snapshot_id.clone()),
        });
    }
    if chunks.is_empty() {
        add_file_chunk_to_vec(build, path, file_id, language_id, content, &mut chunks)?;
    }

    Ok(chunks)
}

fn add_file_chunk(
    build: &mut SnapshotBuild,
    path: &str,
    file_id: &str,
    language_id: &str,
    content: &str,
) -> Result<(), CodeIndexError> {
    let mut chunks = Vec::new();
    add_file_chunk_to_vec(build, path, file_id, language_id, content, &mut chunks)?;
    build.chunks.extend(chunks);

    Ok(())
}

fn add_file_chunk_to_vec(
    build: &SnapshotBuild,
    path: &str,
    file_id: &str,
    language_id: &str,
    content: &str,
    chunks: &mut Vec<RepositoryCodeChunkRecord>,
) -> Result<(), CodeIndexError> {
    let byte_end = content.len();
    let line_end = count_lines(content.as_bytes()).max(1);
    chunks.push(RepositoryCodeChunkRecord {
        repository_id: build.repository_id.clone(),
        chunk_id: stable_id(
            "chunk",
            [path, "file", &stable_content_hash(content.as_bytes())],
        ),
        file_id: file_id.to_owned(),
        path: path.to_owned(),
        language_id: language_id.to_owned(),
        content: trim_to_budget(content, 8_000),
        byte_range: RepositoryCodeRange::new("byte_range", 0, byte_end)
            .map_err(|error| CodeIndexError::InvalidInput(error.to_string()))?,
        line_range: RepositoryCodeRange::new("line_range", 1, line_end)
            .map_err(|error| CodeIndexError::InvalidInput(error.to_string()))?,
        symbol_snapshot_id: None,
    });

    Ok(())
}

fn symbol_record(
    context: &FileParseContext<'_>,
    name: &str,
    kind: &str,
    range: &SyntaxRange,
) -> Result<RepositoryCodeSymbolRecord, CodeIndexError> {
    let signature = context
        .content
        .get(range.byte_start..range.byte_end)
        .unwrap_or(name)
        .lines()
        .next()
        .map(compact_whitespace)
        .filter(|line| !line.is_empty())
        .unwrap_or_else(|| name.to_owned());
    let qualified_name = format!("{}::{name}", module_path(context.path));
    let symbol_snapshot_id = stable_id(
        "symbol",
        [
            &context.build.repository_id,
            context.path,
            &qualified_name,
            &range.byte_start.to_string(),
            &range.byte_end.to_string(),
        ],
    );

    Ok(RepositoryCodeSymbolRecord {
        repository_id: context.build.repository_id.clone(),
        symbol_snapshot_id,
        file_id: context.file_id.to_owned(),
        path: context.path.to_owned(),
        language_id: context.language_id.to_owned(),
        name: name.to_owned(),
        qualified_name,
        kind: kind.to_owned(),
        signature,
        doc_comment: doc_comment_before(context.content, range.line_start),
        byte_range: RepositoryCodeRange::new("byte_range", range.byte_start, range.byte_end)
            .map_err(|error| CodeIndexError::InvalidInput(error.to_string()))?,
        line_range: RepositoryCodeRange::new("line_range", range.line_start, range.line_end)
            .map_err(|error| CodeIndexError::InvalidInput(error.to_string()))?,
    })
}

fn reference_record(
    context: &FileParseContext<'_>,
    name: &str,
    kind: &str,
    range: &SyntaxRange,
) -> Result<RepositoryCodeReferenceRecord, CodeIndexError> {
    Ok(RepositoryCodeReferenceRecord {
        repository_id: context.build.repository_id.clone(),
        reference_id: stable_id(
            "reference",
            [
                context.path,
                name,
                kind,
                &range.byte_start.to_string(),
                &range.byte_end.to_string(),
            ],
        ),
        file_id: context.file_id.to_owned(),
        path: context.path.to_owned(),
        name: name.to_owned(),
        kind: kind.to_owned(),
        target_symbol_snapshot_id: None,
        byte_range: RepositoryCodeRange::new("byte_range", range.byte_start, range.byte_end)
            .map_err(|error| CodeIndexError::InvalidInput(error.to_string()))?,
        line_range: RepositoryCodeRange::new("line_range", range.line_start, range.line_end)
            .map_err(|error| CodeIndexError::InvalidInput(error.to_string()))?,
    })
}

fn syntax_range(node: Node<'_>) -> SyntaxRange {
    SyntaxRange {
        byte_start: node.start_byte(),
        byte_end: node.end_byte(),
        line_start: node.start_position().row + 1,
        line_end: node.end_position().row + 1,
    }
}

fn node_text(content: &str, node: Node<'_>) -> String {
    node.utf8_text(content.as_bytes())
        .unwrap_or_default()
        .trim()
        .to_owned()
}

fn last_identifier_text(content: &str, node: Node<'_>) -> Option<String> {
    if matches!(
        node.kind(),
        "identifier" | "field_identifier" | "property_identifier"
    ) {
        return Some(node_text(content, node));
    }
    let mut cursor = node.walk();
    node.children(&mut cursor)
        .filter_map(|child| last_identifier_text(content, child))
        .last()
}

fn compact_whitespace(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn module_path(path: &str) -> String {
    path.trim_end_matches(".rs")
        .trim_end_matches(".py")
        .trim_end_matches(".ts")
        .trim_end_matches(".tsx")
        .replace(['/', '\\'], "::")
}

fn doc_comment_before(content: &str, line_start: usize) -> Option<String> {
    let lines = content.lines().collect::<Vec<_>>();
    let mut cursor = line_start.saturating_sub(2);
    let mut comments = Vec::new();
    while let Some(line) = lines.get(cursor) {
        let trimmed = line.trim();
        let text = trimmed
            .strip_prefix("///")
            .or_else(|| trimmed.strip_prefix("//!"))
            .or_else(|| trimmed.strip_prefix('#'))
            .or_else(|| trimmed.strip_prefix("//"))
            .map(str::trim);
        let Some(text) = text else {
            break;
        };
        comments.push(text.to_owned());
        if cursor == 0 {
            break;
        }
        cursor -= 1;
    }
    comments.reverse();
    (!comments.is_empty()).then(|| comments.join("\n"))
}

fn trim_to_budget(content: &str, max_bytes: usize) -> String {
    if content.len() <= max_bytes {
        return content.trim().to_owned();
    }
    let mut end = max_bytes;
    while !content.is_char_boundary(end) {
        end -= 1;
    }

    content[..end].trim().to_owned()
}

fn count_lines(bytes: &[u8]) -> usize {
    if bytes.is_empty() {
        return 0;
    }

    bytes.iter().filter(|byte| **byte == b'\n').count() + 1
}

#[cfg(test)]
mod tests {
    use crate::domain::{CodeParseStatus, CodeRepositoryRegistration};

    use super::*;

    #[test]
    fn tree_sitter_captures_symbols_references_imports_and_chunks() {
        let registration =
            CodeRepositoryRegistration::new("repo", "alias", "/tmp/repo", Vec::new(), Vec::new())
                .expect("registration should validate");
        let mut build = SnapshotBuild::new(
            &registration,
            "commit".to_owned(),
            "tree".to_owned(),
            true,
            1,
            0,
        );
        let source = br#"
use std::time::Duration;

/// Runs retries.
fn retry_policy() {
    sleep(Duration::from_secs(1));
}
"#;

        parse_indexed_file(&mut build, "src/lib.rs", source).expect("file should parse");
        let snapshot = build.finish();

        assert!(
            snapshot
                .symbols
                .iter()
                .any(|symbol| symbol.name == "retry_policy")
        );
        assert!(
            snapshot
                .references
                .iter()
                .any(|reference| reference.name == "sleep")
        );
        assert!(
            snapshot
                .imports
                .iter()
                .any(|import| import.module.contains("std::time"))
        );
        assert!(
            snapshot
                .chunks
                .iter()
                .any(|chunk| chunk.content.contains("retry_policy"))
        );
    }

    #[test]
    fn text_only_files_keep_bm25_fallback_chunks() {
        let registration =
            CodeRepositoryRegistration::new("repo", "alias", "/tmp/repo", Vec::new(), Vec::new())
                .expect("registration should validate");
        let mut build = SnapshotBuild::new(
            &registration,
            "commit".to_owned(),
            "tree".to_owned(),
            true,
            1,
            0,
        );

        parse_indexed_file(&mut build, "README.txt", b"RetryPolicy appears in docs")
            .expect("file should index as text");
        let snapshot = build.finish();

        assert_eq!(snapshot.files[0].parse_status, CodeParseStatus::TextOnly);
        assert_eq!(snapshot.chunks.len(), 1);
        assert!(snapshot.diagnostics[0].message.contains("grammar"));
    }

    #[test]
    fn invalid_utf8_files_degrade_to_lossy_text_chunks() {
        let registration =
            CodeRepositoryRegistration::new("repo", "alias", "/tmp/repo", Vec::new(), Vec::new())
                .expect("registration should validate");
        let mut build = SnapshotBuild::new(
            &registration,
            "commit".to_owned(),
            "tree".to_owned(),
            true,
            1,
            0,
        );

        parse_indexed_file(
            &mut build,
            "src/lib.rs",
            b"fn retry_policy() {}\n\xff\nfn caller() {}",
        )
        .expect("invalid utf8 should degrade instead of failing");
        let snapshot = build.finish();

        assert_eq!(snapshot.files[0].parse_status, CodeParseStatus::TextOnly);
        assert!(snapshot.diagnostics[0].message.contains("not valid UTF-8"));
        assert!(snapshot.chunks[0].content.contains("retry_policy"));
    }

    #[test]
    fn oversized_files_truncate_on_utf8_boundary() {
        let mut bytes = vec![b'a'; MAX_TEXT_FILE_BYTES - 1];
        bytes.extend("é".as_bytes());
        bytes.extend(b"tail");

        let (status, reason, content) =
            validate_text_content("src/lib.rs", &bytes, detect_language("src/lib.rs"))
                .expect("oversized utf8 should degrade");
        let content = content.expect("oversized file should keep fallback content");

        assert_eq!(status, CodeParseStatus::TextOnly);
        assert!(
            reason
                .expect("reason should explain budget")
                .contains("exceeds")
        );
        assert_eq!(content.len(), MAX_TEXT_FILE_BYTES - 1);
        assert!(!content.contains('\u{fffd}'));
    }
}
