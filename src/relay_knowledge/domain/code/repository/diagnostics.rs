//! Snapshot-bound content integrity and bounded diagnostic paging contracts.

use super::{CodeFileDiagnostic, CodeRepositorySelector};
use serde::{Deserialize, Serialize};

/// Content coverage is independent of indexed-version freshness.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CodeContentIntegrityState {
    Complete,
    Partial,
    #[default]
    Unknown,
}

/// Coverage of the served immutable code snapshot, never inferred from messages.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CodeContentIntegrity {
    pub state: CodeContentIntegrityState,
    pub degraded_file_count: Option<usize>,
    pub source_scope: Option<String>,
}

impl CodeContentIntegrity {
    /// Associates a counted set of distinct degraded paths with its snapshot.
    pub fn measured(source_scope: String, count: usize) -> Self {
        Self {
            state: if count == 0 {
                CodeContentIntegrityState::Complete
            } else {
                CodeContentIntegrityState::Partial
            },
            degraded_file_count: Some(count),
            source_scope: Some(source_scope),
        }
    }

    /// Preserves conservative coverage when context combines different snapshots.
    pub fn merge(&mut self, other: &Self) {
        if self == other {
            return;
        }
        if self.source_scope == other.source_scope {
            self.degraded_file_count = self
                .degraded_file_count
                .zip(other.degraded_file_count)
                .map(|(a, b)| a.max(b));
        } else {
            self.source_scope = None;
            self.degraded_file_count = None;
        }
        self.state = if self.state == CodeContentIntegrityState::Partial
            || other.state == CodeContentIntegrityState::Partial
        {
            CodeContentIntegrityState::Partial
        } else {
            CodeContentIntegrityState::Unknown
        };
    }
}

/// Public selector for a bounded diagnostic page.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CodeDiagnosticsRequest {
    pub repository: CodeRepositorySelector,
    pub limit: usize,
    pub cursor: Option<String>,
}

impl CodeDiagnosticsRequest {
    /// Canonicalizes bounded repository-relative prefixes before cursor binding.
    pub fn normalize_paths(&mut self) -> Result<(), String> {
        if self.repository.path_filters.len() > 64 {
            return Err("at most 64 diagnostic path filters are supported".into());
        }
        let mut paths = Vec::new();
        let mut whole_scope = false;
        for raw in &self.repository.path_filters {
            if raw.len() > 4096 {
                return Err("diagnostic path filter exceeds 4096 bytes".into());
            }
            let value = raw.replace('\\', "/");
            if value.starts_with('/') || value.contains(':') || value.split('/').any(|p| p == "..")
            {
                return Err("diagnostic paths must be repository-relative prefixes".into());
            }
            let value = value
                .split('/')
                .filter(|p| !p.is_empty() && *p != ".")
                .collect::<Vec<_>>()
                .join("/");
            if value.is_empty() {
                whole_scope = true;
                continue;
            }
            paths.push(value);
        }
        if whole_scope {
            paths.clear();
        }
        paths.sort();
        paths.dedup();
        self.repository.path_filters = paths;
        Ok(())
    }

    /// Rejects oversized pages and tokens at every entry point, including storage.
    pub fn validate(&self) -> Result<(), String> {
        if !(1..=200).contains(&self.limit) {
            return Err("diagnostic limit must be between 1 and 200".into());
        }
        if self.cursor.as_ref().is_some_and(|c| c.len() > 16384) {
            return Err("diagnostic cursor exceeds 16384 bytes".into());
        }
        if !self.repository.language_filters.is_empty() {
            return Err("diagnostics do not accept language filters".into());
        }
        Ok(())
    }
}

/// Keyset continuation bound to the original selector and immutable served scope.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CodeDiagnosticsCursor {
    pub repository_id: String,
    pub source_scope: String,
    pub resolved_commit_sha: String,
    pub requested_ref: String,
    pub path_filters: Vec<String>,
    pub after_path: String,
    pub after_message: String,
}

/// Storage request resolved and authorized by the application service.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodeDiagnosticsPageRequest {
    pub repository_id: String,
    pub source_scope: String,
    pub path_filters: Vec<String>,
    pub limit: usize,
    pub after: Option<(String, String)>,
}

/// Diagnostic rows with a distinct-file total and explicit continuation signal.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CodeDiagnosticsPage {
    pub degraded_file_count: usize,
    pub diagnostics: Vec<CodeFileDiagnostic>,
    pub has_more: bool,
}

#[cfg(test)]
mod mod_tests;
