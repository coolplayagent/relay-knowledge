//! Storage contracts for local file-location indexes.

use serde::{Deserialize, Serialize};

use crate::domain::{EvidenceSpan, IndexKind, IndexState};

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
    pub processed_content_paths: std::collections::BTreeSet<String>,
    pub content_entries: Vec<FileContentEntry>,
    pub scan_error_count: usize,
    pub truncated: bool,
    pub content_truncated: bool,
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
    #[serde(default)]
    pub content_truncated: bool,
    pub indexed_content_count: usize,
    pub skipped_content_count: usize,
    pub unchanged_content_count: usize,
    pub stale_content_cursor_count: usize,
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
    pub indexed_content_count: usize,
    pub skipped_content_count: usize,
    pub unchanged_content_count: usize,
    pub stale_content_cursor_count: usize,
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

/// Text chunk extracted from an authorized local file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileContentChunk {
    pub chunk_index: usize,
    pub start_byte: u32,
    pub end_byte: u32,
    pub start_line: u32,
    pub end_line: u32,
    pub content: String,
}

/// Content read-model record produced by a bounded scan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileContentEntry {
    pub scope_id: String,
    pub root_id: String,
    pub path: String,
    pub relative_path: String,
    pub fingerprint: String,
    pub content_hash: String,
    pub indexed_at_ms: u64,
    pub graph_version: u64,
    pub chunks: Vec<FileContentChunk>,
    pub skipped_reason: Option<String>,
}

/// Scoped derived cursor state for one file-content read-model input.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileContentReadModelCursor {
    pub kind: IndexKind,
    pub source_scope: String,
    pub root_id: String,
    pub path: String,
    pub content_hash: String,
    pub indexed_graph_version: u64,
    pub state: IndexState,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stale_reason: Option<String>,
}

/// Deterministic candidate fact extracted from file content.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileKnowledgeFactCandidate {
    pub candidate_id: String,
    pub kind: String,
    pub subject: String,
    pub predicate: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub object: Option<String>,
    pub confidence_basis_points: u16,
    pub status: String,
    pub source_scope: String,
    pub source_path: String,
    pub span: EvidenceSpan,
    pub fingerprint: String,
    pub freshness_cursor: String,
}

/// Bounded file-content query.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileContentSearchRequest {
    pub query: String,
    pub source_scope: Option<String>,
    pub root_id: Option<String>,
    pub authorized_roots: Vec<FileIndexRoot>,
    pub limit: usize,
    pub timeout_ms: u64,
}

/// One file-content search hit with provenance and prompt-role isolation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FileContentSearchHit {
    pub scope_id: String,
    pub root_id: String,
    pub path: String,
    pub relative_path: String,
    pub chunk_id: String,
    pub content_role: String,
    pub excerpt: String,
    pub span: EvidenceSpan,
    pub fingerprint: String,
    pub content_hash: String,
    pub indexed_at_ms: u64,
    pub graph_version: u64,
    pub indexed_graph_version: u64,
    pub freshness_cursor: String,
    pub rank: usize,
    pub score: f64,
    pub ranking_signals: Vec<String>,
    pub fact_candidates: Vec<FileKnowledgeFactCandidate>,
}

/// Aggregated file-index diagnostics.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileIndexDiagnostics {
    pub root_count: usize,
    pub indexed_file_count: usize,
    pub missing_file_count: usize,
    pub indexed_content_count: usize,
    pub skipped_content_count: usize,
    pub unchanged_content_count: usize,
    pub stale_content_cursor_count: usize,
    pub scan_error_count: usize,
    pub truncated_root_count: usize,
    pub roots: Vec<FileIndexRootStatus>,
    pub content_cursors: Vec<FileContentReadModelCursor>,
}
