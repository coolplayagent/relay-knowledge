use std::path::Path;

use crate::{
    identity::stable_hash64,
    storage::{FileContentChunk, FileContentEntry, FileIndexEntry},
};

use super::read::{self, MAX_CONTENT_INDEX_BYTES};
const MAX_CONTENT_CHUNK_BYTES: usize = 4096;
const MAX_CONTENT_CHUNK_HARD_BYTES: usize = MAX_CONTENT_CHUNK_BYTES * 2;

#[derive(Debug)]
pub(in crate::application::knowledge::file_index) enum FileContentEntryResult {
    Indexed(Box<FileContentEntry>),
    Skipped,
    ReadFailed,
}

pub(in crate::application::knowledge::file_index) fn file_content_entry(
    entry: &FileIndexEntry,
    metadata: &std::fs::Metadata,
    canonical_root: &Path,
    indexed_at_ms: u64,
    graph_version: u64,
) -> FileContentEntryResult {
    if metadata.len() > MAX_CONTENT_INDEX_BYTES
        || !text_content_extension(entry.extension.as_deref())
    {
        return FileContentEntryResult::Skipped;
    }
    let content =
        read::read_authorized_text_content(Path::new(&entry.path), metadata, canonical_root);
    let Some(content) = content else {
        return FileContentEntryResult::ReadFailed;
    };
    if content.trim().is_empty() {
        return FileContentEntryResult::Skipped;
    }
    let chunks = content_chunks(&content);
    if chunks.is_empty() {
        return FileContentEntryResult::Skipped;
    }

    FileContentEntryResult::Indexed(Box::new(FileContentEntry {
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
    }))
}

pub(in crate::application::knowledge::file_index) fn text_content_extension(
    extension: Option<&str>,
) -> bool {
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
        let character_line = current_line;
        let next = index.saturating_add(character.len_utf8());
        if is_content_chunk_boundary(character) && next > chunk_start {
            last_boundary = Some((next, character_line));
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
        if character == '\n' {
            current_line = current_line.saturating_add(1);
            if chunk_start == next {
                chunk_start_line = current_line;
            }
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

#[cfg(test)]
#[path = "extract_tests.rs"]
mod tests;
