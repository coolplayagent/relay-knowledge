use super::super::repository_schema::initialize_repository_schema;
use super::*;

#[test]
fn creates_search_read_model_and_defers_fact_indexes_until_requested() {
    let connection = Connection::open_in_memory().expect("database should open");
    initialize_repository_schema(&connection).expect("repository schema should initialize");

    initialize_search_schema(&connection).expect("search schema should initialize");
    initialize_search_schema(&connection).expect("search schema should be idempotent");

    let deferred_index_count: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM sqlite_schema
             WHERE type = 'index' AND name = 'code_repository_symbols_lookup'",
            [],
            |row| row.get(0),
        )
        .expect("deferred index state should be inspectable");
    assert_eq!(deferred_index_count, 0);
    ensure_search_query_indexes(&connection).expect("query indexes should build");
    ensure_search_query_indexes(&connection).expect("query index build should be idempotent");

    connection
        .execute(
            "
            INSERT INTO code_repository_search (
                source_scope, document_kind, record_id, path, language_id, content
            )
            VALUES ('scope', 'symbol', 'symbol-1', 'src/lib.rs', 'rust', 'SearchableThing')
            ",
            [],
        )
        .expect("search row should insert");
    let match_count: i64 = connection
        .query_row(
            "
            SELECT COUNT(*)
            FROM code_repository_search
            WHERE code_repository_search MATCH 'SearchableThing'
            ",
            [],
            |row| row.get(0),
        )
        .expect("FTS row should be searchable");
    assert_eq!(match_count, 1);

    let metadata_index_count: i64 = connection
        .query_row(
            "
            SELECT COUNT(*)
            FROM sqlite_schema
            WHERE type = 'index'
              AND name = 'code_repository_search_metadata_scope_path'
            ",
            [],
            |row| row.get(0),
        )
        .expect("search metadata indexes should be inspectable");
    assert_eq!(metadata_index_count, 1);
    let redundant_scope_kind_index: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM sqlite_schema WHERE type = 'index' AND name = 'code_repository_search_metadata_scope_kind'",
            [],
            |row| row.get(0),
        )
        .expect("redundant metadata index should be inspectable");
    assert_eq!(redundant_scope_kind_index, 0);
    let search_rowid_primary_key: i64 = connection
        .query_row(
            "SELECT pk FROM pragma_table_info('code_repository_search_metadata') WHERE name = 'search_rowid'",
            [],
            |row| row.get(0),
        )
        .expect("metadata rowid primary key should be inspectable");
    assert_eq!(search_rowid_primary_key, 1);
}

#[test]
fn existing_facts_restore_deferred_query_indexes_on_open() {
    let connection = Connection::open_in_memory().expect("database should open");
    initialize_repository_schema(&connection).expect("repository schema should initialize");
    initialize_search_schema(&connection).expect("search schema should initialize");
    connection
        .execute(
            "INSERT INTO code_repositories (
                repository_id, alias, root_path, path_filters_json, language_filters_json,
                state, indexed_file_count, symbol_count, reference_count, chunk_count, stale
             ) VALUES ('repo', 'alias', '/repo', '[]', '[]', 'indexing', 0, 0, 0, 0, 1)",
            [],
        )
        .expect("repository should insert");
    connection
        .execute(
            "INSERT INTO code_repository_files (
                repository_id, source_scope, file_id, path, language_id, blob_hash, byte_len,
                line_count, parse_status, is_generated
             ) VALUES ('repo', 'scope', 'file', 'src/lib.rs', 'rust', 'hash', 1, 1, 'parsed', 0)",
            [],
        )
        .expect("file fact should insert");

    ensure_search_query_indexes_for_existing_facts(&connection)
        .expect("existing facts should restore query indexes");

    let restored_index_count: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM sqlite_schema
             WHERE type = 'index' AND name = 'code_repository_symbols_lookup'",
            [],
            |row| row.get(0),
        )
        .expect("restored index state should be inspectable");
    assert_eq!(restored_index_count, 1);
}
