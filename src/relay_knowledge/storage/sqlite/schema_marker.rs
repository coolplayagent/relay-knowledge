use rusqlite::{Connection, OptionalExtension, params};

use crate::storage::StorageError;

const SCHEMA_MARKER_KEY: &str = "sqlite_graph_store";
const SCHEMA_MARKER_VERSION: i64 = 2;
const GRAPH_BM25_COLUMNS: &[&str] = &[
    "document_id",
    "document_kind",
    "evidence_id",
    "parent_evidence_id",
    "modality",
    "created_graph_version",
    "source_scope",
    "source_path",
    "entity_labels",
    "entity_aliases",
    "content",
];
const GRAPH_SEMANTIC_COLUMNS: &[&str] = &[
    "document_id",
    "document_kind",
    "evidence_id",
    "parent_evidence_id",
    "modality",
    "created_graph_version",
    "source_scope",
    "source_path",
    "entity_labels_json",
    "content",
    "token_signature_json",
    "model",
    "dimension",
    "source_hash",
    "tokenizer_version",
];
const GRAPH_VECTOR_COLUMNS: &[&str] = &[
    "document_id",
    "document_kind",
    "evidence_id",
    "parent_evidence_id",
    "modality",
    "created_graph_version",
    "source_scope",
    "source_path",
    "entity_labels_json",
    "content",
    "vector_json",
    "model",
    "dimension",
    "source_hash",
    "tokenizer_version",
];
const GRAPH_BM25_LABEL_GRAM_COLUMNS: &[&str] = &[
    "document_id",
    "document_kind",
    "source_scope",
    "created_graph_version",
    "label",
    "label_lower",
    "label_len",
    "gram_size",
    "gram",
];
const CODE_WORKSPACE_PACKAGE_MAPPING_COLUMNS: &[&str] = &[
    "set_id",
    "package_name",
    "ecosystem",
    "repository_id",
    "source_scope",
    "workspace_format",
    "created_at_ms",
];
const CODE_WORKSPACE_PACKAGE_MAPPING_UNIQUE: &[&str] = &["set_id", "package_name", "ecosystem"];

pub(super) fn schema_initialization_is_current(
    connection: &Connection,
) -> Result<bool, StorageError> {
    if !schema_marker_table_exists(connection)? {
        return Ok(false);
    }
    let version = connection
        .query_row(
            "
            SELECT version
            FROM relay_storage_schema_state
            WHERE key = ?1
            ",
            params![SCHEMA_MARKER_KEY],
            |row| row.get::<_, i64>(0),
        )
        .optional()?;

    if version != Some(SCHEMA_MARKER_VERSION) {
        return Ok(false);
    }
    if !table_has_columns(connection, "graph_bm25", GRAPH_BM25_COLUMNS)?
        || !table_has_columns(
            connection,
            "graph_semantic_documents",
            GRAPH_SEMANTIC_COLUMNS,
        )?
        || !table_has_columns(connection, "graph_vector_documents", GRAPH_VECTOR_COLUMNS)?
        || !table_has_columns(
            connection,
            "graph_bm25_label_grams",
            GRAPH_BM25_LABEL_GRAM_COLUMNS,
        )?
        || !workspace_package_mappings_current(connection)?
    {
        return Ok(false);
    }
    if !super::retrieval::derived_documents_current(connection)? {
        return Ok(false);
    }
    if !fact_evidence_links_are_current(connection)? {
        return Ok(false);
    }

    Ok(true)
}

pub(super) fn initialize_schema_marker(connection: &Connection) -> Result<(), StorageError> {
    connection.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS relay_storage_schema_state (
            key TEXT PRIMARY KEY,
            version INTEGER NOT NULL,
            updated_at_ms INTEGER NOT NULL
        );
        ",
    )?;

    Ok(())
}

pub(super) fn mark_schema_initialization_current(
    connection: &Connection,
) -> Result<(), StorageError> {
    initialize_schema_marker(connection)?;
    connection.execute(
        "
        INSERT INTO relay_storage_schema_state (key, version, updated_at_ms)
        VALUES (?1, ?2, CAST(strftime('%s', 'now') AS INTEGER) * 1000)
        ON CONFLICT(key) DO UPDATE SET
            version = excluded.version,
            updated_at_ms = excluded.updated_at_ms
        ",
        params![SCHEMA_MARKER_KEY, SCHEMA_MARKER_VERSION],
    )?;

    Ok(())
}

fn schema_marker_table_exists(connection: &Connection) -> Result<bool, StorageError> {
    table_exists(connection, "relay_storage_schema_state")
}

fn table_has_columns(
    connection: &Connection,
    table: &str,
    required_columns: &[&str],
) -> Result<bool, StorageError> {
    if !table_exists(connection, table)? {
        return Ok(false);
    }
    let mut statement = connection.prepare(&format!("PRAGMA table_info({table})"))?;
    let rows = statement.query_map([], |row| row.get::<_, String>(1))?;
    let columns = rows
        .collect::<Result<Vec<_>, _>>()
        .map_err(StorageError::from)?;

    Ok(required_columns
        .iter()
        .all(|required| columns.iter().any(|column| column == required)))
}

fn workspace_package_mappings_current(connection: &Connection) -> Result<bool, StorageError> {
    if !table_has_columns(
        connection,
        "code_workspace_package_mappings",
        CODE_WORKSPACE_PACKAGE_MAPPING_COLUMNS,
    )? {
        return Ok(false);
    }
    table_has_unique_columns(
        connection,
        "code_workspace_package_mappings",
        CODE_WORKSPACE_PACKAGE_MAPPING_UNIQUE,
    )
}

fn table_has_unique_columns(
    connection: &Connection,
    table: &str,
    expected_columns: &[&str],
) -> Result<bool, StorageError> {
    let mut statement = connection.prepare(&format!("PRAGMA index_list({table})"))?;
    let rows = statement.query_map([], |row| {
        Ok((row.get::<_, String>(1)?, row.get::<_, i64>(2)? != 0))
    })?;
    let indexes = rows
        .collect::<Result<Vec<_>, _>>()
        .map_err(StorageError::from)?;

    for (index_name, unique) in indexes {
        if !unique {
            continue;
        }
        let mut statement = connection.prepare(&format!("PRAGMA index_info({index_name})"))?;
        let rows = statement.query_map([], |row| row.get::<_, String>(2))?;
        let columns = rows
            .collect::<Result<Vec<_>, _>>()
            .map_err(StorageError::from)?;
        if columns
            .iter()
            .map(String::as_str)
            .eq(expected_columns.iter().copied())
        {
            return Ok(true);
        }
    }

    Ok(false)
}

fn fact_evidence_links_are_current(connection: &Connection) -> Result<bool, StorageError> {
    if !table_exists(connection, "graph_fact_evidence")? {
        return Ok(false);
    }
    for (fact_kind, table) in [
        ("relation", "graph_relations"),
        ("claim", "graph_claims"),
        ("event", "graph_events"),
    ] {
        if !fact_evidence_links_are_current_for_kind(connection, fact_kind, table)? {
            return Ok(false);
        }
    }

    Ok(true)
}

fn fact_evidence_links_are_current_for_kind(
    connection: &Connection,
    fact_kind: &'static str,
    table: &'static str,
) -> Result<bool, StorageError> {
    if !table_exists(connection, table)? {
        return Ok(true);
    }
    let mut statement =
        connection.prepare(&format!("SELECT id, evidence_ids_json FROM {table}"))?;
    let rows = statement.query_map([], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
    })?;
    let facts = rows
        .collect::<Result<Vec<_>, _>>()
        .map_err(StorageError::from)?;
    drop(statement);

    for (fact_id, evidence_json) in facts {
        let evidence_ids: Vec<String> = serde_json::from_str(&evidence_json)
            .map_err(|error| StorageError::InvalidInput(error.to_string()))?;
        for evidence_id in evidence_ids {
            if !fact_evidence_link_exists(connection, fact_kind, &fact_id, &evidence_id)? {
                return Ok(false);
            }
        }
    }

    Ok(true)
}

fn fact_evidence_link_exists(
    connection: &Connection,
    fact_kind: &str,
    fact_id: &str,
    evidence_id: &str,
) -> Result<bool, StorageError> {
    connection
        .query_row(
            "
            SELECT EXISTS (
                SELECT 1
                FROM graph_fact_evidence
                WHERE fact_kind = ?1
                  AND fact_id = ?2
                  AND evidence_id = ?3
            )
            ",
            params![fact_kind, fact_id, evidence_id],
            |row| row.get::<_, bool>(0),
        )
        .map_err(StorageError::from)
}

fn table_exists(connection: &Connection, table: &str) -> Result<bool, StorageError> {
    connection
        .query_row(
            "
            SELECT EXISTS (
                SELECT 1
                FROM sqlite_master
                WHERE type = 'table'
                  AND name = ?1
            )
            ",
            params![table],
            |row| row.get::<_, bool>(0),
        )
        .map_err(StorageError::from)
}

#[cfg(test)]
mod tests {
    use super::{mark_schema_initialization_current, schema_initialization_is_current};

    #[test]
    fn schema_marker_reports_current_only_after_successful_mark() {
        let store = super::super::SqliteGraphStore::open_in_memory().expect("store should open");
        let connection = store.connection.lock().expect("connection should lock");

        assert!(
            !schema_initialization_is_current(&connection)
                .expect("missing marker should be readable")
        );

        mark_schema_initialization_current(&connection).expect("marker should write");

        assert!(
            schema_initialization_is_current(&connection)
                .expect("current marker should be readable")
        );
    }

    #[test]
    fn schema_marker_requires_label_gram_table() {
        let store = super::super::SqliteGraphStore::open_in_memory().expect("store should open");
        let connection = store.connection.lock().expect("connection should lock");
        mark_schema_initialization_current(&connection).expect("marker should write");

        connection
            .execute("DROP TABLE graph_bm25_label_grams", [])
            .expect("label gram table should drop");

        assert!(
            !schema_initialization_is_current(&connection)
                .expect("missing label gram table should be detected")
        );
    }

    #[test]
    fn schema_marker_requires_workspace_mapping_ecosystem_unique_key() {
        let store = super::super::SqliteGraphStore::open_in_memory().expect("store should open");
        let connection = store.connection.lock().expect("connection should lock");
        mark_schema_initialization_current(&connection).expect("marker should write");
        connection
            .execute("DROP TABLE code_workspace_package_mappings", [])
            .expect("workspace mappings should drop");
        connection
            .execute_batch(
                "
                CREATE TABLE code_workspace_package_mappings (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    set_id TEXT NOT NULL,
                    package_name TEXT NOT NULL,
                    ecosystem TEXT NOT NULL,
                    repository_id TEXT NOT NULL,
                    source_scope TEXT NOT NULL,
                    workspace_format TEXT NOT NULL,
                    created_at_ms INTEGER NOT NULL,
                    UNIQUE (set_id, package_name)
                );
                ",
            )
            .expect("legacy workspace mappings should create");

        assert!(
            !schema_initialization_is_current(&connection)
                .expect("legacy workspace mapping uniqueness should be detected")
        );
    }

    #[test]
    fn schema_marker_rejects_previous_label_gram_migration_version() {
        let store = super::super::SqliteGraphStore::open_in_memory().expect("store should open");
        let connection = store.connection.lock().expect("connection should lock");
        super::initialize_schema_marker(&connection).expect("marker table should initialize");
        connection
            .execute(
                "
                INSERT INTO relay_storage_schema_state (key, version, updated_at_ms)
                VALUES ('sqlite_graph_store', 1, 0)
                ",
                [],
            )
            .expect("previous marker should insert");

        assert!(
            !schema_initialization_is_current(&connection)
                .expect("previous label gram migration marker should be stale")
        );
    }
}
