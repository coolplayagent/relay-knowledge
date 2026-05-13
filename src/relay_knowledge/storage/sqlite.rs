use std::{
    collections::BTreeSet,
    path::Path,
    sync::{Arc, Mutex},
};

mod code;

use rusqlite::{Connection, OptionalExtension, params};

mod code_graph;
mod indexing;
mod retrieval;

use crate::{
    domain::{
        CodeChunkRecord, CodeGraphBatch, CodeGraphCommitReceipt, CodeReferenceRecord,
        CodeSymbolRecord, CommitReceipt, GraphMutationBatch, GraphVersion, IndexKind, IndexStatus,
        RetrievalHit,
    },
    storage::{
        CodeChunkSearchRequest, CodeGraphStore, CodeReferenceSearchRequest,
        CodeSymbolSearchRequest, GraphInspection, GraphSearchRequest, GraphStore, IndexCursor,
        IndexRefreshClaimRequest, IndexRefreshCompletion, IndexRefreshDiagnostics,
        IndexRefreshFailure, IndexRefreshQueueRequest, IndexRefreshTask, IndexStore,
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
        self.run(|connection| indexing::index_statuses(connection))
    }

    fn mark_refresh_complete(
        &self,
        kind: IndexKind,
        graph_version: GraphVersion,
    ) -> StorageFuture<'_, IndexStatus> {
        self.run(move |connection| indexing::mark_refresh_complete(connection, kind, graph_version))
    }

    fn index_cursors(&self) -> StorageFuture<'_, Vec<IndexCursor>> {
        self.run(indexing::index_cursors)
    }

    fn queue_index_refreshes(
        &self,
        request: IndexRefreshQueueRequest,
    ) -> StorageFuture<'_, IndexRefreshDiagnostics> {
        self.run(move |connection| indexing::queue_index_refreshes(connection, request))
    }

    fn claim_index_refresh_task(
        &self,
        request: IndexRefreshClaimRequest,
    ) -> StorageFuture<'_, Option<IndexRefreshTask>> {
        self.run(move |connection| indexing::claim_index_refresh_task(connection, request))
    }

    fn complete_index_refresh_task(
        &self,
        request: IndexRefreshCompletion,
    ) -> StorageFuture<'_, IndexRefreshTask> {
        self.run(move |connection| indexing::complete_index_refresh_task(connection, request))
    }

    fn fail_index_refresh_task(
        &self,
        request: IndexRefreshFailure,
    ) -> StorageFuture<'_, IndexRefreshTask> {
        self.run(move |connection| indexing::fail_index_refresh_task(connection, request))
    }

    fn index_refresh_diagnostics(&self, now_ms: u64) -> StorageFuture<'_, IndexRefreshDiagnostics> {
        self.run(move |connection| indexing::diagnostics(connection, now_ms))
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
    indexing::initialize_schema(connection)?;
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
    let mut affected_scopes = BTreeSet::new();
    let mut evidence_ids = BTreeSet::new();
    let mut source_hashes = BTreeSet::new();

    for evidence in batch.evidence {
        let evidence_id = evidence.id;
        let source_scope = evidence.source_scope;
        let source_scope_text = source_scope.as_str().to_owned();
        let source_path = evidence.source_path;
        let span = evidence.span;
        let content = evidence.content;
        let entity_labels = evidence.entity_labels;
        if let Some(previous_scope) = evidence_scope(&transaction, &evidence_id)? {
            affected_scopes.insert(previous_scope);
        }
        affected_scopes.insert(source_scope_text.clone());
        evidence_ids.insert(evidence_id.clone());
        source_hashes.insert(indexing::source_hash(
            &source_scope_text,
            source_path.as_deref(),
            &content,
        ));
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
                &evidence_id,
                &source_scope_text,
                source_path.as_deref(),
                span.map(|value| value.start_byte),
                span.map(|value| value.end_byte),
                span.map(|value| value.start_line),
                span.map(|value| value.end_line),
                &content,
                evidence.confidence.basis_points,
                evidence.status.as_str(),
                next.get()
            ],
        )?;

        transaction.execute(
            "DELETE FROM evidence_entities WHERE evidence_id = ?1",
            params![&evidence_id],
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
            &source_scope_text,
            source_path.as_deref(),
            &entity_labels,
            &content,
            next.get(),
        )?;
    }

    for relation in batch.relations {
        evidence_ids.extend(relation.evidence_ids.iter().cloned());
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
        evidence_ids.extend(claim.evidence_ids.iter().cloned());
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
        evidence_ids.extend(event.evidence_ids.iter().cloned());
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
    add_scopes_for_evidence_ids(&transaction, &evidence_ids, &mut affected_scopes)?;
    if affected_scopes.is_empty()
        && (evidence_count > 0 || relation_count > 0 || claim_count > 0 || event_count > 0)
    {
        affected_scopes.insert(indexing::DEFAULT_SCOPE.to_owned());
    }
    let affected_scopes = affected_scopes.into_iter().collect::<Vec<_>>();
    let affected_entity_ids = affected_entity_ids.into_iter().collect::<Vec<_>>();
    let affected_scopes_json = indexing::json_array(affected_scopes.clone())?;
    let affected_entity_ids_json = indexing::json_array(affected_entity_ids.clone())?;
    let evidence_ids_json = indexing::json_array(evidence_ids)?;
    let source_hashes_json = indexing::json_array(source_hashes)?;
    transaction.execute(
        "INSERT INTO graph_mutations (
             graph_version, evidence_count, entity_count, relation_count, claim_count, event_count,
             affected_scopes_json, affected_entity_ids_json, evidence_ids_json, source_hashes_json
         )
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        params![
            next.get(),
            evidence_count,
            entity_count,
            relation_count,
            claim_count,
            event_count,
            affected_scopes_json,
            affected_entity_ids_json,
            evidence_ids_json,
            source_hashes_json
        ],
    )?;
    indexing::mark_mutation_cursors_stale(&transaction, &affected_scopes)?;
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

fn add_scopes_for_evidence_ids(
    connection: &Connection,
    evidence_ids: &BTreeSet<String>,
    affected_scopes: &mut BTreeSet<String>,
) -> Result<(), StorageError> {
    for evidence_id in evidence_ids {
        if let Some(scope) = evidence_scope(connection, evidence_id)? {
            affected_scopes.insert(scope);
        }
    }

    Ok(())
}

fn evidence_scope(
    connection: &Connection,
    evidence_id: &str,
) -> Result<Option<String>, StorageError> {
    connection
        .query_row(
            "SELECT source_scope FROM evidence WHERE id = ?1",
            params![evidence_id],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .map_err(StorageError::from)
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
               relation_count, claim_count, event_count,
               affected_scopes_json, affected_entity_ids_json,
               evidence_ids_json, source_hashes_json
        FROM graph_mutations
        WHERE graph_version > ?1
        ORDER BY graph_version ASC
        LIMIT ?2
        ",
    )?;
    let rows = statement.query_map(params![graph_version.get(), limit], |row| {
        Ok((
            row.get::<_, u64>(0)?,
            row.get::<_, usize>(1)?,
            row.get::<_, usize>(2)?,
            row.get::<_, usize>(3)?,
            row.get::<_, usize>(4)?,
            row.get::<_, usize>(5)?,
            row.get::<_, String>(6)?,
            row.get::<_, String>(7)?,
            row.get::<_, String>(8)?,
            row.get::<_, String>(9)?,
        ))
    })?;
    rows.collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .map(
            |(
                graph_version,
                evidence_count,
                entity_count,
                relation_count,
                claim_count,
                event_count,
                affected_scopes,
                affected_entity_ids,
                evidence_ids,
                source_hashes,
            )| {
                Ok(MutationLogEntry {
                    graph_version: GraphVersion::new(graph_version),
                    evidence_count,
                    entity_count,
                    relation_count,
                    claim_count,
                    event_count,
                    affected_scopes: indexing::parse_json_array(affected_scopes)?,
                    affected_entity_ids: indexing::parse_json_array(affected_entity_ids)?,
                    evidence_ids: indexing::parse_json_array(evidence_ids)?,
                    source_hashes: indexing::parse_json_array(source_hashes)?,
                })
            },
        )
        .collect()
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

#[cfg(test)]
mod index_refresh_tests;
