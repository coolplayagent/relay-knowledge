//! Direct contracts for bounded symbol persistence and search projection.

use rusqlite::{Connection, Transaction, limits::Limit, params};

use crate::{
    domain::{RepositoryCodeRange, RepositoryCodeSymbolRecord, RouteHandlerRole, SymbolRole},
    storage::StorageError,
};

use super::{
    SYMBOL_INSERT_BATCH_SIZE, SYMBOL_INSERT_BIND_COUNT, insert_records, symbol_role_search_fields,
};

#[test]
fn symbol_role_search_fields_include_every_route_handler_binding() {
    let role = Some(SymbolRole::RouteHandlers {
        routes: vec![
            RouteHandlerRole {
                url: "/items".to_owned(),
                http_method: "get".to_owned(),
            },
            RouteHandlerRole {
                url: "/items".to_owned(),
                http_method: "post".to_owned(),
            },
        ],
    });

    let (kind, urls, methods) = symbol_role_search_fields(&role);

    assert_eq!(kind, "route_handler");
    assert!(urls.contains("/items"));
    assert!(methods.contains("get"));
    assert!(methods.contains("post"));
}

#[test]
fn code_index_persistence_performance_suite_symbol_insert_crosses_the_1024_row_boundary_in_input_order()
 {
    assert_eq!(SYMBOL_INSERT_BIND_COUNT, 17_408);
    let mut connection = symbol_database();
    let records = (0..=SYMBOL_INSERT_BATCH_SIZE)
        .map(symbol)
        .collect::<Vec<_>>();
    let transaction = connection.transaction().expect("transaction should start");

    insert_records(&transaction, &records).expect("symbols should persist");

    let fact_ids = ordered_text_column(
        &transaction,
        "SELECT symbol_snapshot_id FROM code_repository_symbols ORDER BY rowid",
    );
    let search_rows = ordered_search_rows(&transaction);
    let expected_ids = records
        .iter()
        .map(|symbol| symbol.symbol_snapshot_id.clone())
        .collect::<Vec<_>>();
    assert_eq!(fact_ids, expected_ids);
    assert_eq!(search_rows.len(), records.len());
    for ((record_id, content), symbol) in search_rows.iter().zip(&records) {
        assert_eq!(record_id, &symbol.symbol_snapshot_id);
        assert!(content.starts_with(&format!(
            "{} {} {} {} {}",
            symbol.name, symbol.qualified_name, symbol.kind, symbol.signature, symbol.path
        )));
    }
    assert_eq!(row_counts(&transaction), (1_025, 1_025, 1_025));

    transaction.commit().expect("transaction should commit");
}

#[test]
fn insert_records_preserves_route_roles_documentation_and_nulls() {
    let mut connection = symbol_database();
    let mut single_route = symbol(1);
    single_route.doc_comment = Some("Lists every item".to_owned());
    single_route.symbol_role = Some(SymbolRole::RouteHandler {
        url: "/api/items".to_owned(),
        http_method: "get".to_owned(),
    });
    let mut multiple_routes = symbol(2);
    multiple_routes.symbol_role = Some(SymbolRole::RouteHandlers {
        routes: vec![
            RouteHandlerRole {
                url: "/api/items".to_owned(),
                http_method: "post".to_owned(),
            },
            RouteHandlerRole {
                url: "/api/items/{id}".to_owned(),
                http_method: "delete".to_owned(),
            },
        ],
    });
    let plain = symbol(3);
    let transaction = connection.transaction().expect("transaction should start");

    insert_records(
        &transaction,
        &[single_route.clone(), multiple_routes.clone(), plain.clone()],
    )
    .expect("symbols should persist");

    let (doc_comment, role_json): (Option<String>, Option<String>) = transaction
        .query_row(
            "SELECT doc_comment, symbol_role_json FROM code_repository_symbols WHERE symbol_snapshot_id = ?1",
            params![single_route.symbol_snapshot_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("single route should exist");
    assert_eq!(doc_comment.as_deref(), Some("Lists every item"));
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(role_json.as_deref().expect("role should exist"))
            .expect("role should be valid JSON"),
        serde_json::json!({
            "type": "route_handler",
            "url": "/api/items",
            "http_method": "get"
        })
    );

    let multiple_role_json: String = transaction
        .query_row(
            "SELECT symbol_role_json FROM code_repository_symbols WHERE symbol_snapshot_id = ?1",
            params![multiple_routes.symbol_snapshot_id],
            |row| row.get(0),
        )
        .expect("multiple routes should exist");
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&multiple_role_json)
            .expect("role should be valid JSON"),
        serde_json::json!({
            "type": "route_handlers",
            "routes": [
                {"url": "/api/items", "http_method": "post"},
                {"url": "/api/items/{id}", "http_method": "delete"}
            ]
        })
    );

    let (plain_comment, plain_role): (Option<String>, Option<String>) = transaction
        .query_row(
            "SELECT doc_comment, symbol_role_json FROM code_repository_symbols WHERE symbol_snapshot_id = ?1",
            params![plain.symbol_snapshot_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("plain symbol should exist");
    assert_eq!(plain_comment, None);
    assert_eq!(plain_role, None);

    let single_content = search_content(&transaction, &single_route.symbol_snapshot_id);
    assert!(single_content.contains("Lists every item"));
    assert!(single_content.contains("route_handler /api/items get"));
    let multiple_content = search_content(&transaction, &multiple_routes.symbol_snapshot_id);
    assert!(multiple_content.contains("route_handler /api/items /api/items/{id} post delete"));
    let plain_content = search_content(&transaction, &plain.symbol_snapshot_id);
    assert!(!plain_content.contains("route_handler"));

    transaction.commit().expect("transaction should commit");
}

#[test]
fn caller_rollback_removes_symbol_facts_and_search_documents_together() {
    let mut connection = symbol_database();
    let transaction = connection.transaction().expect("transaction should start");
    insert_records(&transaction, &[symbol(1), symbol(2)])
        .expect("symbols should persist inside transaction");
    assert_eq!(row_counts(&transaction), (2, 2, 2));

    transaction
        .rollback()
        .expect("transaction should roll back");

    assert_eq!(row_counts(&connection), (0, 0, 0));
}

#[test]
fn second_group_failure_remains_rollback_safe() {
    let mut connection = symbol_database();
    let mut records = (0..=SYMBOL_INSERT_BATCH_SIZE)
        .map(symbol)
        .collect::<Vec<_>>();
    let duplicate_symbol_id = records[0].symbol_snapshot_id.clone();
    records[SYMBOL_INSERT_BATCH_SIZE].symbol_snapshot_id = duplicate_symbol_id;
    let transaction = connection.transaction().expect("transaction should start");

    insert_records(&transaction, &records).expect_err("duplicate symbol should fail");

    assert_eq!(row_counts(&transaction), (1_024, 0, 0));
    transaction
        .rollback()
        .expect("transaction should roll back");
    assert_eq!(row_counts(&connection), (0, 0, 0));
}

#[test]
fn insert_records_clamps_fact_groups_to_the_runtime_variable_limit() {
    let mut connection = symbol_database();
    connection.set_limit(Limit::SQLITE_LIMIT_VARIABLE_NUMBER, 34);
    let transaction = connection.transaction().expect("transaction should start");

    insert_records(&transaction, &(0..5).map(symbol).collect::<Vec<_>>())
        .expect("two rows may use the exact low variable limit");

    assert_eq!(row_counts(&transaction), (5, 5, 5));
    transaction.commit().expect("transaction should commit");
}

#[test]
fn one_symbol_row_may_use_the_exact_sqlite_variable_limit() {
    let mut exact_connection = symbol_database();
    exact_connection.set_limit(Limit::SQLITE_LIMIT_VARIABLE_NUMBER, 17);
    let exact_transaction = exact_connection
        .transaction()
        .expect("transaction should start");
    insert_records(&exact_transaction, &[symbol(1)])
        .expect("the maximum host-parameter index is inclusive");
    exact_transaction
        .commit()
        .expect("transaction should commit");

    let mut short_connection = symbol_database();
    short_connection.set_limit(Limit::SQLITE_LIMIT_VARIABLE_NUMBER, 16);
    let short_transaction = short_connection
        .transaction()
        .expect("transaction should start");
    let error = insert_records(&short_transaction, &[symbol(1)])
        .expect_err("fewer variables than one row requires must fail closed");
    assert!(
        matches!(error, StorageError::Invariant(message) if message.contains("17-column symbol row"))
    );
    assert_eq!(row_counts(&short_transaction), (0, 0, 0));
    short_transaction
        .rollback()
        .expect("transaction should roll back");
}

fn symbol(index: usize) -> RepositoryCodeSymbolRecord {
    let offset = u32::try_from(index).expect("fixture index should fit u32");
    RepositoryCodeSymbolRecord {
        repository_id: "repo".to_owned(),
        source_scope: "scope".to_owned(),
        symbol_snapshot_id: format!("symbol-{index}"),
        canonical_symbol_id: format!("repo://repo/src/lib.rs::handler_{index}"),
        file_id: "file".to_owned(),
        path: "src/lib.rs".to_owned(),
        language_id: "rust".to_owned(),
        name: format!("handler_{index}"),
        qualified_name: format!("crate::handler_{index}"),
        kind: "function".to_owned(),
        signature: format!("fn handler_{index}()"),
        doc_comment: None,
        byte_range: RepositoryCodeRange {
            start: offset,
            end: offset + 1,
        },
        line_range: RepositoryCodeRange {
            start: offset + 1,
            end: offset + 1,
        },
        symbol_role: None,
    }
}

fn symbol_database() -> Connection {
    let connection = Connection::open_in_memory().expect("database should open");
    connection
        .execute_batch(
            "
            CREATE TABLE code_repository_symbols (
                repository_id TEXT NOT NULL,
                source_scope TEXT NOT NULL,
                symbol_snapshot_id TEXT NOT NULL,
                canonical_symbol_id TEXT NOT NULL,
                file_id TEXT NOT NULL,
                path TEXT NOT NULL,
                language_id TEXT NOT NULL,
                name TEXT NOT NULL,
                qualified_name TEXT NOT NULL,
                kind TEXT NOT NULL,
                signature TEXT NOT NULL,
                doc_comment TEXT,
                byte_start INTEGER NOT NULL,
                byte_end INTEGER NOT NULL,
                line_start INTEGER NOT NULL,
                line_end INTEGER NOT NULL,
                symbol_role_json TEXT,
                PRIMARY KEY (source_scope, symbol_snapshot_id)
            );
            CREATE VIRTUAL TABLE code_repository_search USING fts5(
                source_scope UNINDEXED,
                document_kind UNINDEXED,
                record_id UNINDEXED,
                path UNINDEXED,
                language_id UNINDEXED,
                content
            );
            CREATE TABLE code_repository_search_metadata (
                source_scope TEXT NOT NULL,
                document_kind TEXT NOT NULL,
                record_id TEXT NOT NULL,
                path TEXT NOT NULL,
                search_rowid INTEGER NOT NULL UNIQUE,
                PRIMARY KEY (source_scope, document_kind, record_id)
            );
            ",
        )
        .expect("symbol schema should be created");
    connection
}

fn ordered_text_column(transaction: &Transaction<'_>, sql: &str) -> Vec<String> {
    let mut statement = transaction.prepare(sql).expect("query should prepare");
    statement
        .query_map([], |row| row.get(0))
        .expect("rows should query")
        .collect::<rusqlite::Result<Vec<_>>>()
        .expect("text rows should map")
}

fn ordered_search_rows(transaction: &Transaction<'_>) -> Vec<(String, String)> {
    let mut statement = transaction
        .prepare(
            "SELECT record_id, content FROM code_repository_search WHERE document_kind = 'symbol' ORDER BY rowid",
        )
        .expect("search query should prepare");
    statement
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
        .expect("search rows should query")
        .collect::<rusqlite::Result<Vec<_>>>()
        .expect("search rows should map")
}

fn search_content(transaction: &Transaction<'_>, record_id: &str) -> String {
    transaction
        .query_row(
            "SELECT content FROM code_repository_search WHERE document_kind = 'symbol' AND record_id = ?1",
            params![record_id],
            |row| row.get(0),
        )
        .expect("search content should exist")
}

fn row_counts(connection: &Connection) -> (i64, i64, i64) {
    (
        connection
            .query_row("SELECT count(*) FROM code_repository_symbols", [], |row| {
                row.get(0)
            })
            .expect("symbol rows should count"),
        connection
            .query_row("SELECT count(*) FROM code_repository_search", [], |row| {
                row.get(0)
            })
            .expect("search rows should count"),
        connection
            .query_row(
                "SELECT count(*) FROM code_repository_search_metadata",
                [],
                |row| row.get(0),
            )
            .expect("metadata rows should count"),
    )
}
