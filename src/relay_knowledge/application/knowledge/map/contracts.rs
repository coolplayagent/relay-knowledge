//! Public request/response contracts and typed Knowledge Map mutation state.

use std::time::{SystemTime, UNIX_EPOCH};

use serde::Serialize;

use crate::{
    api::{ApiMetadata, RequestContext},
    domain::{
        DirectoryLoadHint, DirectoryUpdateRule, KnowledgeMap, KnowledgeMapHistoryEntry,
        KnowledgeMapRoute, KnowledgeMapSource, KnowledgeMapSourceKind, KnowledgeMapTopic,
        RepositoryMapDirectory, RepositoryMapType,
    },
};

use super::artifact::{KnowledgeMapArchiveRef, KnowledgeMapHistoryIndexRef};

pub(super) struct MutableKnowledgeMap {
    pub(super) map_type: RepositoryMapType,
    pub(super) directories: Vec<RepositoryMapDirectory>,
    pub(super) map: KnowledgeMap,
    pub(super) archived_through: u64,
    pub(super) archive: Option<KnowledgeMapArchiveRef>,
    pub(super) history_index: Option<KnowledgeMapHistoryIndexRef>,
    pub(super) requires_publish: bool,
}

impl MutableKnowledgeMap {
    pub(super) fn initial(map_type: RepositoryMapType, updated_at: String) -> Self {
        Self {
            map: match map_type {
                RepositoryMapType::Knowledge => KnowledgeMap::initial(updated_at),
                RepositoryMapType::Codespec => KnowledgeMap::empty(updated_at),
            },
            map_type,
            directories: baseline_directories(map_type),
            archived_through: 0,
            archive: None,
            history_index: None,
            requires_publish: false,
        }
    }
}

pub(super) fn baseline_directories(map_type: RepositoryMapType) -> Vec<RepositoryMapDirectory> {
    map_type
        .required_directories()
        .iter()
        .map(|directory| RepositoryMapDirectory {
            directory: (*directory).to_owned(),
            purpose: baseline_purpose(map_type, directory).to_owned(),
            content_scope: vec![format!("{}/{directory}/**", map_type.as_str())],
            key_files: vec![format!("{}/{directory}/README.md", map_type.as_str())],
            load_hint: DirectoryLoadHint::OnDemand,
            relations: Vec::new(),
            update_rule: DirectoryUpdateRule::Reviewed,
        })
        .collect()
}

fn baseline_purpose(map_type: RepositoryMapType, directory: &str) -> &'static str {
    match (map_type, directory) {
        (RepositoryMapType::Codespec, "requirements") => {
            "Product requirements and acceptance criteria."
        }
        (RepositoryMapType::Codespec, "design") => {
            "Architecture and implementation design records."
        }
        (RepositoryMapType::Codespec, "api") => "Public interface and schema contracts.",
        (RepositoryMapType::Codespec, "test") => "Verification strategy, fixtures, and evidence.",
        (RepositoryMapType::Codespec, "decisions") => "Durable architecture and product decisions.",
        (RepositoryMapType::Knowledge, "domain") => "Domain concepts, models, and business rules.",
        (RepositoryMapType::Knowledge, "guides") => "Task-oriented repository knowledge guides.",
        (RepositoryMapType::Knowledge, "ops") => "Operational procedures and diagnostics.",
        (RepositoryMapType::Knowledge, "glossary") => {
            "Business terminology, aliases, and technical mappings."
        }
        (RepositoryMapType::Knowledge, "best-practices") => {
            "Reviewed engineering and knowledge-management practices."
        }
        _ => "Repository navigation knowledge.",
    }
}

pub(super) fn metadata(context: &RequestContext) -> ApiMetadata {
    ApiMetadata::graph_only(context, crate::domain::GraphVersion::ZERO)
}

pub(super) fn now_stamp() -> String {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0);
    format!("unix:{seconds}")
}

/// Request to register a source in the repository knowledge map.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KnowledgeMapSourceAddRequest {
    pub id: String,
    pub topic: String,
    pub kind: KnowledgeMapSourceKind,
    pub uri: String,
    pub source_scope: Option<String>,
    pub description: Option<String>,
}

/// Response shared by map mutation commands.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct KnowledgeMapMutationResponse {
    pub metadata: ApiMetadata,
    pub path: String,
    pub map_type: RepositoryMapType,
    pub map_version: u64,
    pub summary: String,
}

/// Response returned by read-only map commands.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct KnowledgeMapShowResponse {
    pub metadata: ApiMetadata,
    pub path: String,
    pub map_type: RepositoryMapType,
    pub map: KnowledgeMapView,
}

/// Bounded assembled view returned by `map show`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct KnowledgeMapView {
    pub artifact_schema_version: u16,
    pub map_version: u64,
    pub updated_at: String,
    pub directories: Vec<RepositoryMapDirectory>,
    pub topics: Vec<KnowledgeMapTopic>,
    pub sources: Vec<KnowledgeMapSource>,
    pub routes: Vec<KnowledgeMapRoute>,
    pub history: KnowledgeMapHistoryWindow,
}

/// Recent history and the checkpoint for history intentionally omitted from a show response.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct KnowledgeMapHistoryWindow {
    pub archived_through: u64,
    pub complete: bool,
    pub recent: Vec<KnowledgeMapHistoryEntry>,
}

/// One explicitly bounded page of complete Knowledge Map history.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct KnowledgeMapHistoryResponse {
    pub metadata: ApiMetadata,
    pub path: String,
    pub map_type: RepositoryMapType,
    pub map_version: u64,
    pub from_version: u64,
    pub through_version: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_from_version: Option<u64>,
    pub entries: Vec<KnowledgeMapHistoryEntry>,
}

/// Response returned by topic routing commands.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct KnowledgeMapRouteResponse {
    pub metadata: ApiMetadata,
    pub path: String,
    pub map_type: RepositoryMapType,
    pub topic: String,
    pub route: Option<KnowledgeMapRoute>,
    pub sources: Vec<KnowledgeMapSource>,
}

/// Response returned by validation commands.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct KnowledgeMapValidationResponse {
    pub metadata: ApiMetadata,
    pub path: String,
    pub map_type: RepositoryMapType,
    pub valid: bool,
    pub diagnostics: Vec<String>,
}

/// Response that contains the AGENTS.md reference snippet.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct KnowledgeMapAgentSnippetResponse {
    pub metadata: ApiMetadata,
    pub snippet: String,
}
