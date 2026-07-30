use serde::{Deserialize, Serialize};

use super::super::{CodeParseStatus, SymbolRole};
use super::RepositoryCodeRange;

/// File-level code index row.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepositoryCodeFileRecord {
    pub repository_id: String,
    pub source_scope: String,
    pub file_id: String,
    pub path: String,
    pub language_id: String,
    pub blob_hash: String,
    pub byte_len: usize,
    pub line_count: usize,
    pub parse_status: CodeParseStatus,
    #[serde(default)]
    pub is_generated: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub degraded_reason: Option<String>,
}

/// Previously indexed file hash used to skip unchanged incremental parses.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CodeFileFingerprint {
    pub path: String,
    pub blob_hash: String,
}

/// Symbol definition extracted from tree-sitter syntax.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepositoryCodeSymbolRecord {
    pub repository_id: String,
    pub source_scope: String,
    pub symbol_snapshot_id: String,
    pub canonical_symbol_id: String,
    pub file_id: String,
    pub path: String,
    pub language_id: String,
    pub name: String,
    pub qualified_name: String,
    pub kind: String,
    pub signature: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub doc_comment: Option<String>,
    pub byte_range: RepositoryCodeRange,
    pub line_range: RepositoryCodeRange,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub symbol_role: Option<SymbolRole>,
}

/// Reference extracted from tree-sitter syntax and optionally resolved.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepositoryCodeReferenceRecord {
    pub repository_id: String,
    pub source_scope: String,
    pub reference_id: String,
    pub file_id: String,
    pub path: String,
    pub name: String,
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_symbol_snapshot_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_hint: Option<String>,
    pub resolution_state: String,
    pub confidence_basis_points: u16,
    pub confidence_tier: String,
    pub byte_range: RepositoryCodeRange,
    pub line_range: RepositoryCodeRange,
}

/// Import relationship extracted from code.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CodeImportRecord {
    pub repository_id: String,
    pub source_scope: String,
    pub import_id: String,
    pub file_id: String,
    pub path: String,
    pub module: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_hint: Option<String>,
    pub resolution_state: String,
    pub confidence_basis_points: u16,
    pub confidence_tier: String,
    pub line_range: RepositoryCodeRange,
}

/// Call relationship extracted from code.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CodeCallRecord {
    pub repository_id: String,
    pub source_scope: String,
    pub call_id: String,
    pub file_id: String,
    pub path: String,
    pub caller_symbol_snapshot_id: Option<String>,
    pub caller_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub callee_symbol_snapshot_id: Option<String>,
    pub callee_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_hint: Option<String>,
    pub resolution_state: String,
    pub confidence_basis_points: u16,
    pub confidence_tier: String,
    pub line_range: RepositoryCodeRange,
}

/// Web framework route mapping extracted from source code.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CodeRouteRecord {
    pub repository_id: String,
    pub source_scope: String,
    pub route_id: String,
    pub file_id: String,
    pub path: String,
    pub language_id: String,
    pub url: String,
    /// Lowercase HTTP verb, or `any` when a framework route accepts all methods.
    pub http_method: String,
    pub handler_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub handler_symbol_snapshot_id: Option<String>,
    pub framework: String,
    pub line_range: RepositoryCodeRange,
}

/// Feature flag or runtime configuration relationship extracted from code.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CodeFeatureFlagRecord {
    pub repository_id: String,
    pub source_scope: String,
    pub feature_flag_id: String,
    pub usage_id: String,
    pub file_id: String,
    pub path: String,
    pub language_id: String,
    pub name: String,
    pub source_kind: String,
    pub source_key: String,
    pub edge_kind: String,
    pub confidence_basis_points: u16,
    pub confidence_tier: String,
    pub byte_range: RepositoryCodeRange,
    pub line_range: RepositoryCodeRange,
    pub excerpt: String,
}

/// Searchable code chunk.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepositoryCodeChunkRecord {
    pub repository_id: String,
    pub source_scope: String,
    pub chunk_id: String,
    pub file_id: String,
    pub path: String,
    pub language_id: String,
    pub content: String,
    pub byte_range: RepositoryCodeRange,
    pub line_range: RepositoryCodeRange,
    pub symbol_snapshot_id: Option<String>,
}

/// File-level diagnostic produced by indexing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CodeFileDiagnostic {
    pub repository_id: String,
    pub source_scope: String,
    pub path: String,
    pub parse_status: CodeParseStatus,
    pub message: String,
}

/// Rename/delete lineage marker retained after incremental updates.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CodePathTombstone {
    pub repository_id: String,
    pub source_scope: String,
    pub old_path: String,
    pub new_path: Option<String>,
    pub base_ref: String,
    pub head_ref: String,
}
