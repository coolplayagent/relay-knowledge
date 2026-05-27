use std::collections::HashSet;

use crate::domain::{
    RepositoryCodeRange, RepositoryCodeReferenceRecord, RepositoryCodeSymbolRecord,
};

use super::super::{
    CodeIndexError,
    languages::{doc_comment_text, strip_supported_extension},
    stable_id,
};
use super::{
    FileParseContext, FileParseOutput, ReferenceDedupKey, nodes::SyntaxRange, syntax::TagCapture,
};

pub(super) fn records_from_captures(
    context: &FileParseContext<'_>,
    captures: Vec<TagCapture>,
    output: &mut FileParseOutput,
) -> Result<(), CodeIndexError> {
    let mut seen_symbols = HashSet::new();
    let mut seen_references = HashSet::new();
    for capture in captures {
        if capture.capture_kind.starts_with("definition.") {
            if context.language_id == "c" && capture.capture_kind == "definition.function" {
                continue;
            }
            if context.language_id == "cpp"
                && capture.capture_kind == "definition.function"
                && capture.target_has_error
            {
                continue;
            }
            if context.language_id == "cpp"
                && capture.capture_kind == "definition.function"
                && capture
                    .target_node
                    .line_start
                    .checked_sub(1)
                    .and_then(|index| context.content.lines().nth(index))
                    .is_some_and(|line| line.contains(&format!("~{}", capture.name)))
            {
                continue;
            }
            let kind = capture.capture_kind.trim_start_matches("definition.");
            let key = (
                capture.name.clone(),
                capture.target_node.byte_start,
                capture.target_node.byte_end,
                kind.to_owned(),
            );
            if seen_symbols.insert(key) {
                upsert_symbol(
                    output,
                    symbol_record(context, &capture.name, kind, &capture.target_node)?,
                );
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
                upsert_reference(
                    output,
                    reference_record(context, &capture.name, kind, &capture.name_node)?,
                );
            }
        }
    }

    Ok(())
}

pub(super) fn upsert_symbol(output: &mut FileParseOutput, symbol: RepositoryCodeSymbolRecord) {
    if symbol.kind == "function" && symbol.signature.contains("::~") {
        return;
    }
    if let Some(existing) = output.symbols.iter_mut().find(|existing| {
        existing.symbol_snapshot_id == symbol.symbol_snapshot_id
            || (existing.name == symbol.name
                && existing.path == symbol.path
                && existing.line_range.start == symbol.line_range.start
                && symbol_kinds_overlap(&existing.kind, &symbol.kind)
                && ranges_overlap(
                    existing.byte_range.start,
                    existing.byte_range.end,
                    symbol.byte_range.start,
                    symbol.byte_range.end,
                ))
    }) {
        let existing_width = existing
            .byte_range
            .end
            .saturating_sub(existing.byte_range.start);
        let symbol_width = symbol
            .byte_range
            .end
            .saturating_sub(symbol.byte_range.start);
        if symbol_width > existing_width || symbol_preferred_over_existing(&symbol, existing) {
            *existing = symbol;
        }
        return;
    }

    output.symbols.push(symbol);
}

fn ranges_overlap(left_start: u32, left_end: u32, right_start: u32, right_end: u32) -> bool {
    left_start < right_end && right_start < left_end
}

fn symbol_kinds_overlap(left: &str, right: &str) -> bool {
    left == right
        || (function_like_symbol_kind(left) && function_like_symbol_kind(right))
        || (function_like_symbol_kind(left) && value_like_symbol_kind(right))
        || (value_like_symbol_kind(left) && function_like_symbol_kind(right))
        || (type_like_symbol_kind(left) && type_like_symbol_kind(right))
        || matches!(
            (left, right),
            ("function", "function_declaration")
                | ("function_declaration", "function")
                | ("macro", "function")
                | ("function", "macro")
        )
}

fn function_like_symbol_kind(kind: &str) -> bool {
    matches!(
        kind,
        "constructor" | "function" | "function_declaration" | "method"
    )
}

fn type_like_symbol_kind(kind: &str) -> bool {
    matches!(kind, "class" | "interface" | "type")
}

fn value_like_symbol_kind(kind: &str) -> bool {
    matches!(kind, "constant" | "variable")
}

fn symbol_preferred_over_existing(
    symbol: &RepositoryCodeSymbolRecord,
    existing: &RepositoryCodeSymbolRecord,
) -> bool {
    matches!(symbol.kind.as_str(), "function" | "macro")
        && !matches!(
            existing.kind.as_str(),
            "constructor" | "function" | "macro" | "method"
        )
}

pub(super) fn upsert_reference(
    output: &mut FileParseOutput,
    reference: RepositoryCodeReferenceRecord,
) {
    if !output
        .reference_keys
        .insert(reference_dedup_key(&reference))
    {
        return;
    }

    output.references.push(reference);
}

fn reference_dedup_key(reference: &RepositoryCodeReferenceRecord) -> ReferenceDedupKey {
    (
        reference.name.clone(),
        reference.path.clone(),
        reference.line_range.start,
        reference.byte_range.start,
        reference.byte_range.end,
    )
}

pub(super) fn symbol_record(
    context: &FileParseContext<'_>,
    name: &str,
    kind: &str,
    range: &SyntaxRange,
) -> Result<RepositoryCodeSymbolRecord, CodeIndexError> {
    let signature = symbol_signature(context.content, range, name);
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

fn symbol_signature(content: &str, range: &SyntaxRange, fallback: &str) -> String {
    const MAX_SIGNATURE_LINES: usize = 8;
    const MAX_SIGNATURE_BYTES: usize = 512;

    let Some(source) = content.get(range.byte_start..range.byte_end) else {
        return fallback.to_owned();
    };
    let mut signature = String::new();
    let mut signature_lines = 0;
    for line in source.lines() {
        if line.split_whitespace().next().is_none() {
            continue;
        }
        signature_lines += 1;
        let budget_reached = append_compact_line(&mut signature, line, MAX_SIGNATURE_BYTES);
        if signature_looks_complete(&signature) || signature.len() >= MAX_SIGNATURE_BYTES {
            break;
        }
        if budget_reached || signature_lines >= MAX_SIGNATURE_LINES {
            break;
        }
    }

    if signature.is_empty() {
        return fallback.to_owned();
    }
    truncate_to_char_boundary(&mut signature, MAX_SIGNATURE_BYTES);
    signature
}

fn signature_looks_complete(signature: &str) -> bool {
    let trimmed = signature.trim_end();
    trimmed.ends_with('{') || trimmed.ends_with(';') || trimmed.ends_with(':')
}

fn append_compact_line(signature: &mut String, line: &str, max_bytes: usize) -> bool {
    if !signature.is_empty() && !push_char_within_budget(signature, ' ', max_bytes) {
        return true;
    }

    let mut first_word = true;
    for word in line.split_whitespace() {
        if first_word {
            first_word = false;
        } else if !push_char_within_budget(signature, ' ', max_bytes) {
            return true;
        }
        if !push_str_within_budget(signature, word, max_bytes) {
            return true;
        }
    }

    false
}

fn push_str_within_budget(signature: &mut String, value: &str, max_bytes: usize) -> bool {
    let remaining = max_bytes.saturating_sub(signature.len());
    if value.len() <= remaining {
        signature.push_str(value);
        return true;
    }

    for character in value.chars() {
        if !push_char_within_budget(signature, character, max_bytes) {
            return false;
        }
    }

    true
}

fn push_char_within_budget(signature: &mut String, character: char, max_bytes: usize) -> bool {
    if signature.len().saturating_add(character.len_utf8()) > max_bytes {
        return false;
    }
    signature.push(character);

    true
}

fn truncate_to_char_boundary(value: &mut String, max_bytes: usize) {
    if value.len() <= max_bytes {
        return;
    }
    let mut end = max_bytes;
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    value.truncate(end);
}

pub(super) fn reference_record(
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
