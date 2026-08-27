//! Compatibility surface for diagnostics contracts moved into the domain layer.

pub use crate::domain::{
    GraphInspection, HealthStorageSnapshot, SqliteStorageDiagnostics, StorageHealth,
};

#[cfg(test)]
#[path = "health_tests.rs"]
mod tests;
