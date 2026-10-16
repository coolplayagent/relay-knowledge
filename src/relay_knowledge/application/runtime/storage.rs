use std::{error::Error, fmt, time::Duration};

use crate::{env::EnvironmentConfig, storage::StorageTopology};

/// Storage backend topology selected for this runtime.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StorageRuntimeConfig {
    pub topology: StorageTopology,
    pub health_budget: Duration,
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

        let health_budget_ms = environment.health_storage_budget_ms.unwrap_or(500);
        if !(1..=30_000).contains(&health_budget_ms) {
            return Err(StorageRuntimeConfigError::InvalidHealthBudget);
        }
        Ok(Self {
            topology,
            health_budget: Duration::from_millis(health_budget_ms),
        })
    }
}

/// Storage runtime configuration validation error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StorageRuntimeConfigError {
    InvalidTopology(String),
    InvalidHealthBudget,
}

impl fmt::Display for StorageRuntimeConfigError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidHealthBudget => formatter
                .write_str("health storage budget must be between 1 and 30000 milliseconds"),
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
