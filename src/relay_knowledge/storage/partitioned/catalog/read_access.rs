//! Catalog reads through the validated control pool and first-open shard policy.

use super::{SqliteShardCatalog, open_cached_repository_store};
use crate::storage::{SqliteGraphStore, StorageError};
use rusqlite::OptionalExtension;
use std::sync::Arc;

impl SqliteShardCatalog {
    pub(in crate::storage::partitioned) async fn cached_health_shards(
        &self,
    ) -> Result<Vec<(String, Arc<SqliteGraphStore>)>, StorageError> {
        let ids = self.repository_ids().await?;
        let cache = self.cache.try_lock().map_err(|error| match error {
            std::sync::TryLockError::Poisoned(_) => StorageError::LockPoisoned,
            std::sync::TryLockError::WouldBlock => {
                StorageError::Busy("shard cache is occupied".to_owned())
            }
        })?;
        let cold = ids.iter().filter(|id| !cache.contains_key(*id)).count();
        if cold > 0 {
            return Err(StorageError::Busy(format!(
                "storage_cold: {cold} repository shards have no validated open handle; health does not open cold shards"
            )));
        }
        Ok(ids
            .into_iter()
            .map(|id| {
                let store = Arc::clone(
                    cache
                        .get(&id)
                        .expect("all active handles were checked under this lock"),
                );
                (id, store)
            })
            .collect())
    }

    pub(in crate::storage::partitioned) async fn repository_ids(
        &self,
    ) -> Result<Vec<String>, StorageError> {
        self.control.run_read(|connection| {
            let mut statement = connection.prepare("SELECT repository_id FROM storage_repository_shards WHERE state = 'active' ORDER BY repository_id ASC")?;
            statement.query_map([], |row| row.get::<_, String>(0))?.collect::<Result<Vec<_>, _>>().map_err(StorageError::from)
        }).await
    }

    pub(in crate::storage::partitioned) async fn diagnostic_repository_ids(
        &self,
    ) -> Result<Vec<String>, StorageError> {
        self.control.run_read(|connection| {
            let limit = crate::storage::sqlite::MAX_SQLITE_DIAGNOSTIC_SHARDS;
            let mut statement = connection.prepare("SELECT repository_id FROM storage_repository_shards WHERE state = 'active' ORDER BY repository_id ASC LIMIT ?1")?;
            let ids = statement.query_map([limit + 1], |row| row.get::<_, String>(0))?
                .collect::<Result<Vec<_>, _>>()?;
            if ids.len() > limit {
                return Err(StorageError::InvalidInput("SQLite inspection exceeds 1024 active shards".to_owned()));
            }
            Ok(ids)
        }).await
    }

    pub(in crate::storage::partitioned) async fn existing_repository_store(
        &self,
        repository_id: String,
    ) -> Result<Option<Arc<SqliteGraphStore>>, StorageError> {
        let requested = repository_id.clone();
        let active = self.control.run_read(move |connection| {
            connection.query_row("SELECT 1 FROM storage_repository_shards WHERE repository_id = ?1 AND state = 'active'", [&requested], |_| Ok(())).optional().map(|row| row.is_some()).map_err(StorageError::from)
        }).await?;
        if !active {
            return Ok(None);
        }
        let db_path = self.paths.repository_shard_database_file(&repository_id);
        let cache = Arc::clone(&self.cache);
        let control_path = self.control_path.clone();
        let paths = self.paths.clone();
        tokio::task::spawn_blocking(move || {
            if !db_path.exists() {
                return Err(StorageError::InvalidInput(format!(
                    "repository shard '{}' is missing",
                    db_path.display()
                )));
            }
            open_cached_repository_store(&cache, repository_id, db_path, control_path, &paths)
                .map(Some)
        })
        .await?
    }
}
