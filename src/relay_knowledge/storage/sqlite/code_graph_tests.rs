use super::*;
use crate::domain::{CodeFileFields, CodeReferenceFields};
use crate::storage::{CodeGraphStore, GraphStore, IndexStore};

#[tokio::test]
async fn commits_code_graph_batch_and_marks_indexes_stale() {
    let store = crate::storage::SqliteGraphStore::open_in_memory().expect("store should open");
    let batch = CodeGraphBatch::new(vec![parsed_file("repo", "src/lib.rs", "sym-main")])
        .expect("batch should validate");

    let receipt = store
        .commit_code_graph_batch(batch)
        .await
        .expect("code graph commit should succeed");
    let graph = store.inspect_graph().await.expect("graph should inspect");
    let indexes = store.index_statuses().await.expect("indexes should load");

    assert_eq!(receipt.graph_version, GraphVersion::new(1));
    assert_eq!(receipt.file_count, 1);
    assert_eq!(receipt.symbol_count, 1);
    assert_eq!(graph.code_file_count, 1);
    assert_eq!(graph.code_symbol_count, 1);
    assert_eq!(graph.code_reference_count, 1);
    assert_eq!(graph.code_chunk_count, 1);
    assert_eq!(graph.code_parse_status_counts.parsed, 1);
    assert!(
        indexes
            .iter()
            .all(|status| status.is_stale_for(GraphVersion::new(1)))
    );
}

#[tokio::test]
async fn startup_rebuilds_obsolete_code_tables_without_deleting_graph_data() {
    let path = temp_db_path("obsolete-code-tables");
    let connection = rusqlite::Connection::open(&path).expect("connection should open");
    connection
        .execute_batch(
            "
            CREATE TABLE graph_state (
                id INTEGER PRIMARY KEY CHECK (id = 1),
                graph_version INTEGER NOT NULL
            );
            INSERT INTO graph_state (id, graph_version) VALUES (1, 1);
            CREATE TABLE evidence (
                id TEXT PRIMARY KEY,
                source_scope TEXT NOT NULL,
                content TEXT NOT NULL,
                created_graph_version INTEGER NOT NULL
            );
            INSERT INTO evidence (id, source_scope, content, created_graph_version)
            VALUES ('ev-code-legacy', 'docs', 'Code graph rebuild should not delete graph data', 1);
            CREATE TABLE code_files (
                repository_id TEXT NOT NULL,
                path TEXT NOT NULL,
                blob_hash TEXT NOT NULL,
                PRIMARY KEY (repository_id, path)
            );
            CREATE TABLE code_symbols (
                symbol_snapshot_id TEXT PRIMARY KEY,
                file_id TEXT NOT NULL,
                name TEXT NOT NULL
            );
            INSERT INTO code_files (repository_id, path, blob_hash)
            VALUES ('repo', 'src/lib.rs', 'hash');
            ",
        )
        .expect("obsolete code tables should be created");
    drop(connection);

    let store = crate::storage::SqliteGraphStore::open(&path)
        .expect("store should rebuild obsolete code tables");
    let graph = store.inspect_graph().await.expect("graph should inspect");
    let guard = store.connection.lock().expect("connection should lock");
    let columns = table_columns(&guard, "code_files").expect("columns should read");

    assert_eq!(graph.graph_version, GraphVersion::new(1));
    assert_eq!(graph.evidence_count, 1);
    assert!(columns.iter().any(|column| column == "source_scope"));
    assert!(columns.iter().any(|column| column == "content_hash"));
    assert!(!table_exists(&guard, "code_files_legacy_0").expect("table check should run"));
    assert_eq!(
        guard
            .query_row("SELECT COUNT(*) FROM code_files", [], |row| {
                row.get::<_, u64>(0)
            })
            .expect("code file count should read"),
        0
    );
    drop(guard);
    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn code_queries_are_scoped_and_version_bounded() {
    let store = crate::storage::SqliteGraphStore::open_in_memory().expect("store should open");
    store
        .commit_code_graph_batch(
            CodeGraphBatch::new(vec![parsed_file("repo-a", "src/lib.rs", "sym-a")])
                .expect("batch should validate"),
        )
        .await
        .expect("first commit should succeed");
    store
        .commit_code_graph_batch(
            CodeGraphBatch::new(vec![parsed_file("repo-b", "src/lib.rs", "sym-b")])
                .expect("batch should validate"),
        )
        .await
        .expect("second commit should succeed");

    let first_snapshot = store
        .search_code_symbols(CodeSymbolSearchRequest {
            source_scope: None,
            path: None,
            name: Some("main".to_owned()),
            graph_version: GraphVersion::new(1),
            limit: 10,
        })
        .await
        .expect("symbol search should succeed");
    let scoped = store
        .search_code_chunks(CodeChunkSearchRequest {
            source_scope: Some("repo-b".to_owned()),
            path: Some("src/lib.rs".to_owned()),
            query: Some("main".to_owned()),
            graph_version: GraphVersion::new(2),
            limit: 10,
        })
        .await
        .expect("chunk search should succeed");

    assert_eq!(first_snapshot.len(), 1);
    assert_eq!(first_snapshot[0].source_scope.as_str(), "repo-a");
    assert_eq!(scoped.len(), 1);
    assert_eq!(scoped[0].source_scope.as_str(), "repo-b");
    assert_eq!(scoped[0].linked_symbol_ids, ["sym-b"]);
}

#[tokio::test]
async fn replacing_file_facts_removes_old_symbols() {
    let store = crate::storage::SqliteGraphStore::open_in_memory().expect("store should open");
    let first = parsed_file("repo", "src/lib.rs", "sym-old");
    let second = parsed_file("repo", "src/lib.rs", "sym-new");
    store
        .commit_code_graph_batch(CodeGraphBatch::new(vec![first]).expect("batch"))
        .await
        .expect("first commit should succeed");
    store
        .commit_code_graph_batch(CodeGraphBatch::new(vec![second]).expect("batch"))
        .await
        .expect("second commit should succeed");

    let symbols = store
        .search_code_symbols(CodeSymbolSearchRequest {
            source_scope: Some("repo".to_owned()),
            path: Some("src/lib.rs".to_owned()),
            name: None,
            graph_version: GraphVersion::new(2),
            limit: 10,
        })
        .await
        .expect("symbol search should succeed");

    assert_eq!(symbols.len(), 1);
    assert_eq!(symbols[0].symbol_id, "sym-new");
}

#[tokio::test]
async fn failed_and_partial_files_are_visible_in_parse_counts() {
    let store = crate::storage::SqliteGraphStore::open_in_memory().expect("store should open");
    let failed = CodeFileRecord::new(CodeFileFields {
        source_scope: SourceScope::parse("repo").expect("scope should parse"),
        path: "src/broken.rs".to_owned(),
        content_hash: "hash-failed".to_owned(),
        language_id: "rust".to_owned(),
        parse_status: CodeParseStatus::Failed,
        diagnostic: Some("parser panic isolated".to_owned()),
        symbols: Vec::new(),
        references: Vec::new(),
        chunks: Vec::new(),
    })
    .expect("failed file should validate");
    let partial = CodeFileRecord::new(CodeFileFields {
        source_scope: SourceScope::parse("repo").expect("scope should parse"),
        path: "src/partial.rs".to_owned(),
        content_hash: "hash-partial".to_owned(),
        language_id: "rust".to_owned(),
        parse_status: CodeParseStatus::Partial,
        diagnostic: Some("syntax error node".to_owned()),
        symbols: Vec::new(),
        references: Vec::new(),
        chunks: Vec::new(),
    })
    .expect("partial file should validate");

    store
        .commit_code_graph_batch(CodeGraphBatch::new(vec![failed, partial]).expect("batch"))
        .await
        .expect("commit should succeed");
    let graph = store.inspect_graph().await.expect("graph should inspect");

    assert_eq!(graph.code_file_count, 2);
    assert_eq!(graph.code_parse_status_counts.failed, 1);
    assert_eq!(graph.code_parse_status_counts.partial, 1);
}

#[tokio::test]
async fn reference_search_can_filter_by_target_symbol() {
    let store = crate::storage::SqliteGraphStore::open_in_memory().expect("store should open");
    store
        .commit_code_graph_batch(
            CodeGraphBatch::new(vec![parsed_file("repo", "src/lib.rs", "sym-main")])
                .expect("batch should validate"),
        )
        .await
        .expect("commit should succeed");

    let references = store
        .search_code_references(CodeReferenceSearchRequest {
            source_scope: Some("repo".to_owned()),
            path: None,
            symbol_text: Some("main".to_owned()),
            target_symbol_id: Some("sym-main".to_owned()),
            graph_version: GraphVersion::new(1),
            limit: 5,
        })
        .await
        .expect("reference search should succeed");

    assert_eq!(references.len(), 1);
    assert_eq!(references[0].target_symbol_id.as_deref(), Some("sym-main"));
}

#[tokio::test]
async fn rejects_zero_code_query_limits() {
    let store = crate::storage::SqliteGraphStore::open_in_memory().expect("store should open");

    let error = store
        .search_code_symbols(CodeSymbolSearchRequest {
            source_scope: None,
            path: None,
            name: None,
            graph_version: GraphVersion::ZERO,
            limit: 0,
        })
        .await
        .expect_err("zero limit should fail");

    assert_eq!(
        error.to_string(),
        "invalid storage input: code symbol search limit must be greater than zero"
    );
}

fn parsed_file(scope: &str, path: &str, symbol_id: &str) -> CodeFileRecord {
    let source_scope = SourceScope::parse(scope).expect("scope should parse");
    let extraction = extraction();
    let symbol = CodeSymbolRecord::new(
        symbol_id,
        source_scope.clone(),
        path,
        "main",
        CodeSymbolKind::Function,
        range(0, 12),
        extraction.clone(),
    )
    .expect("symbol should validate");
    let reference = CodeReferenceRecord::new(CodeReferenceFields {
        reference_id: format!("ref-{symbol_id}"),
        source_scope: source_scope.clone(),
        path: path.to_owned(),
        symbol_text: "main".to_owned(),
        kind: CodeReferenceKind::Call,
        range: range(3, 7),
        resolution_state: CodeResolutionState::Resolved,
        target_symbol_id: Some(symbol_id.to_owned()),
        extraction: extraction.clone(),
    })
    .expect("reference should validate");
    let chunk = CodeChunkRecord::new(
        format!("chunk-{symbol_id}"),
        source_scope.clone(),
        path,
        "fn main() {}",
        range(0, 12),
        vec![symbol_id.to_owned()],
        Some(extraction),
    )
    .expect("chunk should validate");

    CodeFileRecord::new(CodeFileFields {
        source_scope,
        path: path.to_owned(),
        content_hash: format!("hash-{symbol_id}"),
        language_id: "rust".to_owned(),
        parse_status: CodeParseStatus::Parsed,
        diagnostic: None,
        symbols: vec![symbol],
        references: vec![reference],
        chunks: vec![chunk],
    })
    .expect("file should validate")
}

fn extraction() -> CodeExtractionMetadata {
    CodeExtractionMetadata::new(
        "tree-sitter-rust@0.23",
        "rust-tags",
        "v1",
        "function_item",
        "definition.function",
    )
    .expect("extraction should validate")
}

fn range(start: u32, end: u32) -> CodeRange {
    CodeRange::new(start, end, 1, 1).expect("range should validate")
}

fn table_columns(
    connection: &rusqlite::Connection,
    table: &str,
) -> Result<Vec<String>, StorageError> {
    let mut statement = connection.prepare(&format!("PRAGMA table_info({table})"))?;
    let rows = statement.query_map([], |row| row.get::<_, String>(1))?;

    rows.collect::<Result<Vec<_>, _>>()
        .map_err(StorageError::from)
}

fn table_exists(connection: &rusqlite::Connection, table: &str) -> Result<bool, StorageError> {
    connection
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1)",
            [table],
            |row| row.get::<_, bool>(0),
        )
        .map_err(StorageError::from)
}

fn temp_db_path(test_name: &str) -> std::path::PathBuf {
    let mut path = std::env::temp_dir();
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("time should be monotonic")
        .as_nanos();
    path.push(format!(
        "relay-knowledge-{test_name}-{}-{unique}.sqlite",
        std::process::id()
    ));
    let _ = std::fs::remove_file(&path);

    path
}
