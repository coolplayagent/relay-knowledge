use std::{
    collections::BTreeSet,
    path::Path,
    sync::{Arc, Mutex},
};

mod code;

use rusqlite::{Connection, OptionalExtension, params};

mod code_graph;
mod retrieval;

use crate::{
    domain::{
        CodeChunkRecord, CodeGraphBatch, CodeGraphCommitReceipt, CodeReferenceRecord,
        CodeSymbolRecord, CommitReceipt, GraphMutationBatch, GraphVersion, IndexKind, IndexState,
        IndexStatus, RetrievalHit,
    },
    storage::{
        CodeChunkSearchRequest, CodeGraphStore, CodeReferenceSearchRequest,
        CodeSymbolSearchRequest, GraphInspection, GraphSearchRequest, GraphStore, IndexStore,
        MutationLogEntry, MutationLogStore, StorageError, StorageFuture,
    },
};

/// SQLite implementation of graph facts, mutation log, and index metadata.
#[derive(Debug, Clone)]
pub struct SqliteGraphStore {
    connection: Arc<Mutex<Connection>>,
}

impl SqliteGraphStore {
    /// Opens a SQLite database and initializes the v1 schema.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, StorageError> {
        if let Some(parent) = path.as_ref().parent() {
            std::fs::create_dir_all(parent)?;
        }

        let connection = Connection::open(path)?;
        initialize_schema(&connection)?;

        Ok(Self {
            connection: Arc::new(Mutex::new(connection)),
        })
    }

    /// Opens an in-memory database for isolated tests.
    pub fn open_in_memory() -> Result<Self, StorageError> {
        let connection = Connection::open_in_memory()?;
        initialize_schema(&connection)?;

        Ok(Self {
            connection: Arc::new(Mutex::new(connection)),
        })
    }

    fn run<T, F>(&self, operation: F) -> StorageFuture<'_, T>
    where
        T: Send + 'static,
        F: FnOnce(&mut Connection) -> Result<T, StorageError> + Send + 'static,
    {
        let connection = Arc::clone(&self.connection);

        Box::pin(async move {
            tokio::task::spawn_blocking(move || {
                let mut guard = connection.lock().map_err(|_| StorageError::LockPoisoned)?;

                operation(&mut guard)
            })
            .await?
        })
    }
}

impl GraphStore for SqliteGraphStore {
    fn commit_mutation_batch(&self, batch: GraphMutationBatch) -> StorageFuture<'_, CommitReceipt> {
        self.run(move |connection| commit_batch(connection, batch))
    }

    fn inspect_graph(&self) -> StorageFuture<'_, GraphInspection> {
        self.run(inspect_graph)
    }

    fn search(&self, request: GraphSearchRequest) -> StorageFuture<'_, Vec<RetrievalHit>> {
        self.run(move |connection| retrieval::search_graph(connection, request))
    }

    fn current_graph_version(&self) -> StorageFuture<'_, GraphVersion> {
        self.run(current_graph_version)
    }
}

impl MutationLogStore for SqliteGraphStore {
    fn read_after(
        &self,
        graph_version: GraphVersion,
        limit: usize,
    ) -> StorageFuture<'_, Vec<MutationLogEntry>> {
        self.run(move |connection| read_mutations_after(connection, graph_version, limit))
    }
}

impl IndexStore for SqliteGraphStore {
    fn index_statuses(&self) -> StorageFuture<'_, Vec<IndexStatus>> {
        self.run(index_statuses)
    }

    fn mark_refresh_complete(
        &self,
        kind: IndexKind,
        graph_version: GraphVersion,
    ) -> StorageFuture<'_, IndexStatus> {
        self.run(move |connection| mark_refresh_complete(connection, kind, graph_version))
    }
}

impl CodeGraphStore for SqliteGraphStore {
    fn commit_code_graph_batch(
        &self,
        batch: CodeGraphBatch,
    ) -> StorageFuture<'_, CodeGraphCommitReceipt> {
        self.run(move |connection| code_graph::commit_batch(connection, batch))
    }

    fn search_code_symbols(
        &self,
        request: CodeSymbolSearchRequest,
    ) -> StorageFuture<'_, Vec<CodeSymbolRecord>> {
        self.run(move |connection| code_graph::search_symbols(connection, request))
    }

    fn search_code_references(
        &self,
        request: CodeReferenceSearchRequest,
    ) -> StorageFuture<'_, Vec<CodeReferenceRecord>> {
        self.run(move |connection| code_graph::search_references(connection, request))
    }

    fn search_code_chunks(
        &self,
        request: CodeChunkSearchRequest,
    ) -> StorageFuture<'_, Vec<CodeChunkRecord>> {
        self.run(move |connection| code_graph::search_chunks(connection, request))
    }
}

fn initialize_schema(connection: &Connection) -> Result<(), StorageError> {
    connection.execute_batch(
        "
        PRAGMA foreign_keys = ON;
        PRAGMA journal_mode = WAL;

        CREATE TABLE IF NOT EXISTS graph_state (
            id INTEGER PRIMARY KEY CHECK (id = 1),
            graph_version INTEGER NOT NULL
        );

        INSERT OR IGNORE INTO graph_state (id, graph_version) VALUES (1, 0);

        CREATE TABLE IF NOT EXISTS entities (
            id TEXT PRIMARY KEY,
            label TEXT NOT NULL,
            created_graph_version INTEGER NOT NULL
        );

        CREATE TABLE IF NOT EXISTS evidence (
            id TEXT PRIMARY KEY,
            source_scope TEXT NOT NULL,
            source_path TEXT,
            span_start_byte INTEGER,
            span_end_byte INTEGER,
            span_start_line INTEGER,
            span_end_line INTEGER,
            content TEXT NOT NULL,
            confidence_basis_points INTEGER NOT NULL DEFAULT 10000,
            status TEXT NOT NULL DEFAULT 'accepted',
            created_graph_version INTEGER NOT NULL
        );

        CREATE TABLE IF NOT EXISTS evidence_entities (
            evidence_id TEXT NOT NULL,
            entity_id TEXT NOT NULL,
            PRIMARY KEY (evidence_id, entity_id),
            FOREIGN KEY (evidence_id) REFERENCES evidence(id) ON DELETE CASCADE,
            FOREIGN KEY (entity_id) REFERENCES entities(id) ON DELETE CASCADE
        );

        CREATE TABLE IF NOT EXISTS graph_mutations (
            graph_version INTEGER PRIMARY KEY,
            evidence_count INTEGER NOT NULL,
            entity_count INTEGER NOT NULL,
            relation_count INTEGER NOT NULL DEFAULT 0,
            claim_count INTEGER NOT NULL DEFAULT 0,
            event_count INTEGER NOT NULL DEFAULT 0
        );

        CREATE TABLE IF NOT EXISTS graph_relations (
            id TEXT PRIMARY KEY,
            source_entity_id TEXT NOT NULL,
            relation_type TEXT NOT NULL,
            target_entity_id TEXT NOT NULL,
            evidence_ids_json TEXT NOT NULL,
            confidence_basis_points INTEGER NOT NULL,
            status TEXT NOT NULL,
            valid_from_graph_version INTEGER NOT NULL,
            valid_until_graph_version INTEGER,
            created_graph_version INTEGER NOT NULL,
            FOREIGN KEY (source_entity_id) REFERENCES entities(id),
            FOREIGN KEY (target_entity_id) REFERENCES entities(id)
        );

        CREATE TABLE IF NOT EXISTS graph_claims (
            id TEXT PRIMARY KEY,
            subject_entity_id TEXT NOT NULL,
            predicate TEXT NOT NULL,
            object TEXT NOT NULL,
            evidence_ids_json TEXT NOT NULL,
            confidence_basis_points INTEGER NOT NULL,
            status TEXT NOT NULL,
            valid_from_graph_version INTEGER NOT NULL,
            valid_until_graph_version INTEGER,
            created_graph_version INTEGER NOT NULL,
            FOREIGN KEY (subject_entity_id) REFERENCES entities(id)
        );

        CREATE TABLE IF NOT EXISTS graph_events (
            id TEXT PRIMARY KEY,
            event_type TEXT NOT NULL,
            occurred_at TEXT,
            evidence_ids_json TEXT NOT NULL,
            confidence_basis_points INTEGER NOT NULL,
            status TEXT NOT NULL,
            valid_from_graph_version INTEGER NOT NULL,
            valid_until_graph_version INTEGER,
            created_graph_version INTEGER NOT NULL
        );

        CREATE TABLE IF NOT EXISTS graph_event_entities (
            event_id TEXT NOT NULL,
            entity_id TEXT NOT NULL,
            PRIMARY KEY (event_id, entity_id),
            FOREIGN KEY (event_id) REFERENCES graph_events(id) ON DELETE CASCADE,
            FOREIGN KEY (entity_id) REFERENCES entities(id)
        );

        CREATE TABLE IF NOT EXISTS index_status (
            kind TEXT PRIMARY KEY,
            index_version INTEGER NOT NULL,
            indexed_graph_version INTEGER NOT NULL,
            state TEXT NOT NULL,
            last_error TEXT
        );
        ",
    )?;
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
    retrieval::initialize_schema(connection)?;
    code::initialize_code_schema(connection)?;

    for kind in IndexKind::ALL {
        connection.execute(
            "INSERT OR IGNORE INTO index_status
             (kind, index_version, indexed_graph_version, state, last_error)
             VALUES (?1, 0, 0, 'fresh', NULL)",
            params![kind.as_str()],
        )?;
    }
    code_graph::initialize_schema(connection)?;

    Ok(())
}

fn ensure_column(
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

fn commit_batch(
    connection: &mut Connection,
    batch: GraphMutationBatch,
) -> Result<CommitReceipt, StorageError> {
    let transaction = connection.transaction()?;
    let current = current_graph_version_in_transaction(&transaction)?;
    let next = GraphVersion::new(current.get() + 1);
    let evidence_count = batch.evidence.len();
    let relation_count = batch.relations.len();
    let claim_count = batch.claims.len();
    let event_count = batch.events.len();
    let mut affected_entity_ids = BTreeSet::new();

    for evidence in batch.evidence {
        let evidence_id = evidence.id;
        let source_scope = evidence.source_scope;
        let source_path = evidence.source_path;
        let span = evidence.span;
        let content = evidence.content;
        let entity_labels = evidence.entity_labels;
        transaction.execute(
            "INSERT INTO evidence (
                 id, source_scope, source_path, span_start_byte, span_end_byte,
                 span_start_line, span_end_line, content, confidence_basis_points,
                 status, created_graph_version
             )
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
             ON CONFLICT(id) DO UPDATE SET
                 source_scope = excluded.source_scope,
                 source_path = excluded.source_path,
                 span_start_byte = excluded.span_start_byte,
                 span_end_byte = excluded.span_end_byte,
                 span_start_line = excluded.span_start_line,
                 span_end_line = excluded.span_end_line,
                 content = excluded.content,
                 confidence_basis_points = excluded.confidence_basis_points,
                 status = excluded.status,
                 created_graph_version = excluded.created_graph_version",
            params![
                evidence_id,
                source_scope.as_str(),
                source_path.as_deref(),
                span.map(|value| value.start_byte),
                span.map(|value| value.end_byte),
                span.map(|value| value.start_line),
                span.map(|value| value.end_line),
                content,
                evidence.confidence.basis_points,
                evidence.status.as_str(),
                next.get()
            ],
        )?;

        transaction.execute(
            "DELETE FROM evidence_entities WHERE evidence_id = ?1",
            params![evidence_id],
        )?;

        for label in &entity_labels {
            let entity_id = upsert_entity(&transaction, label, next)?;
            transaction.execute(
                "INSERT OR IGNORE INTO evidence_entities (evidence_id, entity_id)
                 VALUES (?1, ?2)",
                params![evidence_id, entity_id],
            )?;
            affected_entity_ids.insert(entity_id);
        }
        retrieval::replace_evidence_document(
            &transaction,
            &evidence_id,
            source_scope.as_str(),
            source_path.as_deref(),
            &entity_labels,
            &content,
            next.get(),
        )?;
    }

    for relation in batch.relations {
        let source_entity_id = upsert_entity(&transaction, &relation.source_entity_label, next)?;
        let target_entity_id = upsert_entity(&transaction, &relation.target_entity_label, next)?;
        affected_entity_ids.insert(source_entity_id.clone());
        affected_entity_ids.insert(target_entity_id.clone());
        transaction.execute(
            "
            INSERT INTO graph_relations (
                id, source_entity_id, relation_type, target_entity_id,
                evidence_ids_json, confidence_basis_points, status,
                valid_from_graph_version, valid_until_graph_version, created_graph_version
            )
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
            ON CONFLICT(id) DO UPDATE SET
                source_entity_id = excluded.source_entity_id,
                relation_type = excluded.relation_type,
                target_entity_id = excluded.target_entity_id,
                evidence_ids_json = excluded.evidence_ids_json,
                confidence_basis_points = excluded.confidence_basis_points,
                status = excluded.status,
                valid_from_graph_version = excluded.valid_from_graph_version,
                valid_until_graph_version = excluded.valid_until_graph_version,
                created_graph_version = excluded.created_graph_version
            ",
            params![
                relation.id,
                source_entity_id,
                relation.relation_type,
                target_entity_id,
                evidence_ids_json(&relation.evidence_ids)?,
                relation.confidence.basis_points,
                relation.status.as_str(),
                relation.version_range.valid_from.get(),
                relation.version_range.valid_until.map(GraphVersion::get),
                next.get(),
            ],
        )?;
    }

    for claim in batch.claims {
        let subject_entity_id = upsert_entity(&transaction, &claim.subject_entity_label, next)?;
        affected_entity_ids.insert(subject_entity_id.clone());
        transaction.execute(
            "
            INSERT INTO graph_claims (
                id, subject_entity_id, predicate, object, evidence_ids_json,
                confidence_basis_points, status, valid_from_graph_version,
                valid_until_graph_version, created_graph_version
            )
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
            ON CONFLICT(id) DO UPDATE SET
                subject_entity_id = excluded.subject_entity_id,
                predicate = excluded.predicate,
                object = excluded.object,
                evidence_ids_json = excluded.evidence_ids_json,
                confidence_basis_points = excluded.confidence_basis_points,
                status = excluded.status,
                valid_from_graph_version = excluded.valid_from_graph_version,
                valid_until_graph_version = excluded.valid_until_graph_version,
                created_graph_version = excluded.created_graph_version
            ",
            params![
                claim.id,
                subject_entity_id,
                claim.predicate,
                claim.object,
                evidence_ids_json(&claim.evidence_ids)?,
                claim.confidence.basis_points,
                claim.status.as_str(),
                claim.version_range.valid_from.get(),
                claim.version_range.valid_until.map(GraphVersion::get),
                next.get(),
            ],
        )?;
    }

    for event in batch.events {
        transaction.execute(
            "
            INSERT INTO graph_events (
                id, event_type, occurred_at, evidence_ids_json,
                confidence_basis_points, status, valid_from_graph_version,
                valid_until_graph_version, created_graph_version
            )
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
            ON CONFLICT(id) DO UPDATE SET
                event_type = excluded.event_type,
                occurred_at = excluded.occurred_at,
                evidence_ids_json = excluded.evidence_ids_json,
                confidence_basis_points = excluded.confidence_basis_points,
                status = excluded.status,
                valid_from_graph_version = excluded.valid_from_graph_version,
                valid_until_graph_version = excluded.valid_until_graph_version,
                created_graph_version = excluded.created_graph_version
            ",
            params![
                event.id,
                event.event_type,
                event.occurred_at,
                evidence_ids_json(&event.evidence_ids)?,
                event.confidence.basis_points,
                event.status.as_str(),
                event.version_range.valid_from.get(),
                event.version_range.valid_until.map(GraphVersion::get),
                next.get(),
            ],
        )?;
        transaction.execute(
            "DELETE FROM graph_event_entities WHERE event_id = ?1",
            params![event.id],
        )?;
        for label in event.entity_labels {
            let entity_id = upsert_entity(&transaction, &label, next)?;
            affected_entity_ids.insert(entity_id.clone());
            transaction.execute(
                "INSERT OR IGNORE INTO graph_event_entities (event_id, entity_id)
                 VALUES (?1, ?2)",
                params![event.id, entity_id],
            )?;
        }
    }

    transaction.execute(
        "
        DELETE FROM entities
        WHERE id NOT IN (SELECT entity_id FROM evidence_entities)
          AND id NOT IN (SELECT source_entity_id FROM graph_relations)
          AND id NOT IN (SELECT target_entity_id FROM graph_relations)
          AND id NOT IN (SELECT subject_entity_id FROM graph_claims)
          AND id NOT IN (SELECT entity_id FROM graph_event_entities)
        ",
        [],
    )?;

    let entity_count = affected_entity_ids.len();
    transaction.execute(
        "INSERT INTO graph_mutations (
             graph_version, evidence_count, entity_count, relation_count, claim_count, event_count
         )
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            next.get(),
            evidence_count,
            entity_count,
            relation_count,
            claim_count,
            event_count
        ],
    )?;
    transaction.execute(
        "UPDATE graph_state SET graph_version = ?1 WHERE id = 1",
        params![next.get()],
    )?;
    transaction.execute("UPDATE index_status SET state = 'stale'", [])?;
    transaction.commit()?;

    Ok(CommitReceipt {
        graph_version: next,
        evidence_count,
        entity_count,
        relation_count,
        claim_count,
        event_count,
    })
}

fn inspect_graph(connection: &mut Connection) -> Result<GraphInspection, StorageError> {
    Ok(GraphInspection {
        graph_version: current_graph_version(connection)?,
        entity_count: count_rows(connection, "entities")?,
        evidence_count: count_rows(connection, "evidence")?,
        relation_count: count_rows(connection, "graph_relations")?,
        claim_count: count_rows(connection, "graph_claims")?,
        event_count: count_rows(connection, "graph_events")?,
        mutation_count: count_rows(connection, "graph_mutations")?,
        code_file_count: count_rows(connection, "code_files")?,
        code_symbol_count: count_rows(connection, "code_symbols")?,
        code_reference_count: count_rows(connection, "code_references")?,
        code_chunk_count: count_rows(connection, "code_chunks")?,
        code_parse_status_counts: code_graph::parse_status_counts(connection)?,
    })
}

fn current_graph_version(connection: &mut Connection) -> Result<GraphVersion, StorageError> {
    current_graph_version_in_transaction(connection)
}

fn current_graph_version_in_transaction(
    connection: &Connection,
) -> Result<GraphVersion, StorageError> {
    let value = connection.query_row(
        "SELECT graph_version FROM graph_state WHERE id = 1",
        [],
        |row| row.get::<_, u64>(0),
    )?;

    Ok(GraphVersion::new(value))
}

fn upsert_entity(
    transaction: &rusqlite::Transaction<'_>,
    label: &str,
    graph_version: GraphVersion,
) -> Result<String, StorageError> {
    let entity_id = stable_id("entity", label);
    transaction.execute(
        "INSERT OR IGNORE INTO entities (id, label, created_graph_version)
         VALUES (?1, ?2, ?3)",
        params![entity_id, label, graph_version.get()],
    )?;

    Ok(entity_id)
}

fn evidence_ids_json(evidence_ids: &[String]) -> Result<String, StorageError> {
    serde_json::to_string(evidence_ids)
        .map_err(|error| StorageError::InvalidInput(error.to_string()))
}

fn count_rows(connection: &Connection, table: &'static str) -> Result<usize, StorageError> {
    let sql = format!("SELECT COUNT(*) FROM {table}");
    let count = connection.query_row(&sql, [], |row| row.get::<_, usize>(0))?;

    Ok(count)
}

fn read_mutations_after(
    connection: &mut Connection,
    graph_version: GraphVersion,
    limit: usize,
) -> Result<Vec<MutationLogEntry>, StorageError> {
    if limit == 0 {
        return Err(StorageError::InvalidInput(
            "mutation log limit must be greater than zero".to_owned(),
        ));
    }

    let mut statement = connection.prepare(
        "
        SELECT graph_version, evidence_count, entity_count,
               relation_count, claim_count, event_count
        FROM graph_mutations
        WHERE graph_version > ?1
        ORDER BY graph_version ASC
        LIMIT ?2
        ",
    )?;
    let rows = statement.query_map(params![graph_version.get(), limit], |row| {
        Ok(MutationLogEntry {
            graph_version: GraphVersion::new(row.get(0)?),
            evidence_count: row.get(1)?,
            entity_count: row.get(2)?,
            relation_count: row.get(3)?,
            claim_count: row.get(4)?,
            event_count: row.get(5)?,
        })
    })?;

    rows.collect::<Result<Vec<_>, _>>()
        .map_err(StorageError::from)
}

fn index_statuses(connection: &mut Connection) -> Result<Vec<IndexStatus>, StorageError> {
    let mut statement = connection.prepare(
        "
        SELECT kind, index_version, indexed_graph_version, state, last_error
        FROM index_status
        ORDER BY kind ASC
        ",
    )?;
    let rows = statement.query_map([], |row| {
        let state: String = row.get(3)?;
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, u64>(1)?,
            row.get::<_, u64>(2)?,
            state,
            row.get::<_, Option<String>>(4)?,
        ))
    })?;
    let raw_statuses = rows
        .collect::<Result<Vec<_>, _>>()
        .map_err(StorageError::from)?;

    let statuses = raw_statuses
        .into_iter()
        .map(
            |(kind, index_version, indexed_graph_version, state, last_error)| {
                Ok(IndexStatus {
                    kind: parse_index_kind(&kind)?,
                    index_version,
                    indexed_graph_version: GraphVersion::new(indexed_graph_version),
                    state: parse_index_state(&state)?,
                    last_error,
                })
            },
        )
        .collect::<Result<Vec<_>, StorageError>>()?;
    validate_required_index_statuses(&statuses)?;

    Ok(statuses)
}

fn mark_refresh_complete(
    connection: &mut Connection,
    kind: IndexKind,
    graph_version: GraphVersion,
) -> Result<IndexStatus, StorageError> {
    let Some(current) = read_index_status(connection, kind)? else {
        return Err(StorageError::InvalidInput(format!(
            "index status row for '{}' is missing",
            kind.as_str()
        )));
    };
    if current.indexed_graph_version > graph_version {
        return Ok(current);
    }

    let updated = connection.execute(
        "
        UPDATE index_status
        SET index_version = index_version + 1,
            indexed_graph_version = ?2,
            state = 'fresh',
            last_error = NULL
        WHERE kind = ?1
        ",
        params![kind.as_str(), graph_version.get()],
    )?;
    if updated != 1 {
        return Err(StorageError::InvalidInput(format!(
            "index status row for '{}' was not updated",
            kind.as_str()
        )));
    }

    read_index_status(connection, kind)?.ok_or_else(|| {
        StorageError::InvalidInput(format!(
            "index status row for '{}' is missing",
            kind.as_str()
        ))
    })
}

fn read_index_status(
    connection: &Connection,
    kind: IndexKind,
) -> Result<Option<IndexStatus>, StorageError> {
    let raw_status = connection
        .query_row(
            "
            SELECT index_version, indexed_graph_version, state, last_error
            FROM index_status
            WHERE kind = ?1
            ",
            params![kind.as_str()],
            |row| {
                let state: String = row.get(2)?;
                Ok((
                    row.get::<_, u64>(0)?,
                    row.get::<_, u64>(1)?,
                    state,
                    row.get::<_, Option<String>>(3)?,
                ))
            },
        )
        .optional()
        .map_err(StorageError::from)?;

    raw_status
        .map(
            |(index_version, indexed_graph_version, state, last_error)| {
                Ok(IndexStatus {
                    kind,
                    index_version,
                    indexed_graph_version: GraphVersion::new(indexed_graph_version),
                    state: parse_index_state(&state)?,
                    last_error,
                })
            },
        )
        .transpose()
}

fn parse_index_kind(value: &str) -> Result<IndexKind, StorageError> {
    match value {
        "bm25" => Ok(IndexKind::Bm25),
        "semantic" => Ok(IndexKind::Semantic),
        "vector" => Ok(IndexKind::Vector),
        _ => Err(invalid_index_metadata(format!(
            "unknown index kind '{value}'"
        ))),
    }
}

fn parse_index_state(value: &str) -> Result<IndexState, StorageError> {
    match value {
        "fresh" => Ok(IndexState::Fresh),
        "stale" => Ok(IndexState::Stale),
        "failed" => Ok(IndexState::Failed),
        "paused" => Ok(IndexState::Paused),
        _ => Err(invalid_index_metadata(format!(
            "unknown index state '{value}'"
        ))),
    }
}

fn validate_required_index_statuses(statuses: &[IndexStatus]) -> Result<(), StorageError> {
    for kind in IndexKind::ALL {
        if !statuses.iter().any(|status| status.kind == kind) {
            return Err(invalid_index_metadata(format!(
                "required index status row for '{}' is missing",
                kind.as_str()
            )));
        }
    }

    Ok(())
}

fn invalid_index_metadata(message: String) -> StorageError {
    StorageError::InvalidInput(format!("{message} in storage metadata"))
}

fn stable_id(prefix: &str, value: &str) -> String {
    let normalized = value.to_lowercase();

    format!("{prefix}:{:016x}", stable_hash64(normalized.as_bytes()))
}

fn stable_hash64(bytes: &[u8]) -> u64 {
    const FNV_OFFSET_BASIS: u64 = 0xcbf29ce484222325;
    const FNV_PRIME: u64 = 0x100000001b3;

    let mut hash = FNV_OFFSET_BASIS;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(FNV_PRIME);
    }

    hash
}

#[cfg(test)]
mod metadata_tests;

#[cfg(test)]
mod graph_tests;
