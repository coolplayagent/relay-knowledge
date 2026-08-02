use serde::{Deserialize, Serialize};

use super::StorageError;

/// Storage topology selected at runtime.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StorageTopology {
    SingleSqlite,
    PartitionedSqlite,
}

impl StorageTopology {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::SingleSqlite => "single_sqlite",
            Self::PartitionedSqlite => "partitioned_sqlite",
        }
    }

    pub fn parse(value: &str) -> Result<Self, StorageError> {
        match value.trim().to_ascii_lowercase().as_str() {
            "" | "single" | "single_sqlite" | "sqlite" => Ok(Self::SingleSqlite),
            "partitioned" | "partitioned_sqlite" | "sqlite_partitioned" => {
                Ok(Self::PartitionedSqlite)
            }
            other => Err(StorageError::InvalidInput(format!(
                "storage topology '{other}' must be single_sqlite or partitioned_sqlite"
            ))),
        }
    }
}

/// Runtime storage topology snapshot surfaced through service diagnostics.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct StorageTopologySnapshot {
    pub shards: Vec<StorageShardCatalogEntry>,
}

/// One repository shard entry from the partitioned SQLite catalog.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StorageShardCatalogEntry {
    pub repository_id: String,
    pub state: String,
    pub shard_locator: String,
    pub resolved_path: String,
    pub source_scope_count: usize,
    pub exists: bool,
    pub updated_at_ms: u64,
}

#[cfg(test)]
#[path = "topology_tests.rs"]
mod tests;
