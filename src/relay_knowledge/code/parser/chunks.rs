use crate::domain::{RepositoryCodeChunkRecord, RepositoryCodeRange, RepositoryCodeSymbolRecord};

use super::{
    super::{CodeIndexError, SnapshotBuild, stable_content_hash, stable_id},
    text::count_lines,
};

pub(super) fn chunks_for_symbols(
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
            content: trim_to_budget(excerpt, 8_000),
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
        content: trim_to_budget(content, 8_000),
        byte_range: RepositoryCodeRange::new("byte_range", 0, byte_end)
            .map_err(|error| CodeIndexError::InvalidInput(error.to_string()))?,
        line_range: RepositoryCodeRange::new("line_range", 1, line_end)
            .map_err(|error| CodeIndexError::InvalidInput(error.to_string()))?,
        symbol_snapshot_id: None,
    });

    Ok(())
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
