use serde::{Deserialize, Serialize};

/// Machine-readable result for `relay-knowledge version check`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VersionCheckResponse {
    pub project_name: String,
    pub current_version: String,
    pub latest_version: Option<String>,
    pub update_available: bool,
    pub source: Option<String>,
    pub release_url: Option<String>,
    pub checked_at_unix_ms: u64,
    pub diagnostics: Vec<VersionCheckDiagnostic>,
}

/// Source-specific version-check diagnostic safe for CLI output.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VersionCheckDiagnostic {
    pub source: Option<String>,
    pub code: String,
    pub message: String,
    pub retryable: bool,
}

#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;
