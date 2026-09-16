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
    /// Structured direct type ownership extracted from syntax, never inferred at query time.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub type_owner: Option<CodeTypeOwner>,
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

/// Snapshot-local type identity shared by declarations and directly owned callables.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CodeTypeOwner {
    /// AST proof that this direct Java static member has no unmodelled inherited overload set.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub static_dispatch: Option<bool>,
    /// Language and lexical/module identity; does not depend on a display name search.
    pub identity: String,
    /// `declaration`, `direct_member`, or `trait_member`.
    pub relation: String,
    /// Source spelling retained for unresolved external or ambiguous owners.
    pub target_hint: String,
    /// Exact language/module/type lookup evidence for detached declarations.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lookup_identity: Option<String>,
    /// `lexical`, `rust_impl`, `go_receiver`, `cpp_qualified`, or `swift_extension`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub basis: Option<String>,
    /// `resolved`, `unresolved`, or `ambiguous`; missing on legacy JSON.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolution_state: Option<String>,
    /// Repository-relative targets proved by an explicit source import.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub target_paths: Vec<String>,
    /// Original explicit module/type target, preserving import aliases.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub import_target: Option<String>,
    /// Visibility relevant to a detached implementation importing this declaration.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub visibility: Option<String>,
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
    /// Exact call-site bytes; absent in snapshots exported before this field existed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub byte_range: Option<RepositoryCodeRange>,
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
    #[serde(default)]
    pub metadata: super::CodeConfigMetadata,
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
