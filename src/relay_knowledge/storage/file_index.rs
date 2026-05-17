//! Storage contracts for local file-location indexes.

use serde::{Deserialize, Serialize};

/// Authorized root scanned by the local file indexer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileIndexRoot {
    pub scope_id: String,
    pub root_id: String,
    pub root_path: String,
}

/// Indexed file-location record produced by a bounded scan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileIndexEntry {
    pub scope_id: String,
    pub root_id: String,
    pub path: String,
    pub relative_path: String,
    pub file_name: String,
    pub extension: Option<String>,
    pub parent_dir: String,
    pub size_bytes: u64,
    pub modified_at_ms: u64,
    pub fingerprint: String,
}

/// Full-root replacement produced by one scan pass.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileIndexRootUpdate {
    pub root: FileIndexRoot,
    pub entries: Vec<FileIndexEntry>,
    pub scan_error_count: usize,
    pub truncated: bool,
    pub last_error: Option<String>,
    pub now_ms: u64,
}

/// Root-level status surfaced to health and service diagnostics.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileIndexRootStatus {
    pub scope_id: String,
    pub root_id: String,
    pub root_path: String,
    pub indexed_file_count: usize,
    pub missing_file_count: usize,
    pub scan_error_count: usize,
    pub truncated: bool,
    pub last_indexed_at_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
}

/// Summary returned after one or more roots are indexed.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileIndexScanSummary {
    pub root_count: usize,
    pub indexed_file_count: usize,
    pub missing_file_count: usize,
    pub scan_error_count: usize,
    pub truncated_root_count: usize,
    pub roots: Vec<FileIndexRootStatus>,
}

/// Bounded file-location query.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileSearchRequest {
    pub query: String,
    pub source_scope: Option<String>,
    pub root_id: Option<String>,
    pub limit: usize,
    pub timeout_ms: u64,
}

/// One file-location search hit.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FileSearchHit {
    pub scope_id: String,
    pub root_id: String,
    pub path: String,
    pub relative_path: String,
    pub file_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub extension: Option<String>,
    pub parent_dir: String,
    pub size_bytes: u64,
    pub modified_at_ms: u64,
    pub status: String,
    pub rank: usize,
    pub score: f64,
}

/// Aggregated file-index diagnostics.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileIndexDiagnostics {
    pub root_count: usize,
    pub indexed_file_count: usize,
    pub missing_file_count: usize,
    pub scan_error_count: usize,
    pub truncated_root_count: usize,
    pub roots: Vec<FileIndexRootStatus>,
}
