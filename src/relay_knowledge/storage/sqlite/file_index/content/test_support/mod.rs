//! Shared deterministic fixtures for content-owner unit tests.

use std::{
    collections::BTreeSet,
    time::{Duration, Instant},
};

use rusqlite::Connection;

use crate::storage::{
    FileContentChunk, FileContentEntry, FileIndexEntry, FileIndexRoot, StorageError,
};

use super::{
    identity::{entry_key, stable_hash64},
    persistence::{ContentReplacementCounts, ContentReplacementRequest, replace_entries},
    schema::initialize_schema,
};

pub(super) fn open_connection() -> Connection {
    let connection = Connection::open_in_memory().expect("connection should open");
    initialize_schema(&connection).expect("content schema should initialize");
    connection
}

pub(super) fn deadline() -> Instant {
    Instant::now() + Duration::from_millis(750)
}

pub(super) fn entry(path: &str, relative_path: &str, extension: &str) -> FileIndexEntry {
    let file_name = path
        .rsplit('/')
        .next()
        .expect("path should include a file name")
        .to_owned();

    FileIndexEntry {
        scope_id: "local-files".to_owned(),
        root_id: "root-a".to_owned(),
        path: path.to_owned(),
        relative_path: relative_path.to_owned(),
        file_name,
        extension: Some(extension.to_owned()),
        parent_dir: "/workspace/docs".to_owned(),
        size_bytes: 128,
        modified_at_ms: 1,
        fingerprint: "128:1".to_owned(),
    }
}

pub(super) fn observed_keys<const N: usize>(entries: [&FileIndexEntry; N]) -> BTreeSet<String> {
    entries
        .into_iter()
        .map(|entry| entry_key(&entry.scope_id, &entry.root_id, &entry.path))
        .collect()
}

pub(super) fn replace_content(
    connection: &Connection,
    root_id: &str,
    entries_len: usize,
    observed_file_keys: &BTreeSet<String>,
    content_entries: &[FileContentEntry],
    now_ms: u64,
) -> Result<ContentReplacementCounts, StorageError> {
    replace_entries(
        connection,
        ContentReplacementRequest {
            scope_id: "local-files",
            root_id,
            entries_len,
            observed_file_keys,
            processed_content_keys: observed_file_keys,
            content_entries,
            file_scan_completed: true,
            content_scan_completed: true,
            now_ms,
        },
    )
}

pub(super) fn root(scope_id: &str, root_id: &str, root_path: &str) -> FileIndexRoot {
    FileIndexRoot {
        scope_id: scope_id.to_owned(),
        root_id: root_id.to_owned(),
        root_path: root_path.to_owned(),
    }
}

pub(super) fn content_entry(
    entry: &FileIndexEntry,
    content: &str,
    indexed_at_ms: u64,
) -> FileContentEntry {
    FileContentEntry {
        scope_id: entry.scope_id.clone(),
        root_id: entry.root_id.clone(),
        path: entry.path.clone(),
        relative_path: entry.relative_path.clone(),
        fingerprint: entry.fingerprint.clone(),
        content_hash: format!("content:{:016x}", stable_hash64(content.as_bytes())),
        indexed_at_ms,
        graph_version: 5,
        chunks: vec![FileContentChunk {
            chunk_index: 0,
            start_byte: 0,
            end_byte: u32::try_from(content.len()).expect("content fits u32"),
            start_line: 1,
            end_line: u32::try_from(content.lines().count()).expect("line count fits u32"),
            content: content.to_owned(),
        }],
        skipped_reason: None,
    }
}
