//! Stable, language-independent diagnostics for skipped local source paths.

use serde::{Deserialize, Serialize};

/// The affected source boundary; directory counts never imply a known file count.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CodePathKind {
    File,
    Directory,
}

/// The local operation which failed before a source could be parsed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CodePathIoOperation {
    Metadata,
    Read,
    ReadDirectory,
    CheckRepositoryBoundary,
}

/// Isolatable errors, classified at the source boundary rather than by message text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CodePathIoErrorKind {
    NotFound,
    PermissionDenied,
    SharingViolation,
    Unsupported,
    InvalidPath,
}

/// The observable action taken after an isolatable source failure.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CodePathIoAction {
    #[default]
    Skipped,
}

/// Presence of this record means the path was skipped from this snapshot.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CodePathIoDiagnostic {
    #[serde(default)]
    pub action: CodePathIoAction,
    pub path_kind: CodePathKind,
    pub operation: CodePathIoOperation,
    pub error_kind: CodePathIoErrorKind,
    pub raw_os_error: Option<i32>,
}

impl super::CodeFileDiagnostic {
    /// Whether this skip diagnostic removes the exact file or its directory subtree.
    pub fn skips_path(&self, path: &str) -> bool {
        self.io.as_ref().is_some_and(|io| {
            path == self.path
                || (io.path_kind == CodePathKind::Directory
                    && path
                        .strip_prefix(&self.path)
                        .is_some_and(|suffix| suffix.starts_with('/')))
        })
    }
}
