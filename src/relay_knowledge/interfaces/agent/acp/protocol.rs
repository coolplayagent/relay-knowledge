use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::api::{AgentRetrievalResult, CodeGraphContextResponse, RuntimeIdentity};

/// ACP initialize response with relay-knowledge capability metadata.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AcpInitializeResponse {
    #[serde(rename = "_meta")]
    pub meta: AcpInitializeMeta,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AcpInitializeMeta {
    #[serde(rename = "relayKnowledge")]
    pub relay_knowledge: AcpRelayKnowledgeCapability,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AcpRelayKnowledgeCapability {
    #[serde(rename = "graphRetrieval")]
    pub graph_retrieval: bool,
    #[serde(rename = "readOnly")]
    pub read_only: bool,
    #[serde(rename = "supportsCancellation")]
    pub supports_cancellation: bool,
    #[serde(rename = "supportsIndexRefreshPermission")]
    pub supports_index_refresh_permission: bool,
}

/// Local ACP session request.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AcpSessionRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub actor_id: Option<String>,
}

/// Created ACP session metadata.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AcpSession {
    pub session_id: String,
    pub runtime_identity: RuntimeIdentity,
    pub policy_id: String,
    pub authorized_scope_count: usize,
}

/// ACP prompt request with structured relay metadata.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AcpPromptRequest {
    pub prompt: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_id: Option<String>,
    #[serde(rename = "_meta", skip_serializing_if = "Option::is_none")]
    pub meta: Option<AcpPromptMeta>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AcpPromptMeta {
    #[serde(rename = "relayKnowledge", skip_serializing_if = "Option::is_none")]
    pub relay_knowledge: Option<AcpRelayKnowledgePrompt>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AcpRelayKnowledgePrompt {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub query: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_scope: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repository: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ref_selector: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub path_filters: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub language_filters: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub freshness: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_context_bytes: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub include_code: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exclude_generated: Option<bool>,
}

/// ACP prompt response containing bounded progress and an optional context artifact.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AcpPromptResponse {
    pub session_id: String,
    pub request_id: String,
    pub updates: Vec<AcpSessionUpdate>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context_artifact: Option<AcpContextArtifact>,
    pub stop_reason: AcpStopReason,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<AcpErrorPayload>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AcpContextArtifact {
    pub artifact_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<AgentRetrievalResult>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub codegraph_context: Option<CodeGraphContextResponse>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AcpStopReason {
    Completed,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AcpErrorPayload {
    pub error_kind: String,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AcpSessionUpdate {
    pub request_id: String,
    pub kind: AcpSessionUpdateKind,
    pub status: AcpSessionUpdateStatus,
    pub message: String,
    #[serde(rename = "_meta", skip_serializing_if = "Option::is_none")]
    pub meta: Option<Value>,
}

impl AcpSessionUpdate {
    pub(super) fn pending(request_id: &str, message: &str) -> Self {
        Self::new(
            request_id,
            AcpSessionUpdateKind::ToolCallUpdate,
            AcpSessionUpdateStatus::Pending,
            message,
            None,
        )
    }

    pub(super) fn in_progress(request_id: &str, message: &str) -> Self {
        Self::new(
            request_id,
            AcpSessionUpdateKind::ToolCallUpdate,
            AcpSessionUpdateStatus::InProgress,
            message,
            None,
        )
    }

    pub(super) fn meta(request_id: &str, message: &str, meta: Value) -> Self {
        Self::new(
            request_id,
            AcpSessionUpdateKind::SessionUpdate,
            AcpSessionUpdateStatus::InProgress,
            message,
            Some(meta),
        )
    }

    pub(super) fn completed(request_id: &str, message: &str) -> Self {
        Self::new(
            request_id,
            AcpSessionUpdateKind::ToolCallUpdate,
            AcpSessionUpdateStatus::Completed,
            message,
            None,
        )
    }

    pub(super) fn failed(request_id: &str, message: &str, status: AcpSessionUpdateStatus) -> Self {
        Self::new(
            request_id,
            AcpSessionUpdateKind::ToolCallUpdate,
            status,
            message,
            None,
        )
    }

    fn new(
        request_id: &str,
        kind: AcpSessionUpdateKind,
        status: AcpSessionUpdateStatus,
        message: &str,
        meta: Option<Value>,
    ) -> Self {
        Self {
            request_id: request_id.to_owned(),
            kind,
            status,
            message: message.to_owned(),
            meta,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AcpSessionUpdateKind {
    SessionUpdate,
    ToolCallUpdate,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AcpSessionUpdateStatus {
    Pending,
    InProgress,
    Completed,
    Failed,
    Cancelled,
}

#[cfg(test)]
#[path = "protocol_tests.rs"]
mod tests;
