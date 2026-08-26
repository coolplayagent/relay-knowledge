use rusqlite::{Connection, params};

use super::*;

#[test]
fn core_column_upgrade_backfills_legacy_mutation_metadata() {
    let connection = legacy_core_schema();
    connection
        .execute(
            "
            INSERT INTO evidence (id, source_scope, content, created_graph_version)
            VALUES (?1, ?2, ?3, ?4)
            ",
            params!["ev", "scope", "legacy content", 1],
        )
        .expect("legacy evidence should insert");
    connection
        .execute(
            "INSERT INTO graph_mutations (graph_version, evidence_count) VALUES (?1, ?2)",
            params![1, 1],
        )
        .expect("legacy mutation should insert");

    ensure_core_schema_columns(&connection).expect("legacy columns should upgrade");

    let metadata = connection
        .query_row(
            "
            SELECT affected_scopes_json, evidence_ids_json, source_hashes_json
            FROM graph_mutations
            WHERE graph_version = 1
            ",
            [],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ))
            },
        )
        .expect("mutation metadata should load");
    assert_eq!(metadata.0, r#"["scope"]"#);
    assert_eq!(metadata.1, r#"["ev"]"#);
    assert_eq!(
        metadata.2,
        format!(
            r#"["{}"]"#,
            indexing::source_hash("scope", None, "legacy content")
        )
    );

    let evidence_defaults = connection
        .query_row(
            "
            SELECT confidence_basis_points, status, modality, extraction_status
            FROM evidence
            WHERE id = 'ev'
            ",
            [],
            |row| {
                Ok((
                    row.get::<_, u16>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                ))
            },
        )
        .expect("upgraded evidence should load");
    assert_eq!(
        evidence_defaults,
        (
            10_000,
            "accepted".to_owned(),
            "text_span".to_owned(),
            "succeeded".to_owned(),
        )
    );
}

#[test]
fn ensure_column_is_idempotent() {
    let connection = Connection::open_in_memory().expect("database should open");
    connection
        .execute("CREATE TABLE sample (id INTEGER PRIMARY KEY)", [])
        .expect("sample table should initialize");

    ensure_column(&connection, "sample", "label", "TEXT")
        .expect("first column addition should succeed");
    ensure_column(&connection, "sample", "label", "TEXT")
        .expect("existing column should be accepted");

    let label_columns = connection
        .prepare("PRAGMA table_info(sample)")
        .expect("table metadata should prepare")
        .query_map([], |row| row.get::<_, String>(1))
        .expect("table metadata should query")
        .collect::<Result<Vec<_>, _>>()
        .expect("table metadata should collect")
        .into_iter()
        .filter(|column| column == "label")
        .count();
    assert_eq!(label_columns, 1);
}

#[test]
fn ensure_column_recognizes_virtual_generated_columns() {
    let connection = Connection::open_in_memory().expect("database should open");
    connection
        .execute(
            "CREATE TABLE sample (
                value INTEGER NOT NULL,
                doubled INTEGER GENERATED ALWAYS AS (value * 2) VIRTUAL
            )",
            [],
        )
        .expect("sample table should initialize");

    ensure_column(
        &connection,
        "sample",
        "doubled",
        "INTEGER GENERATED ALWAYS AS (value * 2) VIRTUAL",
    )
    .expect("existing generated column should be accepted");

    let generated_columns = connection
        .prepare("PRAGMA table_xinfo(sample)")
        .expect("extended table metadata should prepare")
        .query_map([], |row| row.get::<_, String>(1))
        .expect("extended table metadata should query")
        .collect::<Result<Vec<_>, _>>()
        .expect("extended table metadata should collect")
        .into_iter()
        .filter(|column| column == "doubled")
        .count();
    assert_eq!(generated_columns, 1);
}

fn legacy_core_schema() -> Connection {
    let connection = Connection::open_in_memory().expect("database should open");
    connection
        .execute_batch(
            "
            CREATE TABLE entities (
                id TEXT PRIMARY KEY,
                label TEXT NOT NULL,
                created_graph_version INTEGER NOT NULL
            );
            CREATE TABLE evidence (
                id TEXT PRIMARY KEY,
                source_scope TEXT NOT NULL,
                content TEXT NOT NULL,
                created_graph_version INTEGER NOT NULL
            );
            CREATE TABLE graph_mutations (
                graph_version INTEGER PRIMARY KEY,
                evidence_count INTEGER NOT NULL
            );
            ",
        )
        .expect("legacy core schema should initialize");
    connection
}
