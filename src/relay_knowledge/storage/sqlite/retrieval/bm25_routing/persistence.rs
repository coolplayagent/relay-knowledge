use rusqlite::{Connection, OptionalExtension, params};

use crate::storage::StorageError;

use super::{PreparedBm25Route, ROUTING_ALGORITHM_VERSION};

const REBUILD_LEASE_DURATION_MS: i64 = 60_000;
const DELETE_BATCH_SIZE: usize = 256;

pub(in crate::storage::sqlite::retrieval) struct RebuildLease {
    owner: String,
}

pub(in crate::storage::sqlite::retrieval) struct Bm25RouteDocumentIdentity {
    pub(in crate::storage::sqlite::retrieval) document_id: String,
    pub(in crate::storage::sqlite::retrieval) fts_rowid: i64,
}

pub(in crate::storage::sqlite) fn ensure_rebuild_inactive(
    connection: &Connection,
) -> Result<(), StorageError> {
    let rebuilding = connection.query_row(
        "SELECT state = 'building' FROM graph_bm25_route_state WHERE id = 1",
        [],
        |row| row.get::<_, bool>(0),
    )?;
    if rebuilding {
        return Err(StorageError::Busy(
            "BM25 derived-index rebuild is active; retry the graph write after it completes"
                .to_owned(),
        ));
    }
    Ok(())
}

pub(in crate::storage::sqlite::retrieval) fn replace_document(
    connection: &Connection,
    document_id: &str,
    fts_rowid: i64,
    document_kind: &str,
    source_path: Option<&str>,
    label_gram_state: &str,
    route: &PreparedBm25Route,
) -> Result<(), StorageError> {
    delete_document(connection, document_id, route.graph_version)?;
    let term_counts_json = serde_json::to_string(&route.term_counts)
        .map_err(|error| StorageError::InvalidInput(error.to_string()))?;
    connection.execute(
        "
        INSERT INTO graph_bm25_route_documents (
            document_id, fts_rowid, document_kind, created_graph_version,
            source_scope, source_path, label_gram_state, group_token,
            term_counts_json
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
        ",
        params![
            document_id,
            fts_rowid,
            document_kind,
            route.graph_version,
            route.source_scope,
            source_path,
            label_gram_state,
            route.group_token,
            term_counts_json,
        ],
    )?;
    connection.execute(
        "
        INSERT INTO graph_bm25_route_groups (
            source_scope, group_token, document_count
        ) VALUES (?1, ?2, 1)
        ON CONFLICT(source_scope, group_token) DO UPDATE SET
            document_count = document_count + 1
        ",
        params![route.source_scope, route.group_token],
    )?;
    connection.execute(
        "
        INSERT INTO graph_bm25_route_terms (
            term, source_scope, group_token, collection_frequency
        )
        SELECT json_extract(value, '$[0]'), ?1, ?2,
               CAST(json_extract(value, '$[1]') AS INTEGER)
        FROM json_each(?3)
        WHERE 1
        ON CONFLICT(term, source_scope, group_token) DO UPDATE SET
            collection_frequency = collection_frequency + excluded.collection_frequency
        ",
        params![route.source_scope, route.group_token, term_counts_json],
    )?;
    connection.execute(
        "
        INSERT INTO graph_bm25_route_term_totals (
            term, document_frequency
        )
        SELECT json_extract(value, '$[0]'), 1
        FROM json_each(?1)
        WHERE 1
        ON CONFLICT(term) DO UPDATE SET
            document_frequency = document_frequency + 1
        ",
        params![term_counts_json],
    )?;
    connection.execute(
        "UPDATE graph_bm25_route_state
         SET document_count = document_count + 1
         WHERE id = 1",
        [],
    )?;
    mark_graph_version(connection, route.graph_version)?;
    Ok(())
}

pub(in crate::storage::sqlite::retrieval) fn mark_label_gram_state(
    connection: &Connection,
    document_id: &str,
    graph_version: u64,
    state: &str,
) -> Result<(), StorageError> {
    let updated = connection.execute(
        "UPDATE graph_bm25_route_documents
         SET label_gram_state = ?1
         WHERE document_id = ?2 AND created_graph_version = ?3",
        params![state, document_id, graph_version],
    )?;
    if updated != 1 {
        return Err(StorageError::InvalidInput(format!(
            "BM25 label-gram state has no route document for {document_id} at graph version \
             {graph_version}"
        )));
    }
    Ok(())
}

pub(in crate::storage::sqlite::retrieval) fn delete_document(
    connection: &Connection,
    document_id: &str,
    graph_version: u64,
) -> Result<(), StorageError> {
    let stored = connection
        .query_row(
            "
            SELECT source_scope, group_token, term_counts_json
            FROM graph_bm25_route_documents
            WHERE document_id = ?1
            ",
            params![document_id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ))
            },
        )
        .optional()?;
    let Some((source_scope, group_token, term_counts_json)) = stored else {
        mark_graph_version(connection, graph_version)?;
        return Ok(());
    };
    connection.execute(
        "DELETE FROM graph_bm25_route_documents WHERE document_id = ?1",
        params![document_id],
    )?;
    connection.execute(
        "
        WITH removed(term, frequency) AS (
            SELECT json_extract(value, '$[0]'),
                   CAST(json_extract(value, '$[1]') AS INTEGER)
            FROM json_each(?3)
        )
        UPDATE graph_bm25_route_terms AS aggregate
        SET collection_frequency = collection_frequency - removed.frequency
        FROM removed
        WHERE aggregate.term = removed.term
          AND aggregate.source_scope = ?1
          AND aggregate.group_token = ?2
        ",
        params![source_scope, group_token, term_counts_json],
    )?;
    connection.execute(
        "
        DELETE FROM graph_bm25_route_terms
        WHERE source_scope = ?1 AND group_token = ?2
          AND collection_frequency <= 0
        ",
        params![source_scope, group_token],
    )?;
    connection.execute(
        "
        WITH removed(term) AS (
            SELECT json_extract(value, '$[0]')
            FROM json_each(?1)
        )
        UPDATE graph_bm25_route_term_totals AS aggregate
        SET document_frequency = document_frequency - 1
        FROM removed
        WHERE aggregate.term = removed.term
        ",
        params![term_counts_json],
    )?;
    connection.execute(
        "
        DELETE FROM graph_bm25_route_term_totals
        WHERE term IN (
            SELECT json_extract(value, '$[0]') FROM json_each(?1)
        ) AND document_frequency <= 0
        ",
        params![term_counts_json],
    )?;
    connection.execute(
        "
        UPDATE graph_bm25_route_groups
        SET document_count = document_count - 1
        WHERE source_scope = ?1 AND group_token = ?2
        ",
        params![source_scope, group_token],
    )?;
    connection.execute(
        "DELETE FROM graph_bm25_route_groups
         WHERE source_scope = ?1 AND group_token = ?2 AND document_count <= 0",
        params![source_scope, group_token],
    )?;
    connection.execute(
        "UPDATE graph_bm25_route_state
         SET document_count = MAX(document_count - 1, 0)
         WHERE id = 1",
        [],
    )?;
    mark_graph_version(connection, graph_version)?;
    Ok(())
}

pub(in crate::storage::sqlite::retrieval) fn code_document_batch(
    connection: &Connection,
    source_scope: &str,
    path: &str,
) -> Result<Vec<Bm25RouteDocumentIdentity>, StorageError> {
    let mut statement = connection.prepare(
        "SELECT document_id, fts_rowid
         FROM graph_bm25_route_documents
         WHERE document_kind IN ('code_symbol', 'code_chunk')
           AND source_scope = ?1 AND source_path = ?2
         ORDER BY document_id
         LIMIT ?3",
    )?;
    let rows = statement.query_map(params![source_scope, path, DELETE_BATCH_SIZE], |row| {
        Ok(Bm25RouteDocumentIdentity {
            document_id: row.get(0)?,
            fts_rowid: row.get(1)?,
        })
    })?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(StorageError::from)
}

pub(in crate::storage::sqlite::retrieval) fn delete_code_document_batch(
    connection: &Connection,
    source_scope: &str,
    path: &str,
    batch_count: usize,
) -> Result<usize, StorageError> {
    connection.execute(
        "WITH removed_documents AS (
             SELECT source_scope, group_token, term_counts_json
             FROM graph_bm25_route_documents
             WHERE document_kind IN ('code_symbol', 'code_chunk')
               AND source_scope = ?1 AND source_path = ?2
             ORDER BY document_id
             LIMIT ?3
         ),
         removed_terms AS (
             SELECT json_extract(item.value, '$[0]') AS term,
                    document.source_scope,
                    document.group_token,
                    SUM(CAST(json_extract(item.value, '$[1]') AS INTEGER))
                        AS collection_frequency
             FROM removed_documents AS document,
                  json_each(document.term_counts_json) AS item
             GROUP BY term, document.source_scope, document.group_token
         )
         UPDATE graph_bm25_route_terms AS aggregate
         SET collection_frequency =
                 aggregate.collection_frequency - removed.collection_frequency
         FROM removed_terms AS removed
         WHERE aggregate.term = removed.term
           AND aggregate.source_scope = removed.source_scope
           AND aggregate.group_token = removed.group_token",
        params![source_scope, path, DELETE_BATCH_SIZE],
    )?;
    connection.execute(
        "WITH removed_documents AS (
             SELECT source_scope, group_token, term_counts_json
             FROM graph_bm25_route_documents
             WHERE document_kind IN ('code_symbol', 'code_chunk')
               AND source_scope = ?1 AND source_path = ?2
             ORDER BY document_id
             LIMIT ?3
         ),
         removed_terms AS (
             SELECT json_extract(item.value, '$[0]') AS term,
                    document.source_scope, document.group_token
             FROM removed_documents AS document,
                  json_each(document.term_counts_json) AS item
             GROUP BY term, document.source_scope, document.group_token
         )
         DELETE FROM graph_bm25_route_terms
         WHERE (term, source_scope, group_token) IN (
             SELECT term, source_scope, group_token FROM removed_terms
         ) AND collection_frequency <= 0",
        params![source_scope, path, DELETE_BATCH_SIZE],
    )?;
    connection.execute(
        "WITH removed_documents AS (
             SELECT term_counts_json
             FROM graph_bm25_route_documents
             WHERE document_kind IN ('code_symbol', 'code_chunk')
               AND source_scope = ?1 AND source_path = ?2
             ORDER BY document_id
             LIMIT ?3
         ),
         removed_terms AS (
             SELECT json_extract(item.value, '$[0]') AS term,
                    COUNT(*) AS document_frequency
             FROM removed_documents AS document,
                  json_each(document.term_counts_json) AS item
             GROUP BY term
         )
         UPDATE graph_bm25_route_term_totals AS aggregate
         SET document_frequency =
                 aggregate.document_frequency - removed.document_frequency
         FROM removed_terms AS removed
         WHERE aggregate.term = removed.term",
        params![source_scope, path, DELETE_BATCH_SIZE],
    )?;
    connection.execute(
        "WITH removed_documents AS (
             SELECT term_counts_json
             FROM graph_bm25_route_documents
             WHERE document_kind IN ('code_symbol', 'code_chunk')
               AND source_scope = ?1 AND source_path = ?2
             ORDER BY document_id
             LIMIT ?3
         ),
         removed_terms AS (
             SELECT json_extract(item.value, '$[0]') AS term
             FROM removed_documents AS document,
                  json_each(document.term_counts_json) AS item
             GROUP BY term
         )
         DELETE FROM graph_bm25_route_term_totals
         WHERE term IN (SELECT term FROM removed_terms)
           AND document_frequency <= 0",
        params![source_scope, path, DELETE_BATCH_SIZE],
    )?;
    connection.execute(
        "WITH removed_documents AS (
             SELECT document_id, source_scope, group_token
             FROM graph_bm25_route_documents
             WHERE document_kind IN ('code_symbol', 'code_chunk')
               AND source_scope = ?1 AND source_path = ?2
             ORDER BY document_id
             LIMIT ?3
         ),
         removed_groups AS (
             SELECT source_scope, group_token, COUNT(*) AS document_count
             FROM removed_documents
             GROUP BY source_scope, group_token
         )
         UPDATE graph_bm25_route_groups AS aggregate
         SET document_count = aggregate.document_count - removed.document_count
         FROM removed_groups AS removed
         WHERE aggregate.source_scope = removed.source_scope
           AND aggregate.group_token = removed.group_token",
        params![source_scope, path, DELETE_BATCH_SIZE],
    )?;
    connection.execute(
        "WITH removed_documents AS (
             SELECT source_scope, group_token
             FROM graph_bm25_route_documents
             WHERE document_kind IN ('code_symbol', 'code_chunk')
               AND source_scope = ?1 AND source_path = ?2
             ORDER BY document_id
             LIMIT ?3
         )
         DELETE FROM graph_bm25_route_groups
         WHERE (source_scope, group_token) IN (
             SELECT source_scope, group_token FROM removed_documents
         ) AND document_count <= 0",
        params![source_scope, path, DELETE_BATCH_SIZE],
    )?;
    connection.execute(
        "UPDATE graph_bm25_route_state
         SET document_count = MAX(document_count - ?1, 0)
         WHERE id = 1",
        params![batch_count],
    )?;
    connection
        .execute(
            "DELETE FROM graph_bm25_route_documents
             WHERE document_id IN (
                 SELECT document_id
                 FROM graph_bm25_route_documents
                 WHERE document_kind IN ('code_symbol', 'code_chunk')
                   AND source_scope = ?1 AND source_path = ?2
                 ORDER BY document_id
                 LIMIT ?3
             )",
            params![source_scope, path, DELETE_BATCH_SIZE],
        )
        .map_err(StorageError::from)
}

pub(in crate::storage::sqlite::retrieval) fn begin_rebuild(
    connection: &Connection,
) -> Result<RebuildLease, StorageError> {
    let owner = connection.query_row("SELECT lower(hex(randomblob(16)))", [], |row| {
        row.get::<_, String>(0)
    })?;
    let updated = connection.execute(
        "UPDATE graph_bm25_route_state
         SET indexed_graph_version = CASE WHEN state = 'building'
                     AND algorithm_version = ?1
                 THEN indexed_graph_version ELSE 0 END,
             document_count = CASE WHEN state = 'building'
                     AND algorithm_version = ?1
                 THEN document_count ELSE 0 END,
             state = 'building', algorithm_version = ?1,
             rebuild_phase = CASE WHEN state = 'building'
                     AND algorithm_version = ?1
                 THEN COALESCE(rebuild_phase, 'prepare') ELSE 'prepare' END,
             rebuild_cursor = CASE WHEN state = 'building'
                     AND algorithm_version = ?1
                 THEN rebuild_cursor ELSE NULL END,
             rebuild_semantic = CASE WHEN state = 'building'
                     AND algorithm_version = ?1
                 THEN rebuild_semantic ELSE NULL END,
             rebuild_vector = CASE WHEN state = 'building'
                     AND algorithm_version = ?1
                 THEN rebuild_vector ELSE NULL END,
             rebuild_owner = ?2,
             rebuild_lease_expires_at_ms =
                 CAST(strftime('%s', 'now') AS INTEGER) * 1000 + ?3
         WHERE id = 1 AND (
             state <> 'building' OR rebuild_lease_expires_at_ms IS NULL OR
             rebuild_lease_expires_at_ms <=
                 CAST(strftime('%s', 'now') AS INTEGER) * 1000
         )",
        params![ROUTING_ALGORITHM_VERSION, owner, REBUILD_LEASE_DURATION_MS],
    )?;
    if updated != 1 {
        return Err(StorageError::Busy(
            "another BM25 route rebuild holds the durable lease".to_owned(),
        ));
    }
    Ok(RebuildLease { owner })
}

pub(in crate::storage::sqlite::retrieval) fn configure_rebuild(
    connection: &Connection,
    lease: &RebuildLease,
    semantic: bool,
    vector: bool,
) -> Result<(String, Option<String>, bool, bool), StorageError> {
    let updated = connection.execute(
        "UPDATE graph_bm25_route_state
         SET rebuild_semantic = COALESCE(rebuild_semantic, ?1),
             rebuild_vector = COALESCE(rebuild_vector, ?2),
             rebuild_lease_expires_at_ms =
                 CAST(strftime('%s', 'now') AS INTEGER) * 1000 + ?3
         WHERE id = 1 AND state = 'building' AND rebuild_owner = ?4",
        params![semantic, vector, REBUILD_LEASE_DURATION_MS, lease.owner],
    )?;
    if updated != 1 {
        return Err(StorageError::Busy(
            "BM25 route rebuild configuration lost its durable lease".to_owned(),
        ));
    }
    connection
        .query_row(
            "SELECT rebuild_phase, rebuild_cursor, rebuild_semantic, rebuild_vector
             FROM graph_bm25_route_state
             WHERE id = 1 AND state = 'building' AND rebuild_owner = ?1",
            params![lease.owner],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .map_err(StorageError::from)
}

pub(in crate::storage::sqlite::retrieval) fn checkpoint_rebuild(
    connection: &Connection,
    lease: &RebuildLease,
    phase: &str,
    cursor: Option<&str>,
) -> Result<(), StorageError> {
    let updated = connection.execute(
        "UPDATE graph_bm25_route_state
         SET rebuild_phase = ?1, rebuild_cursor = ?2,
             rebuild_lease_expires_at_ms =
                 CAST(strftime('%s', 'now') AS INTEGER) * 1000 + ?3
         WHERE id = 1 AND state = 'building' AND rebuild_owner = ?4",
        params![phase, cursor, REBUILD_LEASE_DURATION_MS, lease.owner],
    )?;
    if updated != 1 {
        return Err(StorageError::Busy(
            "BM25 route rebuild checkpoint lost its durable lease".to_owned(),
        ));
    }
    Ok(())
}

pub(in crate::storage::sqlite::retrieval) fn renew_rebuild(
    connection: &Connection,
    lease: &RebuildLease,
) -> Result<(), StorageError> {
    let updated = connection.execute(
        "UPDATE graph_bm25_route_state
         SET rebuild_lease_expires_at_ms =
                 CAST(strftime('%s', 'now') AS INTEGER) * 1000 + ?1
         WHERE id = 1 AND state = 'building' AND rebuild_owner = ?2",
        params![REBUILD_LEASE_DURATION_MS, lease.owner],
    )?;
    if updated != 1 {
        return Err(StorageError::Busy(
            "BM25 route rebuild lease is no longer owned by this writer".to_owned(),
        ));
    }
    Ok(())
}

pub(in crate::storage::sqlite::retrieval) fn finish_rebuild(
    connection: &Connection,
    lease: &RebuildLease,
    graph_version: u64,
    semantic_generation: Option<&str>,
    vector_generation: Option<&str>,
) -> Result<(), StorageError> {
    let updated = connection.execute(
        "UPDATE graph_bm25_route_state
         SET indexed_graph_version = MAX(indexed_graph_version, ?1),
             state = 'fresh', algorithm_version = ?2,
             semantic_generation = COALESCE(?3, semantic_generation),
             vector_generation = COALESCE(?4, vector_generation),
             rebuild_phase = NULL, rebuild_cursor = NULL,
             rebuild_semantic = NULL, rebuild_vector = NULL,
             rebuild_owner = NULL, rebuild_lease_expires_at_ms = NULL
         WHERE id = 1 AND state = 'building' AND rebuild_owner = ?5",
        params![
            graph_version,
            ROUTING_ALGORITHM_VERSION,
            semantic_generation,
            vector_generation,
            lease.owner
        ],
    )?;
    if updated != 1 {
        return Err(StorageError::InvalidInput(
            "BM25 route rebuild cannot finalize outside the building state".to_owned(),
        ));
    }
    Ok(())
}

pub(in crate::storage::sqlite) fn mark_graph_version(
    connection: &Connection,
    graph_version: u64,
) -> Result<(), StorageError> {
    connection.execute(
        "
        UPDATE graph_bm25_route_state
        SET indexed_graph_version = MAX(indexed_graph_version, ?1),
            algorithm_version = ?2
        WHERE id = 1
        ",
        params![graph_version, ROUTING_ALGORITHM_VERSION],
    )?;
    Ok(())
}

#[cfg(test)]
#[path = "persistence_tests.rs"]
mod tests;
