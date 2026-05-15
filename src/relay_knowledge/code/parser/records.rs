use std::collections::BTreeSet;

use crate::domain::{
    RepositoryCodeRange, RepositoryCodeReferenceRecord, RepositoryCodeSymbolRecord,
};

use super::super::{
    CodeIndexError,
    languages::{doc_comment_text, strip_supported_extension},
    stable_id,
};
use super::{
    FileParseContext, FileParseOutput,
    nodes::{SyntaxRange, compact_whitespace},
    syntax::TagCapture,
};

pub(super) fn records_from_captures(
    context: &FileParseContext<'_>,
    captures: Vec<TagCapture>,
    output: &mut FileParseOutput,
) -> Result<(), CodeIndexError> {
    let mut seen_symbols = BTreeSet::new();
    let mut seen_references = BTreeSet::new();
    for capture in captures {
        if capture.capture_kind.starts_with("definition.") {
            if context.language_id == "c" && capture.capture_kind == "definition.function" {
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
    if let Some(existing) = output.symbols.iter_mut().find(|existing| {
        existing.name == symbol.name
            && existing.path == symbol.path
            && existing.line_range.start == symbol.line_range.start
            && symbol_kinds_overlap(&existing.kind, &symbol.kind)
            && ranges_overlap(
                existing.byte_range.start,
                existing.byte_range.end,
                symbol.byte_range.start,
                symbol.byte_range.end,
            )
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
        || matches!(
            (left, right),
            ("function", "function_declaration")
                | ("function_declaration", "function")
                | ("macro", "function")
                | ("function", "macro")
                | ("type", "class")
                | ("class", "type")
        )
}

fn symbol_preferred_over_existing(
    symbol: &RepositoryCodeSymbolRecord,
    existing: &RepositoryCodeSymbolRecord,
) -> bool {
    matches!(symbol.kind.as_str(), "function" | "macro")
        && !matches!(existing.kind.as_str(), "function" | "macro")
}

pub(super) fn upsert_reference(
    output: &mut FileParseOutput,
    reference: RepositoryCodeReferenceRecord,
) {
    if output.references.iter().any(|existing| {
        existing.name == reference.name
            && existing.path == reference.path
            && existing.line_range.start == reference.line_range.start
            && existing.byte_range.start == reference.byte_range.start
            && existing.byte_range.end == reference.byte_range.end
    }) {
        return;
    }

    output.references.push(reference);
}

pub(super) fn symbol_record(
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
