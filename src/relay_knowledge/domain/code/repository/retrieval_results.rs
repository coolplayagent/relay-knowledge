use serde::{Deserialize, Serialize};

use super::super::staleness::StalenessHint;
use super::{CodeRetrievalLayer, RepositoryCodeRange};

/// Diff paths split by the effective repository selector.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CodeImpactPathGroups {
    pub in_scope_changed_paths: Vec<String>,
    pub out_of_scope_changed_paths: Vec<String>,
}

/// Code retrieval hit with source location, layers, and freshness metadata.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CodeRetrievalHit {
    pub repository_id: String,
    pub scope_id: String,
    pub resolved_commit_sha: String,
    pub tree_hash: String,
    pub path: String,
    pub language_id: String,
    pub byte_range: RepositoryCodeRange,
    pub line_range: RepositoryCodeRange,
    pub symbol_snapshot_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub canonical_symbol_id: Option<String>,
    pub file_id: Option<String>,
    pub retrieval_layers: Vec<CodeRetrievalLayer>,
    pub index_versions: Vec<String>,
    pub stale: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub staleness_hint: Option<StalenessHint>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub degraded_reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub edge_kind: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub edge_resolution_state: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub edge_target_hint: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub edge_confidence_basis_points: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub edge_confidence_tier: Option<String>,
    pub score: f64,
    pub excerpt: String,
}

/// One code location where a feature flag is defined, read, or guards code.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CodeFeatureFlagUsage {
    pub usage_id: String,
    pub path: String,
    pub language_id: String,
    pub file_id: String,
    pub byte_range: RepositoryCodeRange,
    pub line_range: RepositoryCodeRange,
    pub edge_kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub related_symbol_snapshot_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub related_symbol_name: Option<String>,
    pub confidence_basis_points: u16,
    pub confidence_tier: String,
    pub excerpt: String,
}

/// Feature flag graph grouped by stable configuration source.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CodeFeatureFlagGraph {
    pub feature_flag_id: String,
    pub name: String,
    pub source_kind: String,
    pub source_key: String,
    pub score: f64,
    pub usages: Vec<CodeFeatureFlagUsage>,
}
