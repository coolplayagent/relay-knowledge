//! Source-surface chunk construction and bounded content retention.

use crate::domain::{RepositoryCodeChunkRecord, RepositoryCodeRange, RepositoryCodeSymbolRecord};

use super::{
    super::{CodeIndexError, SnapshotBuild, stable_content_hash, stable_id},
    text::count_lines,
};

const MAX_SOURCE_SURFACE_CHUNK_BYTES: usize = 8_000;
const MAX_SOURCE_SURFACE_CHUNK_LINES: usize = 200;
const MIN_DENSE_SOURCE_SYMBOLS: usize = 64;
const DENSE_SOURCE_SYMBOLS_PER_WINDOW: usize = 4;

pub(super) fn chunks_for_symbols(
    build: &SnapshotBuild,
    path: &str,
    file_id: &str,
    language_id: &str,
    content: &str,
    symbols: &[RepositoryCodeSymbolRecord],
) -> Result<Vec<RepositoryCodeChunkRecord>, CodeIndexError> {
    if language_uses_file_surface_chunks(language_id) {
        return bounded_file_surface_chunks(build, path, file_id, language_id, content);
    }
    if uses_dense_source_windows(content, symbols) {
        let mut chunks = bounded_file_surface_chunks(build, path, file_id, language_id, content)?;
        for symbol in symbols
            .iter()
            .filter(|symbol| symbol_requires_context_chunk(symbol))
        {
            chunks.push(chunk_for_symbol(
                build,
                path,
                file_id,
                language_id,
                content,
                symbol,
            ));
        }
        return Ok(chunks);
    }
    let mut chunks = Vec::new();
    for symbol in symbols {
        chunks.push(chunk_for_symbol(
            build,
            path,
            file_id,
            language_id,
            content,
            symbol,
        ));
    }
    if chunks.is_empty() || keeps_file_chunk_with_symbol_chunks(content, symbols) {
        add_file_chunk_to_vec(build, path, file_id, language_id, content, &mut chunks)?;
    }

    Ok(chunks)
}

fn uses_dense_source_windows(content: &str, symbols: &[RepositoryCodeSymbolRecord]) -> bool {
    if symbols.len() < MIN_DENSE_SOURCE_SYMBOLS {
        return false;
    }
    let byte_windows = content.len().div_ceil(MAX_SOURCE_SURFACE_CHUNK_BYTES);
    let line_windows = count_lines(content.as_bytes()).div_ceil(MAX_SOURCE_SURFACE_CHUNK_LINES);
    let surface_windows = byte_windows.max(line_windows).max(1);

    symbols.len()
        > surface_windows
            .saturating_mul(DENSE_SOURCE_SYMBOLS_PER_WINDOW)
            .max(MIN_DENSE_SOURCE_SYMBOLS - 1)
}

fn symbol_requires_context_chunk(symbol: &RepositoryCodeSymbolRecord) -> bool {
    matches!(
        symbol.kind.as_str(),
        "constructor" | "function" | "function_declaration" | "method"
    )
}

fn chunk_for_symbol(
    build: &SnapshotBuild,
    path: &str,
    file_id: &str,
    language_id: &str,
    content: &str,
    symbol: &RepositoryCodeSymbolRecord,
) -> RepositoryCodeChunkRecord {
    let start = symbol.byte_range.start as usize;
    let end = symbol.byte_range.end as usize;
    let excerpt = content.get(start..end).unwrap_or(&symbol.signature).trim();
    RepositoryCodeChunkRecord {
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
        content: trim_to_budget(excerpt, MAX_SOURCE_SURFACE_CHUNK_BYTES),
        byte_range: symbol.byte_range.clone(),
        line_range: symbol.line_range.clone(),
        symbol_snapshot_id: Some(symbol.symbol_snapshot_id.clone()),
    }
}

fn keeps_file_chunk_with_symbol_chunks(
    content: &str,
    symbols: &[RepositoryCodeSymbolRecord],
) -> bool {
    content.len() <= MAX_SOURCE_SURFACE_CHUNK_BYTES
        && count_lines(content.as_bytes()) <= MAX_SOURCE_SURFACE_CHUNK_LINES
        && has_uncovered_source_surface(content, symbols)
}

fn language_uses_file_surface_chunks(language_id: &str) -> bool {
    matches!(
        language_id,
        "cmake"
            | "dockerfile"
            | "gomod"
            | "gotemplate"
            | "ini"
            | "jinja2"
            | "json"
            | "make"
            | "markdown"
            | "ninja"
            | "properties"
            | "starlark"
            | "toml"
            | "xml"
            | "yaml"
    )
}

fn bounded_file_surface_chunks(
    build: &SnapshotBuild,
    path: &str,
    file_id: &str,
    language_id: &str,
    content: &str,
) -> Result<Vec<RepositoryCodeChunkRecord>, CodeIndexError> {
    if keeps_complete_manifest_content(path)
        || (content.len() <= MAX_SOURCE_SURFACE_CHUNK_BYTES
            && count_lines(content.as_bytes()) <= MAX_SOURCE_SURFACE_CHUNK_LINES)
    {
        let mut chunks = Vec::new();
        add_file_chunk_to_vec(build, path, file_id, language_id, content, &mut chunks)?;
        return Ok(chunks);
    }

    let mut chunks = Vec::new();
    let mut byte_start = 0usize;
    let mut line_start = 1usize;
    while byte_start < content.len() {
        let byte_end = file_surface_window_end(content, byte_start);
        let excerpt = &content[byte_start..byte_end];
        let line_end = line_start + excerpt.bytes().filter(|byte| *byte == b'\n').count();
        chunks.push(RepositoryCodeChunkRecord {
            repository_id: build.repository_id.clone(),
            source_scope: build.source_scope.clone(),
            chunk_id: stable_id(
                "chunk",
                [
                    &build.repository_id,
                    &build.source_scope,
                    path,
                    "file-window",
                    &byte_start.to_string(),
                    &byte_end.to_string(),
                    &stable_content_hash(excerpt.as_bytes()),
                ],
            ),
            file_id: file_id.to_owned(),
            path: path.to_owned(),
            language_id: language_id.to_owned(),
            content: if language_id == "markdown" {
                excerpt.to_owned()
            } else {
                excerpt.trim().to_owned()
            },
            byte_range: RepositoryCodeRange::new("byte_range", byte_start, byte_end)
                .map_err(|error| CodeIndexError::InvalidInput(error.to_string()))?,
            line_range: RepositoryCodeRange::new("line_range", line_start, line_end)
                .map_err(|error| CodeIndexError::InvalidInput(error.to_string()))?,
            symbol_snapshot_id: None,
        });
        byte_start = byte_end;
        line_start = line_end;
    }

    Ok(chunks)
}

fn file_surface_window_end(content: &str, byte_start: usize) -> usize {
    let mut byte_end = byte_start
        .saturating_add(MAX_SOURCE_SURFACE_CHUNK_BYTES)
        .min(content.len());
    while !content.is_char_boundary(byte_end) {
        byte_end -= 1;
    }
    if let Some((offset, _)) = content[byte_start..byte_end]
        .match_indices('\n')
        .nth(MAX_SOURCE_SURFACE_CHUNK_LINES - 1)
    {
        return byte_start + offset + 1;
    }
    byte_end
}

fn has_uncovered_source_surface(content: &str, symbols: &[RepositoryCodeSymbolRecord]) -> bool {
    let mut ranges = symbols
        .iter()
        .filter_map(|symbol| {
            let start = usize::try_from(symbol.byte_range.start).ok()?;
            let end = usize::try_from(symbol.byte_range.end).ok()?;
            (start < end && end <= content.len()).then_some((start, end))
        })
        .collect::<Vec<_>>();
    ranges.sort_unstable_by_key(|range| range.0);

    let mut covered_end = 0usize;
    for (start, end) in ranges {
        if start > covered_end && contains_source_token(&content[covered_end..start]) {
            return true;
        }
        covered_end = covered_end.max(end);
    }

    covered_end < content.len() && contains_source_token(&content[covered_end..])
}

fn contains_source_token(content: &str) -> bool {
    content
        .chars()
        .any(|character| character.is_alphanumeric() || matches!(character, '_' | '#' | '@'))
}

pub(super) fn add_file_chunk(
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
        content: file_chunk_content(path, language_id, content),
        byte_range: RepositoryCodeRange::new("byte_range", 0, byte_end)
            .map_err(|error| CodeIndexError::InvalidInput(error.to_string()))?,
        line_range: RepositoryCodeRange::new("line_range", 1, line_end)
            .map_err(|error| CodeIndexError::InvalidInput(error.to_string()))?,
        symbol_snapshot_id: None,
    });

    Ok(())
}

fn file_chunk_content(path: &str, language_id: &str, content: &str) -> String {
    if language_id == "markdown" {
        return content.to_owned();
    }
    if keeps_complete_manifest_content(path) {
        content.trim().to_owned()
    } else {
        trim_to_budget(content, MAX_SOURCE_SURFACE_CHUNK_BYTES)
    }
}

fn keeps_complete_manifest_content(path: &str) -> bool {
    path.replace('\\', "/")
        .rsplit('/')
        .next()
        .is_some_and(|name| {
            matches!(
                name,
                "go.mod"
                    | "go.work"
                    | "package.json"
                    | "pnpm-workspace.yaml"
                    | "pnpm-workspace.yml"
            )
        })
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

#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;
