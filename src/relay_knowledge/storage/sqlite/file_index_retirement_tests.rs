use std::{collections::BTreeSet, time::Instant};

use rusqlite::Connection;

use crate::storage::{
    FileContentChunk, FileContentEntry, FileContentSearchRequest, FileIndexEntry, FileIndexRoot,
    FileIndexRootUpdate, FileSearchRequest,
};

use super::{
    super::file_index_content::search as search_content, diagnostics, initialize_schema,
    mark_unconfigured_roots, replace_root, search,
};

#[test]
fn retiring_roots_clears_content_diagnostics() {
    let mut connection = open_connection();
    replace_root(
        &mut connection,
        FileIndexRootUpdate {
            root: root(),
            entries: vec![entry("/workspace/docs/retired.md", "docs/retired.md")],
            processed_content_paths: processed_paths(["/workspace/docs/retired.md"]),
            content_entries: vec![content_entry(
                "/workspace/docs/retired.md",
                "docs/retired.md",
                "retired content",
                10,
            )],
            scan_error_count: 0,
            truncated: false,
            content_truncated: false,
            last_error: None,
            now_ms: 10,
        },
    )
    .expect("root with content should be indexed");

    let initial = diagnostics(&connection).expect("diagnostics should load");
    assert_eq!(initial.indexed_content_count, 1);
    assert!(initial.stale_content_cursor_count > 0);
    assert!(!initial.content_cursors.is_empty());

    let retired = mark_unconfigured_roots(&mut connection, Vec::new(), 20)
        .expect("unconfigured roots should be marked");
    assert_eq!(retired.indexed_content_count, 0);
    assert_eq!(retired.skipped_content_count, 0);
    assert_eq!(retired.unchanged_content_count, 0);
    assert_eq!(retired.stale_content_cursor_count, 0);

    let refreshed = diagnostics(&connection).expect("diagnostics should reload");
    assert_eq!(refreshed.indexed_content_count, 0);
    assert_eq!(refreshed.stale_content_cursor_count, 0);
    assert!(refreshed.content_cursors.is_empty());
}

#[test]
fn content_truncated_scan_still_retires_unobserved_file_paths() {
    let mut connection = open_connection();
    replace_root(
        &mut connection,
        FileIndexRootUpdate {
            root: root(),
            entries: vec![
                entry("/workspace/docs/keep.md", "docs/keep.md"),
                entry("/workspace/docs/deleted.md", "docs/deleted.md"),
            ],
            processed_content_paths: processed_paths([
                "/workspace/docs/keep.md",
                "/workspace/docs/deleted.md",
            ]),
            content_entries: vec![
                content_entry(
                    "/workspace/docs/keep.md",
                    "docs/keep.md",
                    "keep content",
                    10,
                ),
                content_entry(
                    "/workspace/docs/deleted.md",
                    "docs/deleted.md",
                    "deleted content",
                    10,
                ),
            ],
            scan_error_count: 0,
            truncated: false,
            content_truncated: false,
            last_error: None,
            now_ms: 10,
        },
    )
    .expect("initial root should be indexed");

    let status = replace_root(
        &mut connection,
        FileIndexRootUpdate {
            root: root(),
            entries: vec![entry("/workspace/docs/keep.md", "docs/keep.md")],
            processed_content_paths: BTreeSet::new(),
            content_entries: Vec::new(),
            scan_error_count: 0,
            truncated: false,
            content_truncated: true,
            last_error: Some("file content scan byte budget exceeded".to_owned()),
            now_ms: 20,
        },
    )
    .expect("content-truncated root should update");
    assert!(!status.truncated);
    assert!(status.content_truncated);
    assert_eq!(status.missing_file_count, 1);

    let deleted_hits = search(
        &connection,
        FileSearchRequest {
            query: "deleted".to_owned(),
            source_scope: Some("local-files".to_owned()),
            root_id: Some("root-a".to_owned()),
            limit: 5,
            timeout_ms: 750,
        },
        deadline(),
    )
    .expect("search should run");
    assert!(deleted_hits.is_empty());

    let deleted_content_hits = search_content(
        &connection,
        FileContentSearchRequest {
            query: "deleted".to_owned(),
            source_scope: Some("local-files".to_owned()),
            root_id: Some("root-a".to_owned()),
            authorized_roots: vec![root()],
            limit: 5,
            timeout_ms: 750,
        },
        deadline(),
    )
    .expect("content search should run");
    assert!(deleted_content_hits.is_empty());
}

#[test]
fn full_content_scan_retires_cursors_for_missing_files() {
    let mut connection = open_connection();
    replace_root(
        &mut connection,
        FileIndexRootUpdate {
            root: root(),
            entries: vec![
                entry("/workspace/docs/keep.md", "docs/keep.md"),
                entry("/workspace/docs/deleted.md", "docs/deleted.md"),
            ],
            processed_content_paths: processed_paths([
                "/workspace/docs/keep.md",
                "/workspace/docs/deleted.md",
            ]),
            content_entries: vec![
                content_entry(
                    "/workspace/docs/keep.md",
                    "docs/keep.md",
                    "keep content",
                    10,
                ),
                content_entry(
                    "/workspace/docs/deleted.md",
                    "docs/deleted.md",
                    "deleted content",
                    10,
                ),
            ],
            scan_error_count: 0,
            truncated: false,
            content_truncated: false,
            last_error: None,
            now_ms: 10,
        },
    )
    .expect("initial content should be indexed");
    let initial = diagnostics(&connection).expect("diagnostics should load");
    assert!(
        initial
            .content_cursors
            .iter()
            .any(|cursor| cursor.path == "/workspace/docs/deleted.md")
    );

    replace_root(
        &mut connection,
        FileIndexRootUpdate {
            root: root(),
            entries: vec![entry("/workspace/docs/keep.md", "docs/keep.md")],
            processed_content_paths: processed_paths(["/workspace/docs/keep.md"]),
            content_entries: vec![content_entry(
                "/workspace/docs/keep.md",
                "docs/keep.md",
                "keep content",
                10,
            )],
            scan_error_count: 0,
            truncated: false,
            content_truncated: false,
            last_error: None,
            now_ms: 20,
        },
    )
    .expect("full rescan should retire missing content");

    let refreshed = diagnostics(&connection).expect("diagnostics should reload");
    assert!(
        refreshed
            .content_cursors
            .iter()
            .all(|cursor| cursor.path != "/workspace/docs/deleted.md")
    );
    assert_eq!(refreshed.stale_content_cursor_count, 3);
}

fn open_connection() -> Connection {
    let connection = Connection::open_in_memory().expect("connection should open");
    initialize_schema(&connection).expect("schema should initialize");
    connection
}

fn deadline() -> Instant {
    Instant::now() + std::time::Duration::from_millis(750)
}

fn root() -> FileIndexRoot {
    FileIndexRoot {
        scope_id: "local-files".to_owned(),
        root_id: "root-a".to_owned(),
        root_path: "/workspace".to_owned(),
    }
}

fn entry(path: &str, relative_path: &str) -> FileIndexEntry {
    FileIndexEntry {
        scope_id: "local-files".to_owned(),
        root_id: "root-a".to_owned(),
        path: path.to_owned(),
        relative_path: relative_path.to_owned(),
        file_name: path.rsplit('/').next().expect("file name").to_owned(),
        extension: Some("md".to_owned()),
        parent_dir: "/workspace/docs".to_owned(),
        size_bytes: 128,
        modified_at_ms: 1,
        fingerprint: "128:1".to_owned(),
    }
}

fn content_entry(path: &str, relative_path: &str, content: &str, now_ms: u64) -> FileContentEntry {
    FileContentEntry {
        scope_id: "local-files".to_owned(),
        root_id: "root-a".to_owned(),
        path: path.to_owned(),
        relative_path: relative_path.to_owned(),
        fingerprint: "128:1".to_owned(),
        content_hash: format!("content:{now_ms}"),
        indexed_at_ms: now_ms,
        graph_version: 1,
        chunks: vec![FileContentChunk {
            chunk_index: 0,
            start_byte: 0,
            end_byte: u32::try_from(content.len()).expect("content fits u32"),
            start_line: 1,
            end_line: 1,
            content: content.to_owned(),
        }],
        skipped_reason: None,
    }
}

fn processed_paths<const N: usize>(paths: [&str; N]) -> BTreeSet<String> {
    paths.into_iter().map(str::to_owned).collect()
}
