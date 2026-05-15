use rusqlite::{Connection, params};

use crate::storage::StorageError;

use super::indexing;

pub(super) fn ensure_core_schema_columns(connection: &Connection) -> Result<(), StorageError> {
    ensure_column(connection, "evidence", "source_path", "TEXT")?;
    ensure_column(connection, "evidence", "span_start_byte", "INTEGER")?;
    ensure_column(connection, "evidence", "span_end_byte", "INTEGER")?;
    ensure_column(connection, "evidence", "span_start_line", "INTEGER")?;
    ensure_column(connection, "evidence", "span_end_line", "INTEGER")?;
    ensure_column(
        connection,
        "evidence",
        "confidence_basis_points",
        "INTEGER NOT NULL DEFAULT 10000",
    )?;
    ensure_column(
        connection,
        "evidence",
        "status",
        "TEXT NOT NULL DEFAULT 'accepted'",
    )?;
    ensure_column(
        connection,
        "evidence",
        "modality",
        "TEXT NOT NULL DEFAULT 'text_span'",
    )?;
    ensure_column(connection, "evidence", "source_uri", "TEXT")?;
    ensure_column(connection, "evidence", "source_hash", "TEXT")?;
    ensure_column(connection, "evidence", "media_hash", "TEXT")?;
    ensure_column(connection, "evidence", "extractor", "TEXT")?;
    ensure_column(connection, "evidence", "extractor_version", "TEXT")?;
    ensure_column(connection, "evidence", "observed_at", "TEXT")?;
    ensure_column(connection, "evidence", "parent_evidence_id", "TEXT")?;
    ensure_column(connection, "evidence", "layout_page_number", "INTEGER")?;
    ensure_column(connection, "evidence", "layout_x", "INTEGER")?;
    ensure_column(connection, "evidence", "layout_y", "INTEGER")?;
    ensure_column(connection, "evidence", "layout_width", "INTEGER")?;
    ensure_column(connection, "evidence", "layout_height", "INTEGER")?;
    ensure_column(connection, "evidence", "embedding_model", "TEXT")?;
    ensure_column(connection, "evidence", "embedding_dimension", "INTEGER")?;
    ensure_column(
        connection,
        "evidence",
        "extraction_status",
        "TEXT NOT NULL DEFAULT 'succeeded'",
    )?;
    ensure_column(connection, "evidence", "extraction_message", "TEXT")?;
    ensure_column(
        connection,
        "graph_mutations",
        "relation_count",
        "INTEGER NOT NULL DEFAULT 0",
    )?;
    ensure_column(
        connection,
        "graph_mutations",
        "claim_count",
        "INTEGER NOT NULL DEFAULT 0",
    )?;
    ensure_column(
        connection,
        "graph_mutations",
        "event_count",
        "INTEGER NOT NULL DEFAULT 0",
    )?;
    ensure_column(
        connection,
        "graph_mutations",
        "affected_scopes_json",
        "TEXT NOT NULL DEFAULT '[]'",
    )?;
    ensure_column(
        connection,
        "graph_mutations",
        "affected_entity_ids_json",
        "TEXT NOT NULL DEFAULT '[]'",
    )?;
    ensure_column(
        connection,
        "graph_mutations",
        "evidence_ids_json",
        "TEXT NOT NULL DEFAULT '[]'",
    )?;
    ensure_column(
        connection,
        "graph_mutations",
        "source_hashes_json",
        "TEXT NOT NULL DEFAULT '[]'",
    )?;
    backfill_legacy_mutation_metadata(connection)?;

    Ok(())
}

pub(super) fn ensure_column(
    connection: &Connection,
    table: &str,
    column: &str,
    definition: &str,
) -> Result<(), StorageError> {
    let mut statement = connection.prepare(&format!("PRAGMA table_info({table})"))?;
    let rows = statement.query_map([], |row| row.get::<_, String>(1))?;
    let columns = rows
        .collect::<Result<Vec<_>, _>>()
        .map_err(StorageError::from)?;
    if !columns.iter().any(|existing| existing == column) {
        connection.execute(
            &format!("ALTER TABLE {table} ADD COLUMN {column} {definition}"),
            [],
        )?;
    }

    Ok(())
}

fn backfill_legacy_mutation_metadata(connection: &Connection) -> Result<(), StorageError> {
    let versions = {
        let mut statement = connection.prepare(
            "
            SELECT graph_version
            FROM graph_mutations
            WHERE evidence_count > 0
              AND (affected_scopes_json IS NULL OR affected_scopes_json = '[]')
            ORDER BY graph_version ASC
            ",
        )?;
        let rows = statement.query_map([], |row| row.get::<_, u64>(0))?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(StorageError::from)?
    };

    for version in versions {
        let scopes = collect_strings(
            connection,
            "
            SELECT DISTINCT source_scope
            FROM evidence
            WHERE created_graph_version <= ?1
            ORDER BY source_scope ASC
            ",
            version,
        )?;
        let evidence_ids = collect_strings(
            connection,
            "
            SELECT id
            FROM evidence
            WHERE created_graph_version <= ?1
            ORDER BY id ASC
            ",
            version,
        )?;
        let source_hashes = legacy_source_hashes(connection, version)?;
        connection.execute(
            "
            UPDATE graph_mutations
            SET affected_scopes_json = ?2,
                evidence_ids_json = ?3,
                source_hashes_json = ?4
            WHERE graph_version = ?1
            ",
            params![
                version,
                indexing::json_array(scopes)?,
                indexing::json_array(evidence_ids)?,
                indexing::json_array(source_hashes)?
            ],
        )?;
    }

    Ok(())
}

fn collect_strings(
    connection: &Connection,
    sql: &'static str,
    graph_version: u64,
) -> Result<Vec<String>, StorageError> {
    let mut statement = connection.prepare(sql)?;
    let rows = statement.query_map(params![graph_version], |row| row.get::<_, String>(0))?;

    rows.collect::<Result<Vec<_>, _>>()
        .map_err(StorageError::from)
}

fn legacy_source_hashes(
    connection: &Connection,
    graph_version: u64,
) -> Result<Vec<String>, StorageError> {
    let mut statement = connection.prepare(
        "
        SELECT source_scope, source_path, content, source_hash
        FROM evidence
        WHERE created_graph_version <= ?1
        ORDER BY id ASC
        ",
    )?;
    let rows = statement.query_map(params![graph_version], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, Option<String>>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, Option<String>>(3)?,
        ))
    })?;
    Ok(rows
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .map(|(scope, path, content, source_hash)| {
            source_hash.unwrap_or_else(|| indexing::source_hash(&scope, path.as_deref(), &content))
        })
        .collect())
}
