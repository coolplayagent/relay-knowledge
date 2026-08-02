use std::{error::Error, fmt};

use crate::{env::EnvironmentConfig, storage::StorageTopology};

/// Storage backend topology selected for this runtime.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StorageRuntimeConfig {
    pub topology: StorageTopology,
}

impl StorageRuntimeConfig {
    pub fn from_environment(
        environment: &EnvironmentConfig,
    ) -> Result<Self, StorageRuntimeConfigError> {
        let topology = environment
            .storage_topology
            .as_deref()
            .map(parse_storage_topology)
            .transpose()?
            .unwrap_or(StorageTopology::SingleSqlite);

        Ok(Self { topology })
    }
}

/// Storage runtime configuration validation error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StorageRuntimeConfigError {
    InvalidTopology(String),
}

impl fmt::Display for StorageRuntimeConfigError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidTopology(value) => write!(
                formatter,
                "storage topology '{value}' must be single_sqlite or partitioned_sqlite"
            ),
        }
    }
}

impl Error for StorageRuntimeConfigError {}

fn parse_storage_topology(value: &str) -> Result<StorageTopology, StorageRuntimeConfigError> {
    match StorageTopology::parse(value) {
        Ok(topology) => Ok(topology),
        Err(_) => Err(StorageRuntimeConfigError::InvalidTopology(value.to_owned())),
    }
}

#[cfg(test)]
#[path = "storage_tests.rs"]
mod storage_tests;
