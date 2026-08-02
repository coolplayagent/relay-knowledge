//! Direct replacement, partial-scan, and cursor freshness contracts.

use super::*;
use crate::storage::FileContentSearchRequest;

use super::super::{
    search::search,
    test_support::{
        content_entry, deadline, entry, observed_keys, open_connection, replace_content, root,
    },
};

#[test]
fn unchanged_content_preserves_fresh_cursors() {
    let connection = open_connection();
    let mut text_entry = entry("/workspace/docs/stable.md", "docs/stable.md", "md");
    replace_content(
        &connection,
        "root-a",
        1,
        &observed_keys([&text_entry]),
        &[content_entry(&text_entry, "stable content", 10)],
        10,
    )
    .expect("initial content root should be indexed");

    text_entry.fingerprint = "128:20".to_owned();
    let counts = replace_content(
        &connection,
        "root-a",
        1,
        &observed_keys([&text_entry]),
        &[content_entry(&text_entry, "stable content", 20)],
        20,
    )
    .expect("unchanged content root should update");

    assert_eq!(counts.indexed_content_count, 1);
    assert_eq!(counts.unchanged_content_count, 1);
    assert_eq!(counts.stale_content_cursor_count, 0);

    let cursors = cursors(&connection).expect("cursors should load");
    assert_eq!(cursors.len(), 3);
    let bm25 = cursors
        .iter()
        .find(|cursor| cursor.kind == crate::domain::IndexKind::Bm25)
        .expect("BM25 cursor should be present");
    assert_eq!(bm25.state, crate::domain::IndexState::Fresh);
    assert_eq!(bm25.stale_reason, None);
    assert_eq!(bm25.indexed_graph_version, 5);
    assert!(cursors.iter().all(|cursor| {
        cursor.kind == crate::domain::IndexKind::Bm25
            || (cursor.state == crate::domain::IndexState::Paused
                && cursor
                    .stale_reason
                    .as_deref()
                    .is_some_and(|reason| reason.contains("read model is not built")))
    }));

    let hits = search(
        &connection,
        FileContentSearchRequest {
            query: "stable".to_owned(),
            source_scope: Some("local-files".to_owned()),
            root_id: Some("root-a".to_owned()),
            authorized_roots: vec![root("local-files", "root-a", "/workspace")],
            limit: 5,
            timeout_ms: 750,
        },
        deadline(),
    )
    .expect("content query should run");
    assert_eq!(hits[0].fingerprint, "128:20");
}

#[test]
fn partial_scan_retires_processed_skips_and_preserves_unprocessed_content() {
    let connection = open_connection();
    let skipped = entry("/workspace/docs/skipped.md", "docs/skipped.md", "md");
    let unprocessed = entry(
        "/workspace/docs/unprocessed.md",
        "docs/unprocessed.md",
        "md",
    );
    let observed = observed_keys([&skipped, &unprocessed]);
    replace_content(
        &connection,
        "root-a",
        2,
        &observed,
        &[
            content_entry(&skipped, "old skipped database content", 10),
            content_entry(&unprocessed, "preserved overflow database content", 10),
        ],
        10,
    )
    .expect("initial content should index");

    let processed = observed_keys([&skipped]);
    replace_entries(
        &connection,
        ContentReplacementRequest {
            scope_id: "local-files",
            root_id: "root-a",
            entries_len: 2,
            observed_file_keys: &observed,
            processed_content_keys: &processed,
            content_entries: &[],
            file_scan_completed: true,
            content_scan_completed: false,
            now_ms: 20,
        },
    )
    .expect("partial content replacement should update");

    let skipped_hits = search(
        &connection,
        FileContentSearchRequest {
            query: "skipped".to_owned(),
            source_scope: Some("local-files".to_owned()),
            root_id: Some("root-a".to_owned()),
            authorized_roots: vec![root("local-files", "root-a", "/workspace")],
            limit: 5,
            timeout_ms: 750,
        },
        deadline(),
    )
    .expect("content query should run");
    assert!(skipped_hits.is_empty());

    let preserved_hits = search(
        &connection,
        FileContentSearchRequest {
            query: "preserved".to_owned(),
            source_scope: Some("local-files".to_owned()),
            root_id: Some("root-a".to_owned()),
            authorized_roots: vec![root("local-files", "root-a", "/workspace")],
            limit: 5,
            timeout_ms: 750,
        },
        deadline(),
    )
    .expect("content query should run");
    assert_eq!(preserved_hits.len(), 1);
    assert_eq!(preserved_hits[0].path, "/workspace/docs/unprocessed.md");
}
