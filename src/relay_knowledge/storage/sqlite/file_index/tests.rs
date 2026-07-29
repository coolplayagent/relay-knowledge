use super::*;
use crate::storage::FileIndexRoot;

#[test]
fn replace_search_and_diagnostics_round_trip() {
    let mut connection = open_connection();
    let first = update(
        vec![
            entry(
                "/workspace/docs/quarterly-design.pdf",
                "docs/quarterly-design.pdf",
                "pdf",
            ),
            entry(
                "/workspace/docs/quarterly-notes.md",
                "docs/quarterly-notes.md",
                "md",
            ),
        ],
        10,
    );
    let status = replace_root(&mut connection, first).expect("root should be indexed");
    assert_eq!(status.indexed_file_count, 2);
    assert_eq!(status.missing_file_count, 0);
    assert_eq!(status.last_indexed_at_ms, Some(10));

    let hits = search(
        &connection,
        FileSearchRequest {
            query: "quarterly design pdf".to_owned(),
            source_scope: Some("local-files".to_owned()),
            root_id: Some("root-a".to_owned()),
            limit: 5,
            timeout_ms: 750,
        },
        deadline(),
    )
    .expect("indexed files should be searchable");
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].rank, 1);
    assert_eq!(hits[0].file_name, "quarterly-design.pdf");
    assert_eq!(hits[0].extension.as_deref(), Some("pdf"));
    assert_eq!(hits[0].status, INDEXED_STATUS);

    let diagnostics = diagnostics(&connection).expect("diagnostics should load");
    assert_eq!(diagnostics.root_count, 1);
    assert_eq!(diagnostics.indexed_file_count, 2);
    assert_eq!(diagnostics.missing_file_count, 0);

    let second = update(
        vec![entry(
            "/workspace/docs/quarterly-design.pdf",
            "docs/quarterly-design.pdf",
            "pdf",
        )],
        20,
    );
    let status = replace_root(&mut connection, second).expect("root should update");
    assert_eq!(status.indexed_file_count, 1);
    assert_eq!(status.missing_file_count, 1);

    let removed = search(
        &connection,
        FileSearchRequest {
            query: "quarterly notes".to_owned(),
            source_scope: Some("local-files".to_owned()),
            root_id: Some("root-a".to_owned()),
            limit: 5,
            timeout_ms: 750,
        },
        deadline(),
    )
    .expect("query should run");
    assert!(removed.is_empty());
}

#[test]
fn failed_scan_preserves_previous_indexed_entries() {
    let mut connection = open_connection();
    replace_root(
        &mut connection,
        update(
            vec![
                entry("/workspace/docs/keep.pdf", "docs/keep.pdf", "pdf"),
                entry("/workspace/docs/older.txt", "docs/older.txt", "txt"),
            ],
            10,
        ),
    )
    .expect("initial root should be indexed");

    let status = replace_root(
        &mut connection,
        FileIndexRootUpdate {
            root: root(),
            entries: vec![entry("/workspace/docs/keep.pdf", "docs/keep.pdf", "pdf")],
            processed_content_paths: BTreeSet::new(),
            content_entries: Vec::new(),
            scan_error_count: 1,
            truncated: false,
            content_truncated: false,
            content_read_error_count: 0,
            last_error: Some("permission denied".to_owned()),
            now_ms: 20,
        },
    )
    .expect("failed scan should update diagnostics");
    assert_eq!(status.indexed_file_count, 2);
    assert_eq!(status.missing_file_count, 0);
    assert_eq!(status.last_error.as_deref(), Some("permission denied"));

    let hits = search(
        &connection,
        FileSearchRequest {
            query: "keep pdf".to_owned(),
            source_scope: Some("local-files".to_owned()),
            root_id: Some("root-a".to_owned()),
            limit: 5,
            timeout_ms: 750,
        },
        deadline(),
    )
    .expect("previous entries should remain searchable");
    assert_eq!(hits.len(), 1);

    let older_hits = search(
        &connection,
        FileSearchRequest {
            query: "older txt".to_owned(),
            source_scope: Some("local-files".to_owned()),
            root_id: Some("root-a".to_owned()),
            limit: 5,
            timeout_ms: 750,
        },
        deadline(),
    )
    .expect("unobserved entries should survive partial scan errors");
    assert_eq!(older_hits.len(), 1);
}

#[test]
fn truncated_scan_preserves_unobserved_entries() {
    let mut connection = open_connection();
    replace_root(
        &mut connection,
        update(
            vec![
                entry("/workspace/docs/first.pdf", "docs/first.pdf", "pdf"),
                entry("/workspace/docs/second.pdf", "docs/second.pdf", "pdf"),
            ],
            10,
        ),
    )
    .expect("initial root should be indexed");

    let status = replace_root(
        &mut connection,
        FileIndexRootUpdate {
            root: root(),
            entries: vec![entry("/workspace/docs/first.pdf", "docs/first.pdf", "pdf")],
            processed_content_paths: BTreeSet::new(),
            content_entries: Vec::new(),
            scan_error_count: 0,
            truncated: true,
            content_truncated: false,
            content_read_error_count: 0,
            last_error: None,
            now_ms: 20,
        },
    )
    .expect("truncated scan should update diagnostics");
    assert_eq!(status.indexed_file_count, 2);
    assert_eq!(status.missing_file_count, 0);
    assert!(status.truncated);

    let hits = search(
        &connection,
        FileSearchRequest {
            query: "second pdf".to_owned(),
            source_scope: Some("local-files".to_owned()),
            root_id: Some("root-a".to_owned()),
            limit: 5,
            timeout_ms: 750,
        },
        deadline(),
    )
    .expect("unobserved entries should survive truncated scans");
    assert_eq!(hits.len(), 1);
}

#[test]
fn unconfigured_roots_are_removed_from_search() {
    let mut connection = open_connection();
    replace_root(
        &mut connection,
        update(
            vec![entry(
                "/workspace/docs/retired.pdf",
                "docs/retired.pdf",
                "pdf",
            )],
            10,
        ),
    )
    .expect("initial root should be indexed");

    let diagnostics = mark_unconfigured_roots(&mut connection, Vec::new(), 20)
        .expect("unconfigured roots should be marked");
    assert_eq!(diagnostics.indexed_file_count, 0);
    assert_eq!(diagnostics.missing_file_count, 1);
    assert_eq!(diagnostics.scan_error_count, 1);

    let hits = search(
        &connection,
        FileSearchRequest {
            query: "retired pdf".to_owned(),
            source_scope: Some("local-files".to_owned()),
            root_id: Some("root-a".to_owned()),
            limit: 5,
            timeout_ms: 750,
        },
        deadline(),
    )
    .expect("query should run");
    assert!(hits.is_empty());
}

#[test]
fn search_validation_and_numeric_boundaries_are_explicit() {
    let connection = open_connection();
    let error = search(
        &connection,
        FileSearchRequest {
            query: "!!!".to_owned(),
            source_scope: None,
            root_id: None,
            limit: 10,
            timeout_ms: 750,
        },
        deadline(),
    )
    .expect_err("query without terms should fail");
    assert!(error.to_string().contains("searchable term"));
    assert!(limit_i64(usize::MAX).is_err());
    assert!(i64_from_u64(u64::MAX).is_err());
    assert!(u64_from_sql(-1).is_err());
    assert_eq!(
        u64_from_sql(42).expect("positive integer should convert"),
        42
    );
}

fn open_connection() -> Connection {
    let connection = Connection::open_in_memory().expect("connection should open");
    initialize_schema(&connection).expect("schema should initialize");
    connection
}

fn update(entries: Vec<FileIndexEntry>, now_ms: u64) -> FileIndexRootUpdate {
    FileIndexRootUpdate {
        root: root(),
        entries,
        processed_content_paths: BTreeSet::new(),
        content_entries: Vec::new(),
        scan_error_count: 0,
        truncated: false,
        content_truncated: false,
        content_read_error_count: 0,
        last_error: None,
        now_ms,
    }
}

fn root() -> FileIndexRoot {
    FileIndexRoot {
        scope_id: "local-files".to_owned(),
        root_id: "root-a".to_owned(),
        root_path: "/workspace".to_owned(),
    }
}

fn deadline() -> Instant {
    Instant::now() + std::time::Duration::from_millis(750)
}

fn entry(path: &str, relative_path: &str, extension: &str) -> FileIndexEntry {
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
