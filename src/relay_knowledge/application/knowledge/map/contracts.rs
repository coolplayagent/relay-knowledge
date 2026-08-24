//! Public request/response contracts and typed Knowledge Map mutation state.

use std::time::{SystemTime, UNIX_EPOCH};

use serde::Serialize;

use crate::{
    api::{ApiMetadata, RequestContext},
    domain::{KnowledgeMap, KnowledgeMapRoute, KnowledgeMapSource, KnowledgeMapSourceKind},
};

use super::artifact::KnowledgeMapArchiveRef;

pub(super) struct MutableKnowledgeMap {
    pub(super) map: KnowledgeMap,
    pub(super) archived_through: u64,
    pub(super) archive: Option<KnowledgeMapArchiveRef>,
}

impl MutableKnowledgeMap {
    pub(super) fn initial(updated_at: String) -> Self {
        Self {
            map: KnowledgeMap::initial(updated_at),
            archived_through: 0,
            archive: None,
        }
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
    pub map_version: u64,
    pub summary: String,
}

/// Response returned by read-only map commands.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct KnowledgeMapShowResponse {
    pub metadata: ApiMetadata,
    pub path: String,
    pub map: KnowledgeMap,
}

/// Response returned by topic routing commands.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct KnowledgeMapRouteResponse {
    pub metadata: ApiMetadata,
    pub path: String,
    pub topic: String,
    pub route: Option<KnowledgeMapRoute>,
    pub sources: Vec<KnowledgeMapSource>,
}

/// Response returned by validation commands.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct KnowledgeMapValidationResponse {
    pub metadata: ApiMetadata,
    pub path: String,
    pub valid: bool,
    pub diagnostics: Vec<String>,
}

/// Response that contains the AGENTS.md reference snippet.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct KnowledgeMapAgentSnippetResponse {
    pub metadata: ApiMetadata,
    pub snippet: String,
}
