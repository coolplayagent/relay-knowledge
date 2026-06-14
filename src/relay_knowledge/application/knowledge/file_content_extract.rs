use std::path::Path;

use crate::storage::{FileContentChunk, FileContentEntry, FileIndexEntry};

use super::file_content_read::{self, MAX_CONTENT_INDEX_BYTES};
const MAX_CONTENT_CHUNK_BYTES: usize = 4096;
const MAX_CONTENT_CHUNK_HARD_BYTES: usize = MAX_CONTENT_CHUNK_BYTES * 2;

pub(super) fn file_content_entry(
    entry: &FileIndexEntry,
    metadata: &std::fs::Metadata,
    canonical_root: &Path,
    indexed_at_ms: u64,
    graph_version: u64,
) -> Option<FileContentEntry> {
    if metadata.len() > MAX_CONTENT_INDEX_BYTES
        || !text_content_extension(entry.extension.as_deref())
    {
        return None;
    }
    let content = file_content_read::read_authorized_text_content(
        Path::new(&entry.path),
        metadata,
        canonical_root,
    )?;
    if content.trim().is_empty() {
        return None;
    }
    let chunks = content_chunks(&content);
    if chunks.is_empty() {
        return None;
    }

    Some(FileContentEntry {
        scope_id: entry.scope_id.clone(),
        root_id: entry.root_id.clone(),
        path: entry.path.clone(),
        relative_path: entry.relative_path.clone(),
        fingerprint: entry.fingerprint.clone(),
        content_hash: format!("content:{:016x}", stable_hash64(content.as_bytes())),
        indexed_at_ms,
        graph_version,
        chunks,
        skipped_reason: None,
    })
}

pub(super) fn text_content_extension(extension: Option<&str>) -> bool {
    matches!(
        extension.unwrap_or_default(),
        "md" | "markdown"
            | "txt"
            | "text"
            | "yaml"
            | "yml"
            | "json"
            | "sql"
            | "toml"
            | "csv"
            | "ini"
            | "conf"
            | "xml"
    )
}

fn content_chunks(content: &str) -> Vec<FileContentChunk> {
    let mut chunks = Vec::new();
    let mut chunk_start = 0usize;
    let mut chunk_start_line = 1u32;
    let mut current_line = 1u32;
    let mut last_boundary = None;

    for (index, character) in content.char_indices() {
        if character == '\n' {
            current_line = current_line.saturating_add(1);
        }
        let next = index.saturating_add(character.len_utf8());
        if is_content_chunk_boundary(character) && next > chunk_start {
            last_boundary = Some((next, current_line));
        }

        let chunk_bytes = next.saturating_sub(chunk_start);
        if chunk_bytes >= MAX_CONTENT_CHUNK_BYTES {
            let Some((chunk_end, chunk_end_line)) =
                content_chunk_end(chunk_bytes, last_boundary, next, current_line)
            else {
                continue;
            };
            push_content_chunk(
                &mut chunks,
                content,
                chunk_start,
                chunk_end,
                chunk_start_line,
                chunk_end_line,
            );
            chunk_start = chunk_end;
            chunk_start_line = chunk_end_line;
            last_boundary = None;
        }
    }
    if chunk_start < content.len() {
        push_content_chunk(
            &mut chunks,
            content,
            chunk_start,
            content.len(),
            chunk_start_line,
            current_line,
        );
    }

    chunks
}

fn content_chunk_end(
    chunk_bytes: usize,
    last_boundary: Option<(usize, u32)>,
    fallback_end: usize,
    fallback_line: u32,
) -> Option<(usize, u32)> {
    if let Some(boundary) = last_boundary {
        return Some(boundary);
    }
    if chunk_bytes >= MAX_CONTENT_CHUNK_HARD_BYTES {
        return Some((fallback_end, fallback_line));
    }

    None
}

fn is_content_chunk_boundary(character: char) -> bool {
    !character.is_alphanumeric() && character != '_'
}

fn push_content_chunk(
    chunks: &mut Vec<FileContentChunk>,
    content: &str,
    start: usize,
    end: usize,
    start_line: u32,
    end_line: u32,
) {
    let Some(start_byte) = u32::try_from(start).ok() else {
        return;
    };
    let Some(end_byte) = u32::try_from(end).ok() else {
        return;
    };
    let text = &content[start..end];
    if text.trim().is_empty() || end_byte <= start_byte {
        return;
    }
    chunks.push(FileContentChunk {
        chunk_index: chunks.len(),
        start_byte,
        end_byte,
        start_line,
        end_line,
        content: text.to_owned(),
    });
}

fn stable_hash64(bytes: &[u8]) -> u64 {
    const FNV_OFFSET_BASIS: u64 = 0xcbf29ce484222325;
    const FNV_PRIME: u64 = 0x100000001b3;

    let mut hash = FNV_OFFSET_BASIS;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(FNV_PRIME);
    }

    hash
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn content_chunks_keep_query_tokens_in_one_chunk() {
        let filler = "x ".repeat((MAX_CONTENT_CHUNK_BYTES - 12) / 2);
        let token = "TOKEN_SAFE_IDENTIFIER_that_crosses_the_soft_chunk_boundary";
        let content = format!("{filler}{token} tail");

        let chunks = content_chunks(&content);
        let token_chunks = chunks
            .iter()
            .filter(|chunk| chunk.content.contains(token))
            .collect::<Vec<_>>();

        assert_eq!(token_chunks.len(), 1);
        assert!(chunks[0].end_byte as usize <= filler.len());
        assert!(token_chunks[0].start_byte as usize >= filler.len());
    }
}
