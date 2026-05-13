use std::{
    collections::BTreeSet,
    panic::{self, AssertUnwindSafe},
};

use tree_sitter::{Node, Parser, Query, QueryCursor, StreamingIterator};

use crate::domain::{
    CodeFileDiagnostic, CodeImportRecord, CodeParseStatus, RepositoryCodeChunkRecord,
    RepositoryCodeFileRecord, RepositoryCodeRange, RepositoryCodeReferenceRecord,
    RepositoryCodeSymbolRecord,
};

use super::{
    CodeIndexError, SnapshotBuild,
    languages::{LanguageSpec, detect_language, doc_comment_text, strip_supported_extension},
    stable_content_hash, stable_id,
};

const MAX_TEXT_FILE_BYTES: usize = 512 * 1024;

pub(super) fn parse_indexed_file(
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
    let imports = collect_imports(build, input.path, input.file_id, input.content, root)?;
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

fn parse_tree_safely(
    language: LanguageSpec,
    content: &str,
) -> Result<tree_sitter::Tree, CodeIndexError> {
    match panic::catch_unwind(AssertUnwindSafe(|| parse_tree(language, content))) {
        Ok(result) => result,
        Err(_) => Err(CodeIndexError::TreeSitter(
            "parser panicked while parsing file".to_owned(),
        )),
    }
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

fn extract_tag_captures_safely(
    language: LanguageSpec,
    root: Node<'_>,
    content: &str,
) -> Result<Vec<TagCapture>, CodeIndexError> {
    match panic::catch_unwind(AssertUnwindSafe(|| {
        extract_tag_captures(language, root, content)
    })) {
        Ok(result) => result,
        Err(_) => Err(CodeIndexError::TreeSitter(
            "query extraction panicked while parsing file".to_owned(),
        )),
    }
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
                capture.target_node.byte_start,
                capture.target_node.byte_end,
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
                capture.name_node.byte_start,
                capture.name_node.byte_end,
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
                && reference.byte_range.start as usize == range.byte_start
                && reference.byte_range.end as usize == range.byte_end
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
        "method_declaration" | "method_definition" | "method_signature" => "method",
        "class_declaration" | "class_definition" | "enum_declaration" | "enum_definition"
        | "enum_item" | "object_definition" | "struct_item" => "class",
        "interface_declaration" | "protocol_declaration" | "trait_definition" | "trait_item" => {
            "interface"
        }
        "namespace_definition" | "object_declaration" | "package_clause" | "package_header" => {
            "module"
        }
        "type_alias_declaration" | "type_definition" | "type_item" => "type",
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
    if is_import_node(node) {
        let module = compact_whitespace(&node_text(content, node));
        let range = syntax_range(node);
        imports.push(CodeImportRecord {
            repository_id: build.repository_id.clone(),
            source_scope: build.source_scope.clone(),
            import_id: stable_id(
                "import",
                [
                    &build.repository_id,
                    &build.source_scope,
                    path,
                    &module,
                    &range.line_start.to_string(),
                    &range.line_end.to_string(),
                ],
            ),
            file_id: file_id.to_owned(),
            path: path.to_owned(),
            module,
            target_hint: None,
            resolution_state: "unresolved".to_owned(),
            confidence_basis_points: 10_000,
            confidence_tier: "extracted".to_owned(),
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

fn is_import_node(node: Node<'_>) -> bool {
    match node.kind() {
        "import" => node.is_named(),
        "import_declaration"
        | "import_from_statement"
        | "import_statement"
        | "namespace_use_declaration"
        | "preproc_include"
        | "use_declaration"
        | "using_directive" => true,
        _ => false,
    }
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
            source_scope: build.source_scope.clone(),
            chunk_id: stable_id(
                "chunk",
                [
                    &build.repository_id,
                    &build.source_scope,
                    path,
                    &symbol.symbol_snapshot_id,
                    excerpt,
                ],
            ),
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
        source_scope: build.source_scope.clone(),
        chunk_id: stable_id(
            "chunk",
            [
                &build.repository_id,
                &build.source_scope,
                path,
                "file",
                &stable_content_hash(content.as_bytes()),
            ],
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
            &context.build.source_scope,
            context.path,
            &qualified_name,
            &range.byte_start.to_string(),
            &range.byte_end.to_string(),
        ],
    );

    Ok(RepositoryCodeSymbolRecord {
        repository_id: context.build.repository_id.clone(),
        source_scope: context.build.source_scope.clone(),
        symbol_snapshot_id,
        canonical_symbol_id: qualified_name.clone(),
        file_id: context.file_id.to_owned(),
        path: context.path.to_owned(),
        language_id: context.language_id.to_owned(),
        name: name.to_owned(),
        qualified_name,
        kind: kind.to_owned(),
        signature,
        doc_comment: doc_comment_before(context.content, range.line_start, context.language_id),
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
                &context.build.repository_id,
                &context.build.source_scope,
                context.path,
                name,
                kind,
                &range.byte_start.to_string(),
                &range.byte_end.to_string(),
            ],
        ),
        source_scope: context.build.source_scope.clone(),
        file_id: context.file_id.to_owned(),
        path: context.path.to_owned(),
        name: name.to_owned(),
        kind: kind.to_owned(),
        target_symbol_snapshot_id: None,
        target_hint: Some(name.to_owned()),
        resolution_state: "unresolved".to_owned(),
        confidence_basis_points: 5_000,
        confidence_tier: "ambiguous".to_owned(),
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
        "command_name"
            | "identifier"
            | "field_identifier"
            | "property_identifier"
            | "type_identifier"
            | "word"
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
    strip_supported_extension(path).replace(['/', '\\'], "::")
}

fn doc_comment_before(content: &str, line_start: usize, language_id: &str) -> Option<String> {
    let lines = content.lines().collect::<Vec<_>>();
    let mut cursor = line_start.saturating_sub(2);
    let mut comments = Vec::new();
    while let Some(line) = lines.get(cursor) {
        let trimmed = line.trim();
        let text = doc_comment_text(trimmed, language_id);
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
#[path = "parser_tests.rs"]
mod tests;
