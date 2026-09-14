//! Durable repository-shard catalog and cached shard-store ownership.

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::Duration,
};

use rusqlite::{Connection, OpenFlags, OptionalExtension, Transaction, params};

use crate::storage::sqlite::code::lifecycle::publication_fence::{
    PartitionedPublicationTarget, prepare_partitioned_target,
};
use crate::{
    clock::system_now_millis_or_zero as now_millis,
    domain::{CodeIndexPublicationFence, CodeRepositoryStatus},
    paths::RuntimePaths,
    storage::{
        SqliteGraphStore, StorageError, StorageShardCatalogEntry, StorageTopologySnapshot,
        sqlite::{configure_connection, preserve_existing_scope_commit, record_commit_scope},
    },
};

const CATALOG_READ_BUSY_TIMEOUT: Duration = Duration::from_millis(50);

mod schema;

pub(super) use schema::initialize_catalog_schema;

#[derive(Debug)]
pub(super) struct SqliteShardCatalog {
    control_path: PathBuf,
    paths: RuntimePaths,
    cache: Arc<Mutex<HashMap<String, Arc<SqliteGraphStore>>>>,
}

impl SqliteShardCatalog {
    pub(super) fn new(control_path: PathBuf, paths: RuntimePaths) -> Self {
        Self {
            control_path,
            paths,
            cache: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub(super) async fn staged_repository_store(
        &self,
        repository_id: String,
    ) -> Result<Arc<SqliteGraphStore>, StorageError> {
        let db_path = self.paths.repository_shard_database_file(&repository_id);
        let cache = Arc::clone(&self.cache);
        let control_path = self.control_path.clone();
        tokio::task::spawn_blocking(move || {
            open_cached_repository_store(&cache, repository_id, db_path, control_path)
        })
        .await?
    }

    pub(super) async fn checkpoint_scope_store(
        &self,
        source_scope: String,
    ) -> Result<Option<Arc<SqliteGraphStore>>, StorageError> {
        let control_path = self.control_path.clone();
        let paths = self.paths.clone();
        let cache = Arc::clone(&self.cache);
        tokio::task::spawn_blocking(move || {
            let Some(repository_id) = catalog_repository_for_scope(&control_path, &source_scope)?
            else {
                return Ok(None);
            };
            let db_path = paths.repository_shard_database_file(&repository_id);
            if !db_path.exists() {
                return Ok(None);
            }
            open_cached_repository_store(&cache, repository_id, db_path, control_path).map(Some)
        })
        .await?
    }

    pub(super) async fn checkpoint_repository_store(
        &self,
        repository_id: String,
    ) -> Result<Option<Arc<SqliteGraphStore>>, StorageError> {
        let db_path = self.paths.repository_shard_database_file(&repository_id);
        let cache = Arc::clone(&self.cache);
        let control_path = self.control_path.clone();
        tokio::task::spawn_blocking(move || {
            if !db_path.exists() {
                return Ok(None);
            }
            open_cached_repository_store(&cache, repository_id, db_path, control_path).map(Some)
        })
        .await?
    }

    pub(super) async fn existing_repository_store(
        &self,
        repository_id: String,
    ) -> Result<Option<Arc<SqliteGraphStore>>, StorageError> {
        let control_path = self.control_path.clone();
        let paths = self.paths.clone();
        let cache = Arc::clone(&self.cache);
        tokio::task::spawn_blocking(move || {
            let Some(db_path) = catalog_repository_path(&control_path, &paths, &repository_id)?
            else {
                return Ok(None);
            };
            if !db_path.exists() {
                return Err(StorageError::InvalidInput(format!(
                    "repository shard '{}' is missing",
                    db_path.display()
                )));
            }

            open_cached_repository_store(&cache, repository_id, db_path, control_path).map(Some)
        })
        .await?
    }

    pub(super) async fn import_control_repository_metadata(
        &self,
        shard: Arc<SqliteGraphStore>,
        repository_id: String,
    ) -> Result<(), StorageError> {
        shard
            .import_code_repository_from_database(self.control_path.clone(), repository_id, None)
            .await
    }

    /// Persists the bounded control-plane handoff before the shard enters its
    /// publication transaction.
    pub(super) async fn prepare_snapshot_target(
        &self,
        snapshot: &crate::domain::CodeIndexSnapshot,
        fence: CodeIndexPublicationFence,
    ) -> Result<(), StorageError> {
        let control_path = self.control_path.clone();
        let target = PartitionedPublicationTarget::from(snapshot);
        tokio::task::spawn_blocking(move || {
            let mut connection = open_catalog_connection(&control_path)?;
            prepare_partitioned_target(&mut connection, &target, fence)
        })
        .await?
    }

    pub(super) async fn activate_repository(
        &self,
        repository_id: String,
    ) -> Result<(), StorageError> {
        let control_path = self.control_path.clone();
        let db_path = self.paths.repository_shard_database_file(&repository_id);
        let shard_locator = shard_locator(&self.paths, &db_path);
        tokio::task::spawn_blocking(move || {
            upsert_catalog_repository(&control_path, &repository_id, &shard_locator)
        })
        .await?
    }

    pub(super) async fn record_scope(
        &self,
        repository_id: String,
        source_scope: String,
    ) -> Result<(), StorageError> {
        let control_path = self.control_path.clone();
        let db_path = self.paths.repository_shard_database_file(&repository_id);
        let shard_locator = shard_locator(&self.paths, &db_path);
        tokio::task::spawn_blocking(move || {
            record_catalog_scope(&control_path, &repository_id, &source_scope, &shard_locator)
        })
        .await?
    }

    /// Atomically opens the control-plane read gate for one already-published
    /// shard scope and mirrors the matching repository status.
    pub(super) async fn publish_scope_status_with_fence(
        &self,
        repository_id: String,
        source_scope: String,
        status: CodeRepositoryStatus,
        fence: CodeIndexPublicationFence,
    ) -> Result<(), StorageError> {
        let control_path = self.control_path.clone();
        let db_path = self.paths.repository_shard_database_file(&repository_id);
        let shard_locator = shard_locator(&self.paths, &db_path);
        tokio::task::spawn_blocking(move || {
            publish_catalog_scope_status_with_fence(
                &control_path,
                &repository_id,
                &source_scope,
                &shard_locator,
                &status,
                fence,
            )
        })
        .await?
    }

    pub(super) async fn stage_scope(
        &self,
        repository_id: String,
        source_scope: String,
    ) -> Result<(), StorageError> {
        let control_path = self.control_path.clone();
        let db_path = self.paths.repository_shard_database_file(&repository_id);
        let shard_locator = shard_locator(&self.paths, &db_path);
        tokio::task::spawn_blocking(move || {
            stage_catalog_scope(&control_path, &repository_id, &source_scope, &shard_locator)
        })
        .await?
    }

    pub(super) async fn stage_scope_with_fence(
        &self,
        repository_id: String,
        source_scope: String,
        fence: CodeIndexPublicationFence,
    ) -> Result<(), StorageError> {
        let control_path = self.control_path.clone();
        let db_path = self.paths.repository_shard_database_file(&repository_id);
        let shard_locator = shard_locator(&self.paths, &db_path);
        tokio::task::spawn_blocking(move || {
            stage_catalog_scope_with_fence(
                &control_path,
                &repository_id,
                &source_scope,
                &shard_locator,
                fence,
            )
        })
        .await?
    }

    #[cfg(test)]
    pub(super) async fn repository_for_scope(
        &self,
        source_scope: String,
    ) -> Result<Option<String>, StorageError> {
        let control_path = self.control_path.clone();
        tokio::task::spawn_blocking(move || {
            catalog_repository_for_scope(&control_path, &source_scope)
        })
        .await?
    }

    pub(super) async fn active_repository_for_scope(
        &self,
        source_scope: String,
    ) -> Result<Option<String>, StorageError> {
        let control_path = self.control_path.clone();
        tokio::task::spawn_blocking(move || {
            catalog_active_repository_for_scope(&control_path, &source_scope)
        })
        .await?
    }

    pub(super) async fn staged_scope_owned_by_task(
        &self,
        repository_id: String,
        source_scope: String,
        task_id: String,
    ) -> Result<bool, StorageError> {
        let control_path = self.control_path.clone();
        tokio::task::spawn_blocking(move || {
            let connection = open_catalog_readonly_connection(&control_path)?;
            connection
                .query_row(
                    "SELECT 1 FROM storage_repository_shard_scopes
                     WHERE repository_id = ?1 AND source_scope = ?2
                       AND state = 'staged' AND staged_task_id = ?3",
                    params![repository_id, source_scope, task_id],
                    |_| Ok(()),
                )
                .optional()
                .map(|row| row.is_some())
                .map_err(StorageError::from)
        })
        .await?
    }

    pub(super) async fn remove_scope_route(
        &self,
        repository_id: String,
        source_scope: String,
    ) -> Result<usize, StorageError> {
        let control_path = self.control_path.clone();
        tokio::task::spawn_blocking(move || {
            let connection = open_catalog_connection(&control_path)?;
            connection
                .execute(
                    "DELETE FROM storage_repository_shard_scopes
                     WHERE repository_id = ?1 AND source_scope = ?2",
                    params![repository_id, source_scope],
                )
                .map_err(StorageError::from)
        })
        .await?
    }

    pub(super) async fn repository_ids(&self) -> Result<Vec<String>, StorageError> {
        let control_path = self.control_path.clone();
        tokio::task::spawn_blocking(move || catalog_repository_ids(&control_path)).await?
    }

    pub(super) async fn active_repository_database_paths(
        &self,
    ) -> Result<Vec<(String, PathBuf)>, StorageError> {
        let control_path = self.control_path.clone();
        let paths = self.paths.clone();
        tokio::task::spawn_blocking(move || {
            let repository_ids = catalog_repository_ids(&control_path)?;
            Ok(repository_ids
                .into_iter()
                .map(|repository_id| {
                    let db_path = paths.repository_shard_database_file(&repository_id);
                    (repository_id, db_path)
                })
                .collect())
        })
        .await?
    }

    pub(super) async fn topology_snapshot(&self) -> Result<StorageTopologySnapshot, StorageError> {
        let control_path = self.control_path.clone();
        let paths = self.paths.clone();
        tokio::task::spawn_blocking(move || catalog_topology_snapshot(&control_path, &paths))
            .await?
    }

    pub(super) async fn remove_repository(
        &self,
        repository_id: String,
    ) -> Result<(), StorageError> {
        let control_path = self.control_path.clone();
        let cache = Arc::clone(&self.cache);
        tokio::task::spawn_blocking(move || {
            cache
                .lock()
                .map_err(|_| StorageError::LockPoisoned)?
                .remove(&repository_id);
            remove_catalog_repository(&control_path, &repository_id)
        })
        .await?
    }
}

fn open_cached_repository_store(
    cache: &Arc<Mutex<HashMap<String, Arc<SqliteGraphStore>>>>,
    repository_id: String,
    db_path: PathBuf,
    control_path: PathBuf,
) -> Result<Arc<SqliteGraphStore>, StorageError> {
    let mut cache = cache.lock().map_err(|_| StorageError::LockPoisoned)?;
    if let Some(store) = cache.get(&repository_id) {
        return Ok(Arc::clone(store));
    }

    let store = Arc::new(SqliteGraphStore::open_with_publication_authority(
        &db_path,
        control_path,
    )?);
    cache.insert(repository_id, Arc::clone(&store));
    Ok(store)
}

fn shard_locator(paths: &RuntimePaths, db_path: &Path) -> String {
    db_path
        .strip_prefix(&paths.data_dir)
        .unwrap_or(db_path)
        .display()
        .to_string()
}

fn upsert_catalog_repository(
    control_path: &Path,
    repository_id: &str,
    shard_locator: &str,
) -> Result<(), StorageError> {
    upsert_catalog_repository_state(control_path, repository_id, shard_locator, "active")
}

fn stage_catalog_repository(
    control_path: &Path,
    repository_id: &str,
    shard_locator: &str,
) -> Result<(), StorageError> {
    upsert_catalog_repository_state(control_path, repository_id, shard_locator, "staged")
}

fn upsert_catalog_repository_state(
    control_path: &Path,
    repository_id: &str,
    shard_locator: &str,
    state: &str,
) -> Result<(), StorageError> {
    initialize_catalog_schema(control_path)?;
    let connection = open_catalog_connection(control_path)?;
    connection.execute(
        "
        INSERT INTO storage_repository_shards (
            repository_id, db_path, state, created_at_ms, updated_at_ms
        )
        VALUES (?1, ?2, ?3, ?4, ?4)
        ON CONFLICT(repository_id) DO UPDATE SET
            db_path = excluded.db_path,
            state = CASE
                WHEN storage_repository_shards.state = 'active'
                    AND excluded.state = 'staged'
                THEN 'active'
                ELSE excluded.state
            END,
            updated_at_ms = excluded.updated_at_ms
        ",
        params![repository_id, shard_locator, state, now_millis()],
    )?;
    Ok(())
}

fn record_catalog_scope(
    control_path: &Path,
    repository_id: &str,
    source_scope: &str,
    shard_locator: &str,
) -> Result<(), StorageError> {
    upsert_catalog_repository(control_path, repository_id, shard_locator)?;
    let connection = open_catalog_connection(control_path)?;
    connection.execute(
        "
        INSERT INTO storage_repository_shard_scopes (
            source_scope, repository_id, state, staged_task_id, updated_at_ms
        )
        VALUES (?1, ?2, 'active', NULL, ?3)
        ON CONFLICT(source_scope) DO UPDATE SET
            repository_id = excluded.repository_id,
            state = excluded.state,
            staged_task_id = NULL,
            updated_at_ms = excluded.updated_at_ms
        ",
        params![source_scope, repository_id, now_millis()],
    )?;
    Ok(())
}

fn stage_catalog_scope(
    control_path: &Path,
    repository_id: &str,
    source_scope: &str,
    shard_locator: &str,
) -> Result<(), StorageError> {
    stage_catalog_repository(control_path, repository_id, shard_locator)?;
    let connection = open_catalog_connection(control_path)?;
    connection.execute(
        "
        INSERT INTO storage_repository_shard_scopes (
            source_scope, repository_id, state, staged_task_id, updated_at_ms
        )
        VALUES (?1, ?2, 'staged', NULL, ?3)
        ON CONFLICT(source_scope) DO UPDATE SET
            repository_id = CASE
                WHEN storage_repository_shard_scopes.state = 'active'
                THEN storage_repository_shard_scopes.repository_id
                ELSE excluded.repository_id
            END,
            state = CASE
                WHEN storage_repository_shard_scopes.state = 'active'
                THEN 'active'
                ELSE excluded.state
            END,
            updated_at_ms = excluded.updated_at_ms
        ",
        params![source_scope, repository_id, now_millis()],
    )?;
    Ok(())
}

fn stage_catalog_scope_with_fence(
    control_path: &Path,
    repository_id: &str,
    source_scope: &str,
    shard_locator: &str,
    fence: CodeIndexPublicationFence,
) -> Result<(), StorageError> {
    write_catalog_scope_with_fence(
        control_path,
        repository_id,
        source_scope,
        shard_locator,
        "staged",
        fence,
    )
}

fn write_catalog_scope_with_fence(
    control_path: &Path,
    repository_id: &str,
    source_scope: &str,
    shard_locator: &str,
    state: &str,
    fence: CodeIndexPublicationFence,
) -> Result<(), StorageError> {
    initialize_catalog_schema(control_path)?;
    let mut connection = open_catalog_connection(control_path)?;
    let guard = crate::storage::sqlite::code::lifecycle::publication_fence::prepare_guard(
        &connection,
        fence,
        None,
    )?;
    guard.validate_repository(repository_id)?;
    let transaction = connection.transaction()?;
    upsert_catalog_repository_in_transaction(&transaction, repository_id, shard_locator, state)?;
    transaction.execute(
        "
        INSERT INTO storage_repository_shard_scopes (
            source_scope, repository_id, state, staged_task_id, updated_at_ms
        )
        VALUES (?1, ?2, ?3, ?4, ?5)
        ON CONFLICT(source_scope) DO UPDATE SET
            repository_id = CASE
                WHEN storage_repository_shard_scopes.state = 'active'
                    AND excluded.state = 'staged'
                THEN storage_repository_shard_scopes.repository_id
                ELSE excluded.repository_id
            END,
            state = CASE
                WHEN storage_repository_shard_scopes.state = 'active'
                    AND excluded.state = 'staged'
                THEN 'active'
                ELSE excluded.state
            END,
            staged_task_id = CASE
                WHEN storage_repository_shard_scopes.state = 'active'
                    AND excluded.state = 'staged'
                THEN storage_repository_shard_scopes.staged_task_id
                ELSE excluded.staged_task_id
            END,
            updated_at_ms = excluded.updated_at_ms
        ",
        params![
            source_scope,
            repository_id,
            state,
            guard.task_id(),
            now_millis()
        ],
    )?;
    guard.validate_target_scope(&transaction, source_scope)?;
    guard.validate(&transaction)?;
    transaction.commit()?;
    Ok(())
}

fn upsert_catalog_repository_in_transaction(
    transaction: &Transaction<'_>,
    repository_id: &str,
    shard_locator: &str,
    state: &str,
) -> Result<(), StorageError> {
    transaction.execute(
        "
        INSERT INTO storage_repository_shards (
            repository_id, db_path, state, created_at_ms, updated_at_ms
        )
        VALUES (?1, ?2, ?3, ?4, ?4)
        ON CONFLICT(repository_id) DO UPDATE SET
            db_path = excluded.db_path,
            state = CASE
                WHEN storage_repository_shards.state = 'active'
                    AND excluded.state = 'staged'
                THEN 'active'
                ELSE excluded.state
            END,
            updated_at_ms = excluded.updated_at_ms
        ",
        params![repository_id, shard_locator, state, now_millis()],
    )?;
    Ok(())
}

fn publish_catalog_scope_status_with_fence(
    control_path: &Path,
    repository_id: &str,
    source_scope: &str,
    shard_locator: &str,
    status: &CodeRepositoryStatus,
    fence: CodeIndexPublicationFence,
) -> Result<(), StorageError> {
    if status.repository_id != repository_id
        || status.last_indexed_scope_id.as_deref() != Some(source_scope)
        || status.stale
        || status.state != "fresh"
    {
        return Err(StorageError::InvalidInput(format!(
            "shard status for repository '{repository_id}' does not describe fresh scope '{source_scope}'"
        )));
    }
    initialize_catalog_schema(control_path)?;
    let mut connection = open_catalog_connection(control_path)?;
    let guard = crate::storage::sqlite::code::lifecycle::publication_fence::prepare_guard(
        &connection,
        fence,
        None,
    )?;
    guard.validate_repository(repository_id)?;
    let transaction = connection.transaction()?;
    guard.validate_target_scope(&transaction, source_scope)?;
    guard.validate(&transaction)?;
    let route_is_publishable = transaction
        .query_row(
            "SELECT 1 FROM storage_repository_shard_scopes
             WHERE repository_id = ?1 AND source_scope = ?2
               AND (state = 'active'
                    OR (state = 'staged' AND staged_task_id = ?3))",
            params![repository_id, source_scope, guard.task_id()],
            |_| Ok(()),
        )
        .optional()?
        .is_some();
    if !route_is_publishable {
        return Err(StorageError::InvalidInput(format!(
            "catalog scope '{source_scope}' is not active or staged by task '{}'",
            guard.task_id()
        )));
    }
    upsert_catalog_repository_in_transaction(&transaction, repository_id, shard_locator, "active")?;
    transaction.execute(
        "
        INSERT INTO storage_repository_shard_scopes (
            source_scope, repository_id, state, staged_task_id, updated_at_ms
        )
        VALUES (?1, ?2, 'active', NULL, ?3)
        ON CONFLICT(source_scope) DO UPDATE SET
            repository_id = excluded.repository_id,
            state = excluded.state,
            staged_task_id = NULL,
            updated_at_ms = excluded.updated_at_ms
        ",
        params![source_scope, repository_id, now_millis()],
    )?;
    mirror_repository_status(&transaction, status)?;
    guard.validate_target_scope(&transaction, source_scope)?;
    crate::storage::sqlite::code::record_receipt_from_active_fence(&transaction, source_scope)?;
    guard.validate(&transaction)?;
    transaction.commit()?;
    Ok(())
}

fn catalog_repository_for_scope(
    control_path: &Path,
    source_scope: &str,
) -> Result<Option<String>, StorageError> {
    let connection = open_catalog_readonly_connection(control_path)?;
    connection
        .query_row(
            "
            SELECT repository_id
            FROM storage_repository_shard_scopes
            WHERE source_scope = ?1
            ",
            params![source_scope],
            |row| row.get(0),
        )
        .optional()
        .map_err(StorageError::from)
}

fn catalog_active_repository_for_scope(
    control_path: &Path,
    source_scope: &str,
) -> Result<Option<String>, StorageError> {
    let connection = open_catalog_readonly_connection(control_path)?;
    connection
        .query_row(
            "
            SELECT scope.repository_id
            FROM storage_repository_shard_scopes scope
            JOIN storage_repository_shards shard
              ON shard.repository_id = scope.repository_id
            WHERE scope.source_scope = ?1
              AND scope.state = 'active'
              AND shard.state = 'active'
            ",
            params![source_scope],
            |row| row.get(0),
        )
        .optional()
        .map_err(StorageError::from)
}

fn catalog_repository_path(
    control_path: &Path,
    paths: &RuntimePaths,
    repository_id: &str,
) -> Result<Option<PathBuf>, StorageError> {
    let connection = open_catalog_readonly_connection(control_path)?;
    let is_active = connection
        .query_row(
            "
            SELECT 1
            FROM storage_repository_shards
            WHERE repository_id = ?1 AND state = 'active'
            ",
            params![repository_id],
            |_| Ok(()),
        )
        .optional()?
        .is_some();
    Ok(is_active.then(|| paths.repository_shard_database_file(repository_id)))
}

fn catalog_repository_ids(control_path: &Path) -> Result<Vec<String>, StorageError> {
    let connection = open_catalog_readonly_connection(control_path)?;
    let mut statement = connection.prepare(
        "
        SELECT repository_id
        FROM storage_repository_shards
        WHERE state = 'active'
        ORDER BY repository_id ASC
        ",
    )?;
    let rows = statement.query_map([], |row| row.get::<_, String>(0))?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(StorageError::from)
}

pub(super) fn catalog_has_active_repositories(control_path: &Path) -> Result<bool, StorageError> {
    if !control_path.exists() {
        return Ok(false);
    }
    let connection = open_catalog_readonly_connection(control_path)?;
    let has_catalog = connection
        .query_row(
            "
            SELECT 1
            FROM sqlite_master
            WHERE type = 'table' AND name = 'storage_repository_shards'
            ",
            [],
            |_| Ok(()),
        )
        .optional()?
        .is_some();
    if !has_catalog {
        return Ok(false);
    }

    connection
        .query_row(
            "
            SELECT 1
            FROM storage_repository_shards
            WHERE state = 'active'
            LIMIT 1
            ",
            [],
            |_| Ok(()),
        )
        .optional()
        .map(|row| row.is_some())
        .map_err(StorageError::from)
}

pub(super) fn catalog_topology_snapshot(
    control_path: &Path,
    paths: &RuntimePaths,
) -> Result<StorageTopologySnapshot, StorageError> {
    if !control_path.exists() {
        return Ok(StorageTopologySnapshot::default());
    }
    let connection = open_catalog_readonly_connection(control_path)?;
    let has_catalog = connection
        .query_row(
            "
            SELECT 1
            FROM sqlite_master
            WHERE type = 'table' AND name = 'storage_repository_shards'
            ",
            [],
            |_| Ok(()),
        )
        .optional()?
        .is_some();
    if !has_catalog {
        return Ok(StorageTopologySnapshot::default());
    }

    let mut statement = connection.prepare(
        "
        SELECT shard.repository_id,
               shard.state,
               shard.db_path,
               COUNT(scope.source_scope) AS source_scope_count,
               shard.updated_at_ms
        FROM storage_repository_shards shard
        LEFT JOIN storage_repository_shard_scopes scope
               ON scope.repository_id = shard.repository_id
        WHERE shard.state IN ('active', 'staged')
        GROUP BY shard.repository_id, shard.state, shard.db_path, shard.updated_at_ms
        ORDER BY shard.repository_id ASC
        ",
    )?;
    let rows = statement.query_map([], |row| {
        let repository_id = row.get::<_, String>(0)?;
        let resolved_path = paths.repository_shard_database_file(&repository_id);
        Ok(StorageShardCatalogEntry {
            repository_id,
            state: row.get(1)?,
            shard_locator: row.get(2)?,
            resolved_path: resolved_path.display().to_string(),
            source_scope_count: row.get::<_, i64>(3)?.max(0) as usize,
            exists: resolved_path.exists(),
            updated_at_ms: row.get::<_, i64>(4)?.max(0) as u64,
        })
    })?;
    let shards = rows
        .collect::<Result<Vec<_>, _>>()
        .map_err(StorageError::from)?;

    Ok(StorageTopologySnapshot { shards })
}

fn remove_catalog_repository(control_path: &Path, repository_id: &str) -> Result<(), StorageError> {
    let connection = open_catalog_connection(control_path)?;
    connection.execute(
        "DELETE FROM storage_repository_shard_scopes WHERE repository_id = ?1",
        params![repository_id],
    )?;
    connection.execute(
        "
        UPDATE storage_repository_shards
        SET state = 'removed', updated_at_ms = ?2
        WHERE repository_id = ?1
        ",
        params![repository_id, now_millis()],
    )?;
    Ok(())
}

fn open_catalog_connection(control_path: &Path) -> Result<Connection, StorageError> {
    let connection = Connection::open(control_path)?;
    configure_connection(&connection)?;
    Ok(connection)
}

fn open_catalog_readonly_connection(control_path: &Path) -> Result<Connection, StorageError> {
    let connection = Connection::open_with_flags(control_path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    configure_connection(&connection)?;
    connection.busy_timeout(CATALOG_READ_BUSY_TIMEOUT)?;
    Ok(connection)
}

pub(super) fn mirror_repository_status(
    connection: &Connection,
    status: &CodeRepositoryStatus,
) -> Result<(), StorageError> {
    let Some(source_scope) = status.last_indexed_scope_id.as_deref() else {
        return Ok(());
    };
    let path_filters_json = json(&status.path_filters)?;
    let language_filters_json = json(&status.language_filters)?;
    preserve_existing_scope_commit(connection, &status.repository_id, source_scope)?;
    connection.execute(
        "
        INSERT INTO code_repository_scopes (
            source_scope, repository_id, resolved_commit_sha, tree_hash,
            path_filters_json, language_filters_json, indexed_file_count,
            symbol_count, reference_count, chunk_count, stale, degraded_reason
        )
        VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
        ON CONFLICT(source_scope) DO UPDATE SET
            repository_id = excluded.repository_id,
            resolved_commit_sha = excluded.resolved_commit_sha,
            tree_hash = excluded.tree_hash,
            path_filters_json = excluded.path_filters_json,
            language_filters_json = excluded.language_filters_json,
            indexed_file_count = excluded.indexed_file_count,
            symbol_count = excluded.symbol_count,
            reference_count = excluded.reference_count,
            chunk_count = excluded.chunk_count,
            stale = excluded.stale,
            degraded_reason = excluded.degraded_reason
        ",
        params![
            source_scope,
            status.repository_id,
            status.last_indexed_commit,
            status.tree_hash,
            path_filters_json,
            language_filters_json,
            status.indexed_file_count,
            status.symbol_count,
            status.reference_count,
            status.chunk_count,
            i64::from(status.stale),
            status.degraded_reason,
        ],
    )?;
    if let Some(resolved_commit_sha) = status.last_indexed_commit.as_deref() {
        record_commit_scope(
            connection,
            &status.repository_id,
            resolved_commit_sha,
            source_scope,
        )?;
    }
    connection.execute(
        "
        UPDATE code_repositories
        SET last_indexed_scope_id = ?2,
            last_indexed_commit = ?3,
            tree_hash = ?4,
            state = ?5,
            indexed_file_count = ?6,
            symbol_count = ?7,
            reference_count = ?8,
            chunk_count = ?9,
            stale = ?10,
            degraded_reason = ?11
        WHERE repository_id = ?1
        ",
        params![
            status.repository_id,
            source_scope,
            status.last_indexed_commit,
            status.tree_hash,
            status.state,
            status.indexed_file_count,
            status.symbol_count,
            status.reference_count,
            status.chunk_count,
            i64::from(status.stale),
            status.degraded_reason,
        ],
    )?;

    Ok(())
}

fn json<T: serde::Serialize>(value: &T) -> Result<String, StorageError> {
    serde_json::to_string(value).map_err(|error| StorageError::InvalidInput(error.to_string()))
}

#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;
