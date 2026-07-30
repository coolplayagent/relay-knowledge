use super::super::repository_schema::initialize_repository_schema;
use super::*;

#[test]
fn creates_search_read_model_and_lookup_indexes() {
    let connection = Connection::open_in_memory().expect("database should open");
    initialize_repository_schema(&connection).expect("repository schema should initialize");

    initialize_search_schema(&connection).expect("search schema should initialize");
    initialize_search_schema(&connection).expect("search schema should be idempotent");

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
              AND name IN (
                  'code_repository_search_metadata_scope_kind',
                  'code_repository_search_metadata_scope_path'
              )
            ",
            [],
            |row| row.get(0),
        )
        .expect("search metadata indexes should be inspectable");
    assert_eq!(metadata_index_count, 2);
}
