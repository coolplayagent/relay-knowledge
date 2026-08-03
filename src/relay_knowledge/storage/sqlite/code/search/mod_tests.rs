//! Direct contracts for code search-document content.

use rusqlite::{Connection, params};

use super::{
    SEARCH_DOCUMENT_INSERT_BATCH_SIZE, SearchDocumentInserter, search_document_content,
    search_document_content_into,
};

#[test]
fn symbol_search_content_preserves_identifier_expansion() {
    let content = search_document_content(
        "symbol",
        [
            "NewLRUCache",
            "",
            "leveldb::NewLRUCache",
            "function",
            "db/cache.cc",
        ],
    );

    assert_eq!(
        content,
        "NewLRUCache leveldb::NewLRUCache function db/cache.cc cache leveldb lru new newlrucache"
    );
}

#[test]
fn route_search_content_expands_handler_identifier_terms() {
    let content = search_document_content(
        "route",
        [
            "route endpoint http",
            "/api/users",
            "get",
            "listUsers",
            "express",
            "src/routes.ts",
        ],
    );

    assert_eq!(
        content,
        "route endpoint http /api/users get listUsers express src/routes.ts list listusers users"
    );
}

#[test]
fn non_symbol_search_content_keeps_only_nonempty_fields() {
    let content = search_document_content("chunk", ["", "body text", "  ", "src/lib.rs"]);

    assert_eq!(content, "body text src/lib.rs");
}

#[test]
fn reusable_search_content_buffers_do_not_leak_previous_terms() {
    let mut content = String::from("stale content");
    let mut symbol_terms = vec!["stale".to_owned()];
    search_document_content_into(
        &mut content,
        &mut symbol_terms,
        "symbol",
        ["GraphIndex", "relay_knowledge::GraphIndex"],
    );
    assert_eq!(
        content,
        "GraphIndex relay_knowledge::GraphIndex graph graphindex index knowledge relay relay_knowledge"
    );

    search_document_content_into(&mut content, &mut symbol_terms, "chunk", ["new chunk"]);
    assert_eq!(content, "new chunk");
    assert!(symbol_terms.is_empty());
}

#[test]
fn buffered_search_inserts_keep_fts_and_metadata_in_lockstep() {
    let mut connection = Connection::open_in_memory().expect("database should open");
    super::super::schema::initialize_code_schema(&connection)
        .expect("code schema should initialize");
    let transaction = connection.transaction().expect("transaction should start");
    let mut inserter = SearchDocumentInserter::new(&transaction).expect("inserter should build");
    for index in 0..=SEARCH_DOCUMENT_INSERT_BATCH_SIZE {
        let record_id = format!("symbol-{index}");
        let symbol = format!("GraphIndex{index}");
        inserter
            .insert(
                "scope",
                "symbol",
                &record_id,
                "src/lib.rs",
                "rust",
                [symbol.as_str(), "relay_knowledge::GraphIndex"],
            )
            .expect("search document should buffer");
    }
    inserter.finish().expect("remaining documents should flush");
    transaction.commit().expect("transaction should commit");

    let expected = (SEARCH_DOCUMENT_INSERT_BATCH_SIZE + 1) as i64;
    let search_count: i64 = connection
        .query_row(
            "SELECT count(*) FROM code_repository_search WHERE source_scope = 'scope'",
            [],
            |row| row.get(0),
        )
        .expect("search rows should count");
    let metadata_count: i64 = connection
        .query_row(
            "SELECT count(*) FROM code_repository_search_metadata WHERE source_scope = 'scope'",
            [],
            |row| row.get(0),
        )
        .expect("metadata rows should count");
    let matched: i64 = connection
        .query_row(
            "SELECT count(*) FROM code_repository_search WHERE code_repository_search MATCH ?1",
            params!["graph"],
            |row| row.get(0),
        )
        .expect("expanded identifier terms should remain searchable");

    assert_eq!(search_count, expected);
    assert_eq!(metadata_count, expected);
    assert_eq!(matched, expected);
}
