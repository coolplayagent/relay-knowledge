use rusqlite::{Connection, params};

use super::*;

#[test]
fn fact_evidence_backfill_links_only_existing_evidence() {
    let connection = fact_schema();
    connection
        .execute(
            "INSERT INTO evidence (id) VALUES (?1)",
            params!["ev-existing"],
        )
        .expect("evidence should insert");
    connection
        .execute(
            "INSERT INTO graph_relations (id, evidence_ids_json) VALUES (?1, ?2)",
            params!["rel", r#"["ev-existing","ev-missing"]"#],
        )
        .expect("relation should insert");

    backfill_fact_evidence_kind(&connection, "relation", "graph_relations")
        .expect("backfill should succeed");

    let linked = connection
        .query_row(
            "SELECT group_concat(evidence_id, ',') FROM graph_fact_evidence",
            [],
            |row| row.get::<_, String>(0),
        )
        .expect("links should be readable");
    assert_eq!(linked, "ev-existing");
}

#[test]
fn fact_evidence_backfill_rejects_invalid_evidence_json() {
    let connection = fact_schema();
    connection
        .execute(
            "INSERT INTO graph_relations (id, evidence_ids_json) VALUES (?1, ?2)",
            params!["rel", "not-json"],
        )
        .expect("relation should insert");

    let error = backfill_fact_evidence_kind(&connection, "relation", "graph_relations")
        .expect_err("invalid JSON should fail");
    assert!(matches!(error, StorageError::InvalidInput(_)));
}

fn fact_schema() -> Connection {
    let connection = Connection::open_in_memory().expect("database should open");
    connection
        .execute_batch(
            "
            CREATE TABLE evidence (
                id TEXT PRIMARY KEY
            );
            CREATE TABLE graph_relations (
                id TEXT PRIMARY KEY,
                evidence_ids_json TEXT NOT NULL
            );
            CREATE TABLE graph_fact_evidence (
                fact_kind TEXT NOT NULL,
                fact_id TEXT NOT NULL,
                evidence_id TEXT NOT NULL,
                PRIMARY KEY (fact_kind, fact_id, evidence_id),
                FOREIGN KEY (evidence_id) REFERENCES evidence(id) ON DELETE CASCADE
            );
            ",
        )
        .expect("fact schema should initialize");
    connection
}
