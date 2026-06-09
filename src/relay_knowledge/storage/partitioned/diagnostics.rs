use std::collections::BTreeSet;
use std::path::Path;

use crate::paths::RuntimePaths;
use crate::storage::sqlite::read_only_database_diagnostics;
use crate::storage::{
    GraphInspection, GraphStore, HealthStorageSnapshot, SqliteStorageDiagnostics, StorageError,
    StorageTopologySnapshot,
};

use super::{
    PartitionedSqliteKnowledgeStore,
    catalog::{catalog_has_active_repositories, catalog_topology_snapshot},
};

impl PartitionedSqliteKnowledgeStore {
    pub fn has_active_catalog(control_path: impl AsRef<Path>) -> Result<bool, StorageError> {
        catalog_has_active_repositories(control_path.as_ref())
    }

    pub async fn topology_snapshot(&self) -> Result<StorageTopologySnapshot, StorageError> {
        self.catalog.topology_snapshot().await
    }

    pub fn topology_snapshot_from_catalog(
        control_path: impl AsRef<Path>,
        paths: &RuntimePaths,
    ) -> Result<StorageTopologySnapshot, StorageError> {
        catalog_topology_snapshot(control_path.as_ref(), paths)
    }
}

pub(super) async fn inspect_graph(
    store: &PartitionedSqliteKnowledgeStore,
) -> Result<GraphInspection, StorageError> {
    let mut graph = store.control.inspect_graph().await?;
    graph.sqlite = aggregate_sqlite_diagnostics(store, graph.sqlite).await?;
    Ok(graph)
}

pub(super) async fn health_snapshot(
    store: &PartitionedSqliteKnowledgeStore,
    now_ms: u64,
) -> Result<HealthStorageSnapshot, StorageError> {
    let mut snapshot = store.control.health_snapshot(now_ms).await?;
    snapshot.graph.sqlite = aggregate_sqlite_diagnostics(store, snapshot.graph.sqlite).await?;
    Ok(snapshot)
}

async fn aggregate_sqlite_diagnostics(
    store: &PartitionedSqliteKnowledgeStore,
    control_sqlite: SqliteStorageDiagnostics,
) -> Result<SqliteStorageDiagnostics, StorageError> {
    let mut aggregate = SqliteDiagnosticsAggregate::new();
    aggregate.push("control", control_sqlite);
    for (repository_id, shard_path) in store.catalog.active_repository_database_paths().await? {
        let label = format!("shard {repository_id}");
        let diagnostics =
            tokio::task::spawn_blocking(move || shard_sqlite_diagnostics(&shard_path)).await?;
        match diagnostics {
            Ok(diagnostics) => aggregate.push(format!("shard {repository_id}"), diagnostics),
            Err(error) => aggregate.push_error(label, error),
        }
    }
    Ok(aggregate.finish())
}

fn shard_sqlite_diagnostics(shard_path: &Path) -> Result<SqliteStorageDiagnostics, StorageError> {
    match std::fs::metadata(shard_path) {
        Ok(_) => read_only_database_diagnostics(shard_path),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            Err(StorageError::InvalidInput(format!(
                "repository shard '{}' is missing",
                shard_path.display()
            )))
        }
        Err(error) => Err(error.into()),
    }
}

struct SqliteDiagnosticsAggregate {
    journal_modes: BTreeSet<String>,
    wal_size_bytes: Option<u64>,
    last_maintenance_at_ms: Option<u64>,
    maintenance_errors: Vec<String>,
}

impl SqliteDiagnosticsAggregate {
    fn new() -> Self {
        Self {
            journal_modes: BTreeSet::new(),
            wal_size_bytes: Some(0),
            last_maintenance_at_ms: None,
            maintenance_errors: Vec::new(),
        }
    }

    fn push(&mut self, label: impl AsRef<str>, diagnostics: SqliteStorageDiagnostics) {
        if !diagnostics.journal_mode.is_empty() {
            self.journal_modes.insert(diagnostics.journal_mode);
        }
        self.wal_size_bytes = match (self.wal_size_bytes, diagnostics.wal_size_bytes) {
            (Some(left), Some(right)) => Some(left.saturating_add(right)),
            _ => None,
        };
        self.last_maintenance_at_ms = self
            .last_maintenance_at_ms
            .max(diagnostics.last_maintenance_at_ms);
        if let Some(error) = diagnostics.last_maintenance_error {
            self.maintenance_errors
                .push(format!("{}: {error}", label.as_ref()));
        }
    }

    fn push_error(&mut self, label: impl AsRef<str>, error: StorageError) {
        self.wal_size_bytes = None;
        self.maintenance_errors
            .push(format!("{}: {error}", label.as_ref()));
    }

    fn finish(self) -> SqliteStorageDiagnostics {
        SqliteStorageDiagnostics {
            journal_mode: match self.journal_modes.len() {
                0 => String::new(),
                1 => self
                    .journal_modes
                    .into_iter()
                    .next()
                    .expect("one journal mode should exist"),
                _ => "mixed".to_owned(),
            },
            wal_size_bytes: self.wal_size_bytes,
            last_maintenance_at_ms: self.last_maintenance_at_ms,
            last_maintenance_error: (!self.maintenance_errors.is_empty())
                .then(|| self.maintenance_errors.join("; ")),
        }
    }
}
