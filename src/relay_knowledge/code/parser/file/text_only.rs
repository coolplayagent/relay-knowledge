//! Text-only Markdown and knowledge-map topic symbol extraction.

use crate::{
    code::{CodeIndexError, SnapshotBuild, config_files, stable_id},
    domain::{RepositoryCodeRange, RepositoryCodeSymbolRecord},
    project::{KNOWLEDGE_MAP_RELATIVE_PATH, KNOWLEDGE_MAP_TOPICS_RELATIVE_PREFIX},
};

use super::contracts::{FileParseContext, FileParseOutput};
use crate::code::parser::records::upsert_symbol;

pub(super) fn record_topic_symbols(
    build: &mut SnapshotBuild,
    path: &str,
    file_id: &str,
    language_id: &str,
    bytes: &[u8],
) -> Result<(), CodeIndexError> {
    if !topic_source(path, language_id) {
        return Ok(());
    }
    let context = FileParseContext {
        build,
        path,
        file_id,
        language_id,
        content: "",
    };
    let mut output = FileParseOutput::new();
    match language_id {
        "markdown" => record_markdown_headings(&context, &mut output, bytes)?,
        "yaml" if knowledge_map_path(path) => {
            record_knowledge_map_facts(&context, &mut output, bytes)?;
        }
        _ => {}
    }
    build.symbols.extend(output.symbols);

    Ok(())
}

fn topic_source(path: &str, language_id: &str) -> bool {
    language_id == "markdown" || (language_id == "yaml" && knowledge_map_path(path))
}

fn knowledge_map_path(path: &str) -> bool {
    path == KNOWLEDGE_MAP_RELATIVE_PATH || path.starts_with(KNOWLEDGE_MAP_TOPICS_RELATIVE_PREFIX)
}

fn record_markdown_headings(
    context: &FileParseContext<'_>,
    output: &mut FileParseOutput,
    bytes: &[u8],
) -> Result<(), CodeIndexError> {
    let mut fence = None;
    scan_lines(bytes, |line| {
        if let Some(active) = fence {
            if markdown_structural_line(line.text)
                .is_some_and(|trimmed| closes_markdown_fence(trimmed, active))
            {
                fence = None;
            }
            return Ok(());
        }
        let Some(trimmed) = markdown_structural_line(line.text) else {
            return Ok(());
        };
        if let Some(marker) = markdown_fence_marker(trimmed) {
            fence = Some(marker);
            return Ok(());
        }

        let level = trimmed
            .chars()
            .take_while(|character| *character == '#')
            .count();
        if (1..=6).contains(&level) && trimmed.as_bytes().get(level) == Some(&b' ') {
            record_symbol(context, output, trimmed[level..].trim(), "heading", &line)?;
        }

        Ok(())
    })
}

fn record_knowledge_map_facts(
    context: &FileParseContext<'_>,
    output: &mut FileParseOutput,
    bytes: &[u8],
) -> Result<(), CodeIndexError> {
    let Ok(content) = std::str::from_utf8(bytes) else {
        return Ok(());
    };
    let (definitions, _) = config_files::structured_facts(context.path, "yaml", content);
    for definition in definitions
        .into_iter()
        .filter(|definition| definition.kind.starts_with("knowledge_map_"))
    {
        let line = TextOnlyLine {
            number: definition.range.line_start,
            byte_start: definition.range.byte_start,
            byte_end: definition.range.byte_end,
            text: &definition.name,
        };
        record_symbol(context, output, &definition.name, definition.kind, &line)?;
    }
    Ok(())
}

fn record_symbol(
    context: &FileParseContext<'_>,
    output: &mut FileParseOutput,
    name: &str,
    kind: &'static str,
    line: &TextOnlyLine<'_>,
) -> Result<(), CodeIndexError> {
    if name.is_empty() {
        return Ok(());
    }
    let qualified_name = format!("{}::{name}", module_path(context.path));
    let symbol_snapshot_id = stable_id(
        "symbol",
        [
            &context.build.repository_id,
            &context.build.source_scope,
            context.path,
            &qualified_name,
            &line.byte_start.to_string(),
            &line.byte_end.to_string(),
        ],
    );
    let symbol = RepositoryCodeSymbolRecord {
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
        signature: signature(line.text, name),
        doc_comment: None,
        byte_range: RepositoryCodeRange::new("byte_range", line.byte_start, line.byte_end)
            .map_err(|error| CodeIndexError::InvalidInput(error.to_string()))?,
        line_range: RepositoryCodeRange::new("line_range", line.number, line.number)
            .map_err(|error| CodeIndexError::InvalidInput(error.to_string()))?,
        symbol_role: None,
    };
    upsert_symbol(output, symbol);

    Ok(())
}

fn scan_lines(
    bytes: &[u8],
    mut visit: impl FnMut(TextOnlyLine<'_>) -> Result<(), CodeIndexError>,
) -> Result<(), CodeIndexError> {
    let mut byte_start = 0usize;
    for (index, raw_line) in bytes.split_inclusive(|byte| *byte == b'\n').enumerate() {
        let without_lf = raw_line.strip_suffix(b"\n").unwrap_or(raw_line);
        let text_bytes = without_lf.strip_suffix(b"\r").unwrap_or(without_lf);
        let Ok(text) = std::str::from_utf8(text_bytes) else {
            byte_start += raw_line.len();
            continue;
        };
        visit(TextOnlyLine {
            number: index + 1,
            byte_start,
            byte_end: byte_start + text_bytes.len(),
            text,
        })?;
        byte_start += raw_line.len();
    }

    Ok(())
}

struct TextOnlyLine<'a> {
    number: usize,
    byte_start: usize,
    byte_end: usize,
    text: &'a str,
}

fn module_path(path: &str) -> String {
    path.rsplit_once('.')
        .map_or(path, |(base, _)| base)
        .replace(['/', '\\'], "::")
}

fn signature(line: &str, fallback: &str) -> String {
    const MAX_SIGNATURE_BYTES: usize = 512;

    let trimmed = line.trim();
    if trimmed.is_empty() {
        return fallback.to_owned();
    }
    let mut signature = String::new();
    for character in trimmed.chars() {
        if signature.len().saturating_add(character.len_utf8()) > MAX_SIGNATURE_BYTES {
            break;
        }
        signature.push(character);
    }

    if signature.is_empty() {
        fallback.to_owned()
    } else {
        signature
    }
}

fn markdown_structural_line(line: &str) -> Option<&str> {
    if line.starts_with('\t') {
        return None;
    }
    let spaces = line
        .chars()
        .take_while(|character| *character == ' ')
        .count();
    (spaces <= 3).then(|| &line[spaces..])
}

fn markdown_fence_marker(trimmed: &str) -> Option<(char, usize)> {
    let marker = trimmed
        .chars()
        .next()
        .filter(|character| matches!(*character, '`' | '~'))?;
    let count = trimmed
        .chars()
        .take_while(|character| *character == marker)
        .count();
    (count >= 3).then_some((marker, count))
}

fn closes_markdown_fence(trimmed: &str, active: (char, usize)) -> bool {
    let (marker, count) = active;
    trimmed
        .chars()
        .take_while(|character| *character == marker)
        .count()
        >= count
}

#[cfg(test)]
#[path = "text_only_tests.rs"]
mod tests;
