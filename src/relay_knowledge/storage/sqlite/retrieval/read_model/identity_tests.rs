use rusqlite::{Connection, params};

use super::derived_document_identity_mismatch;

#[test]
fn bm25_hierarchy_suite_matches_length_prefixed_unicode_code_identities() {
    let connection = Connection::open_in_memory().expect("connection should open");
    connection
        .execute_batch(
            "CREATE TABLE evidence (id TEXT PRIMARY KEY, status TEXT NOT NULL);
             CREATE TABLE code_symbols (
                 source_scope TEXT NOT NULL, path TEXT NOT NULL, symbol_id TEXT NOT NULL
             );
             CREATE TABLE code_chunks (
                 source_scope TEXT NOT NULL, path TEXT NOT NULL, chunk_id TEXT NOT NULL
             );
             CREATE TABLE derived_documents (document_id TEXT PRIMARY KEY);",
        )
        .expect("identity schema should initialize");
    let scope = "范围:a";
    let path = "src/模块:b.rs";
    let symbol_id = "符号:c";
    let chunk_id = "片段:d";
    connection
        .execute(
            "INSERT INTO code_symbols VALUES (?1, ?2, ?3)",
            params![scope, path, symbol_id],
        )
        .expect("symbol should insert");
    connection
        .execute(
            "INSERT INTO code_chunks VALUES (?1, ?2, ?3)",
            params![scope, path, chunk_id],
        )
        .expect("chunk should insert");
    for document_id in [
        format!(
            "code:symbol:{}:{scope}:{}:{path}:{}:{symbol_id}",
            scope.len(),
            path.len(),
            symbol_id.len()
        ),
        format!(
            "code:chunk:{}:{scope}:{}:{path}:{}:{chunk_id}",
            scope.len(),
            path.len(),
            chunk_id.len()
        ),
    ] {
        connection
            .execute(
                "INSERT INTO derived_documents VALUES (?1)",
                params![document_id],
            )
            .expect("derived identity should insert");
    }

    assert!(
        !derived_document_identity_mismatch(&connection, "derived_documents")
            .expect("matching identities should inspect")
    );
    connection
        .execute(
            "UPDATE derived_documents SET document_id = 'code:code_symbol:wrong'
             WHERE document_id LIKE 'code:symbol:%'",
            [],
        )
        .expect("symbol identity should drift");
    assert!(
        derived_document_identity_mismatch(&connection, "derived_documents")
            .expect("drifted identities should inspect")
    );
}

#[test]
fn incomplete_empty_bootstrap_sources_do_not_create_false_identity_drift() {
    let connection = rusqlite::Connection::open_in_memory().expect("connection should open");
    connection
        .execute_batch(
            "CREATE TABLE evidence (status TEXT NOT NULL);
             CREATE TABLE graph_semantic_documents (document_id TEXT PRIMARY KEY);",
        )
        .expect("minimal bootstrap schema should create");

    assert!(
        !derived_document_identity_mismatch(&connection, "graph_semantic_documents")
            .expect("incomplete empty authority should be ignored")
    );
}
