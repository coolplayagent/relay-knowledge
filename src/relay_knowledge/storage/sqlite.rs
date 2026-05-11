use std::{
    collections::BTreeSet,
    path::Path,
    sync::{Arc, Mutex},
};

use rusqlite::{Connection, OptionalExtension, params};

use crate::{
    domain::{
        CommitReceipt, GraphMutationBatch, GraphVersion, IndexKind, IndexState, IndexStatus,
        RetrievalHit,
    },
    storage::{
        GraphInspection, GraphSearchRequest, GraphStore, IndexStore, MutationLogEntry,
        MutationLogStore, StorageError, StorageFuture,
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
        self.run(move |connection| search_graph(connection, request))
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
            content TEXT NOT NULL,
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
            entity_count INTEGER NOT NULL
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

    for kind in IndexKind::ALL {
        connection.execute(
            "INSERT OR IGNORE INTO index_status
             (kind, index_version, indexed_graph_version, state, last_error)
             VALUES (?1, 0, 0, 'fresh', NULL)",
            params![kind.as_str()],
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
    let mut affected_entity_ids = BTreeSet::new();

    for evidence in batch.evidence {
        transaction.execute(
            "INSERT INTO evidence (id, source_scope, content, created_graph_version)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(id) DO UPDATE SET
                 source_scope = excluded.source_scope,
                 content = excluded.content,
                 created_graph_version = excluded.created_graph_version",
            params![
                evidence.id,
                evidence.source_scope.as_str(),
                evidence.content,
                next.get()
            ],
        )?;

        transaction.execute(
            "DELETE FROM evidence_entities WHERE evidence_id = ?1",
            params![evidence.id],
        )?;

        for label in evidence.entity_labels {
            let entity_id = stable_id("entity", &label);
            transaction.execute(
                "INSERT OR IGNORE INTO entities (id, label, created_graph_version)
                 VALUES (?1, ?2, ?3)",
                params![entity_id, label, next.get()],
            )?;
            transaction.execute(
                "INSERT OR IGNORE INTO evidence_entities (evidence_id, entity_id)
                 VALUES (?1, ?2)",
                params![evidence.id, entity_id],
            )?;
            affected_entity_ids.insert(entity_id);
        }
    }

    transaction.execute(
        "DELETE FROM entities
         WHERE id NOT IN (SELECT entity_id FROM evidence_entities)",
        [],
    )?;

    let entity_count = affected_entity_ids.len();
    transaction.execute(
        "INSERT INTO graph_mutations (graph_version, evidence_count, entity_count)
         VALUES (?1, ?2, ?3)",
        params![next.get(), evidence_count, entity_count],
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
    })
}

fn inspect_graph(connection: &mut Connection) -> Result<GraphInspection, StorageError> {
    Ok(GraphInspection {
        graph_version: current_graph_version(connection)?,
        entity_count: count_rows(connection, "entities")?,
        evidence_count: count_rows(connection, "evidence")?,
        mutation_count: count_rows(connection, "graph_mutations")?,
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

fn count_rows(connection: &Connection, table: &'static str) -> Result<usize, StorageError> {
    let sql = format!("SELECT COUNT(*) FROM {table}");
    let count = connection.query_row(&sql, [], |row| row.get::<_, usize>(0))?;

    Ok(count)
}

fn search_graph(
    connection: &mut Connection,
    request: GraphSearchRequest,
) -> Result<Vec<RetrievalHit>, StorageError> {
    if request.limit == 0 {
        return Err(StorageError::InvalidInput(
            "search limit must be greater than zero".to_owned(),
        ));
    }

    let mut statement = connection.prepare(
        "
        SELECT
            e.id,
            e.source_scope,
            e.content
        FROM evidence e
        WHERE (?1 IS NULL OR e.source_scope = ?1)
          AND e.created_graph_version <= ?2
        ORDER BY e.created_graph_version DESC, e.id ASC
        ",
    )?;
    let scope = request.source_scope.as_deref();
    let rows = statement.query_map(params![scope, request.graph_version.get()], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
        ))
    })?;
    let evidence_rows = rows
        .collect::<Result<Vec<_>, _>>()
        .map_err(StorageError::from)?;
    drop(statement);

    let mut hits = Vec::new();
    for (evidence_id, source_scope, content) in evidence_rows {
        let mut hit = RetrievalHit {
            entity_labels: entity_labels_for_evidence(connection, &evidence_id)?,
            evidence_id,
            source_scope,
            content,
            score: 0.0,
        };
        hit.score = score_hit(&request.query, &hit);
        if hit.score > 0.0 {
            hits.push(hit);
        }
    }

    hits.sort_by(|left, right| {
        right
            .score
            .total_cmp(&left.score)
            .then_with(|| left.evidence_id.cmp(&right.evidence_id))
    });
    hits.truncate(request.limit);

    Ok(hits)
}

fn entity_labels_for_evidence(
    connection: &Connection,
    evidence_id: &str,
) -> Result<Vec<String>, StorageError> {
    let mut statement = connection.prepare(
        "
        SELECT ent.label
        FROM evidence_entities ee
        INNER JOIN entities ent ON ent.id = ee.entity_id
        WHERE ee.evidence_id = ?1
        ORDER BY ent.label ASC, ent.id ASC
        ",
    )?;
    let rows = statement.query_map(params![evidence_id], |row| row.get::<_, String>(0))?;

    rows.collect::<Result<Vec<_>, _>>()
        .map_err(StorageError::from)
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
        SELECT graph_version, evidence_count, entity_count
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

fn score_hit(query: &str, hit: &RetrievalHit) -> f64 {
    let haystack = format!(
        "{} {}",
        hit.content.to_lowercase(),
        hit.entity_labels.join(" ").to_lowercase()
    );
    let mut score = 0.0;
    for token in query.to_lowercase().split_whitespace() {
        if haystack.contains(token) {
            score += 1.0;
        }
    }

    score
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
mod tests {
    use super::*;
    use crate::domain::{EvidenceRecord, SourceScope};

    #[tokio::test]
    async fn commits_graph_batch_and_marks_indexes_stale() {
        let store = SqliteGraphStore::open_in_memory().expect("store should open");
        let scope = SourceScope::parse("repo").expect("scope should parse");
        let evidence = EvidenceRecord::new(
            "ev-1",
            scope,
            "Rust uses ownership",
            vec!["Rust".to_owned()],
        )
        .expect("evidence should validate");
        let batch = GraphMutationBatch::new(vec![evidence]).expect("batch should validate");

        let receipt = store
            .commit_mutation_batch(batch)
            .await
            .expect("commit should succeed");
        let inspection = store.inspect_graph().await.expect("inspection should load");
        let statuses = store.index_statuses().await.expect("statuses should load");

        assert_eq!(receipt.graph_version, GraphVersion::new(1));
        assert_eq!(inspection.entity_count, 1);
        assert_eq!(inspection.evidence_count, 1);
        assert!(
            statuses
                .iter()
                .all(|status| status.is_stale_for(GraphVersion::new(1)))
        );
    }

    #[tokio::test]
    async fn searches_evidence_by_query_token() {
        let store = SqliteGraphStore::open_in_memory().expect("store should open");
        let scope = SourceScope::parse("docs").expect("scope should parse");
        let evidence = EvidenceRecord::new("ev-1", scope, "Hybrid retrieval uses BM25", Vec::new())
            .expect("evidence should validate");
        let batch = GraphMutationBatch::new(vec![evidence]).expect("batch should validate");
        store
            .commit_mutation_batch(batch)
            .await
            .expect("commit should succeed");

        let hits = store
            .search(GraphSearchRequest {
                query: "BM25".to_owned(),
                source_scope: None,
                graph_version: GraphVersion::new(1),
                limit: 5,
            })
            .await
            .expect("search should succeed");

        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].evidence_id, "ev-1");
    }

    #[tokio::test]
    async fn marks_index_refresh_complete_at_graph_version() {
        let store = SqliteGraphStore::open_in_memory().expect("store should open");

        let status = store
            .mark_refresh_complete(IndexKind::Vector, GraphVersion::new(7))
            .await
            .expect("refresh should update metadata");

        assert_eq!(status.kind, IndexKind::Vector);
        assert_eq!(status.index_version, 1);
        assert_eq!(status.indexed_graph_version, GraphVersion::new(7));
        assert_eq!(status.state, IndexState::Fresh);
    }

    #[tokio::test]
    async fn reads_mutation_log_after_version() {
        let store = SqliteGraphStore::open_in_memory().expect("store should open");
        commit_evidence(&store, "ev-1", "docs", "Rust async storage").await;
        commit_evidence(&store, "ev-2", "docs", "SQLite graph storage").await;

        let entries = store
            .read_after(GraphVersion::new(1), 10)
            .await
            .expect("mutation log should load");

        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].graph_version, GraphVersion::new(2));
        assert_eq!(entries[0].evidence_count, 1);
    }

    #[tokio::test]
    async fn commit_receipt_counts_unique_affected_entities() {
        let store = SqliteGraphStore::open_in_memory().expect("store should open");
        let scope = SourceScope::parse("docs").expect("scope should parse");
        let first = EvidenceRecord::new(
            "ev-1",
            scope.clone(),
            "Rust async storage",
            vec!["Rust".to_owned()],
        )
        .expect("evidence should validate");
        let second = EvidenceRecord::new(
            "ev-2",
            scope,
            "Rust graph retrieval",
            vec!["rust".to_owned()],
        )
        .expect("evidence should validate");
        let batch = GraphMutationBatch::new(vec![first, second]).expect("batch should validate");

        let receipt = store
            .commit_mutation_batch(batch)
            .await
            .expect("commit should succeed");
        let entries = store
            .read_after(GraphVersion::ZERO, 10)
            .await
            .expect("mutation log should load");

        assert_eq!(receipt.entity_count, 1);
        assert_eq!(entries[0].entity_count, 1);
    }

    #[tokio::test]
    async fn rejects_zero_limits_for_search_and_mutation_log() {
        let store = SqliteGraphStore::open_in_memory().expect("store should open");

        let search_error = store
            .search(GraphSearchRequest {
                query: "Rust".to_owned(),
                source_scope: None,
                graph_version: GraphVersion::ZERO,
                limit: 0,
            })
            .await
            .expect_err("zero search limit should fail");
        let log_error = store
            .read_after(GraphVersion::ZERO, 0)
            .await
            .expect_err("zero log limit should fail");

        assert_eq!(
            search_error.to_string(),
            "invalid storage input: search limit must be greater than zero"
        );
        assert_eq!(
            log_error.to_string(),
            "invalid storage input: mutation log limit must be greater than zero"
        );
    }

    #[tokio::test]
    async fn search_filters_by_source_scope_and_sorts_by_score() {
        let store = SqliteGraphStore::open_in_memory().expect("store should open");
        commit_evidence(&store, "ev-1", "docs", "Rust Rust SQLite").await;
        commit_evidence(&store, "ev-2", "repo", "Rust").await;

        let docs = store
            .search(GraphSearchRequest {
                query: "Rust SQLite".to_owned(),
                source_scope: Some("docs".to_owned()),
                graph_version: GraphVersion::new(2),
                limit: 5,
            })
            .await
            .expect("search should succeed");
        let all = store
            .search(GraphSearchRequest {
                query: "Rust".to_owned(),
                source_scope: None,
                graph_version: GraphVersion::new(2),
                limit: 5,
            })
            .await
            .expect("search should succeed");

        assert_eq!(docs.len(), 1);
        assert_eq!(docs[0].source_scope, "docs");
        assert_eq!(all.len(), 2);
    }

    #[tokio::test]
    async fn search_considers_matches_beyond_newest_candidates() {
        let store = SqliteGraphStore::open_in_memory().expect("store should open");
        commit_evidence(
            &store,
            "ev-old",
            "docs",
            "Needle evidence remains searchable after newer writes",
        )
        .await;

        let scope = SourceScope::parse("docs").expect("scope should parse");
        let newer = (0..500)
            .map(|index| {
                EvidenceRecord::new(
                    format!("ev-new-{index}"),
                    scope.clone(),
                    format!("Unrelated graph maintenance record {index}"),
                    Vec::new(),
                )
                .expect("evidence should validate")
            })
            .collect::<Vec<_>>();
        let batch = GraphMutationBatch::new(newer).expect("batch should validate");
        let receipt = store
            .commit_mutation_batch(batch)
            .await
            .expect("commit should succeed");

        let hits = store
            .search(GraphSearchRequest {
                query: "Needle".to_owned(),
                source_scope: Some("docs".to_owned()),
                graph_version: receipt.graph_version,
                limit: 5,
            })
            .await
            .expect("search should succeed");

        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].evidence_id, "ev-old");
    }

    #[tokio::test]
    async fn search_respects_graph_version_snapshot() {
        let store = SqliteGraphStore::open_in_memory().expect("store should open");
        commit_evidence(&store, "ev-1", "docs", "Snapshot only sees Rust").await;
        commit_evidence(&store, "ev-2", "docs", "Future vector token").await;

        let before_future = store
            .search(GraphSearchRequest {
                query: "Future".to_owned(),
                source_scope: Some("docs".to_owned()),
                graph_version: GraphVersion::new(1),
                limit: 5,
            })
            .await
            .expect("search should succeed");
        let after_future = store
            .search(GraphSearchRequest {
                query: "Future".to_owned(),
                source_scope: Some("docs".to_owned()),
                graph_version: GraphVersion::new(2),
                limit: 5,
            })
            .await
            .expect("search should succeed");

        assert!(before_future.is_empty());
        assert_eq!(after_future.len(), 1);
        assert_eq!(after_future[0].evidence_id, "ev-2");
    }

    #[tokio::test]
    async fn search_snapshot_excludes_updated_evidence_from_future_version() {
        let store = SqliteGraphStore::open_in_memory().expect("store should open");
        commit_evidence(&store, "ev-1", "docs", "Original graph token").await;
        commit_evidence(&store, "ev-1", "docs", "Future graph token").await;

        let before_update = store
            .search(GraphSearchRequest {
                query: "Future".to_owned(),
                source_scope: Some("docs".to_owned()),
                graph_version: GraphVersion::new(1),
                limit: 5,
            })
            .await
            .expect("search should succeed");
        let after_update = store
            .search(GraphSearchRequest {
                query: "Future".to_owned(),
                source_scope: Some("docs".to_owned()),
                graph_version: GraphVersion::new(2),
                limit: 5,
            })
            .await
            .expect("search should succeed");

        assert!(before_update.is_empty());
        assert_eq!(after_update.len(), 1);
        assert_eq!(after_update[0].evidence_id, "ev-1");
    }

    #[test]
    fn stable_entity_ids_are_deterministic() {
        assert_eq!(stable_id("entity", "Rust"), "entity:bffedf1f6f66c727");
        assert_eq!(stable_id("entity", "Rust"), stable_id("entity", "rust"));
    }

    #[tokio::test]
    async fn open_creates_parent_database_directory() {
        let path = std::env::temp_dir()
            .join(format!("relay-knowledge-storage-{}", std::process::id()))
            .join("nested")
            .join("graph.sqlite");
        let _ = std::fs::remove_file(&path);

        let store = SqliteGraphStore::open(&path).expect("store should open");
        let version = store
            .current_graph_version()
            .await
            .expect("version should load");

        assert_eq!(version, GraphVersion::ZERO);
        assert!(path.exists());
    }

    async fn commit_evidence(
        store: &SqliteGraphStore,
        id: &str,
        source_scope: &str,
        content: &str,
    ) {
        let evidence = EvidenceRecord::new(
            id,
            SourceScope::parse(source_scope).expect("scope should parse"),
            content,
            vec!["Rust".to_owned()],
        )
        .expect("evidence should validate");
        let batch = GraphMutationBatch::new(vec![evidence]).expect("batch should validate");

        store
            .commit_mutation_batch(batch)
            .await
            .expect("commit should succeed");
    }
}
