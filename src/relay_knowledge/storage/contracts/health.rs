use serde::{Deserialize, Serialize};

use crate::domain::{CodeParseStatusCounts, CodeRepositoryTotals, GraphVersion, IndexStatus};

use super::{FileIndexDiagnostics, IndexCursor, IndexRefreshDiagnostics};

/// Aggregated graph status for diagnostics.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GraphInspection {
    pub graph_version: GraphVersion,
    pub entity_count: usize,
    pub evidence_count: usize,
    pub relation_count: usize,
    pub claim_count: usize,
    pub event_count: usize,
    pub mutation_count: usize,
    pub code_file_count: usize,
    pub code_symbol_count: usize,
    pub code_reference_count: usize,
    pub code_chunk_count: usize,
    pub code_parse_status_counts: CodeParseStatusCounts,
    #[serde(default)]
    pub sqlite: SqliteStorageDiagnostics,
}

impl Default for GraphInspection {
    fn default() -> Self {
        Self {
            graph_version: GraphVersion::ZERO,
            entity_count: 0,
            evidence_count: 0,
            relation_count: 0,
            claim_count: 0,
            event_count: 0,
            mutation_count: 0,
            code_file_count: 0,
            code_symbol_count: 0,
            code_reference_count: 0,
            code_chunk_count: 0,
            code_parse_status_counts: CodeParseStatusCounts::default(),
            sqlite: SqliteStorageDiagnostics::default(),
        }
    }
}

/// SQLite-specific health data included in shared graph diagnostics.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SqliteStorageDiagnostics {
    pub journal_mode: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub wal_size_bytes: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_maintenance_at_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_maintenance_error: Option<String>,
}

/// Read-only storage view used by service health without mutating indexes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HealthStorageSnapshot {
    pub graph: GraphInspection,
    pub repository_code_totals: CodeRepositoryTotals,
    pub indexes: Vec<IndexStatus>,
    pub index_cursors: Vec<IndexCursor>,
    pub index_refresh: IndexRefreshDiagnostics,
    pub file_index: FileIndexDiagnostics,
}

/// Storage health surfaced to diagnostics.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StorageHealth {
    pub graph_version: GraphVersion,
    pub healthy: bool,
    pub detail: String,
}

#[cfg(test)]
#[path = "health_tests.rs"]
mod tests;
