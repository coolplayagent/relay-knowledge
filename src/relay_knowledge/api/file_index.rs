use serde::{Deserialize, Serialize};

use crate::domain::FreshnessPolicy;

/// Freshness state for local file-index answers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FileIndexFreshnessState {
    Fresh,
    Pending,
    Paused,
    Stale,
    Degraded,
    Overflow,
}

/// Bounded-scan cursor for one local file-index root.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileIndexFreshnessCursor {
    pub source_scope: String,
    pub root_id: String,
    pub root_path: String,
    pub backend: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scan_watermark_ms: Option<u64>,
    pub indexed_file_count: usize,
    pub missing_file_count: usize,
    pub scan_error_count: usize,
    pub overflow: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
}

/// Root and file-count lag visible before a caller trusts file-index hits.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileIndexLag {
    pub configured_root_count: usize,
    pub indexed_root_count: usize,
    pub pending_root_count: usize,
    pub stale_root_count: usize,
    pub overflow_root_count: usize,
    pub missing_file_count: usize,
    pub pending_task_count: usize,
}

/// Freshness governance fields returned with local file-index responses.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileIndexFreshnessDiagnostics {
    pub state: FileIndexFreshnessState,
    pub freshness_policy: FreshnessPolicy,
    pub graph_version: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_scope: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub root_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stale_reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub degraded_reason: Option<String>,
    pub index_lag: FileIndexLag,
    pub cursors: Vec<FileIndexFreshnessCursor>,
    pub direct_source_read_required: bool,
    pub bounded_rescan_required: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub direct_source_read_paths: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub agent_instructions: Vec<String>,
}

impl FileIndexFreshnessDiagnostics {
    pub fn legacy_unknown() -> Self {
        Self {
            state: FileIndexFreshnessState::Degraded,
            freshness_policy: FreshnessPolicy::AllowStale,
            graph_version: 0,
            source_scope: None,
            root_id: None,
            stale_reason: None,
            degraded_reason: Some("response did not include file freshness diagnostics".to_owned()),
            index_lag: FileIndexLag::default(),
            cursors: Vec::new(),
            direct_source_read_required: false,
            bounded_rescan_required: false,
            direct_source_read_paths: Vec::new(),
            agent_instructions: Vec::new(),
        }
    }
}
