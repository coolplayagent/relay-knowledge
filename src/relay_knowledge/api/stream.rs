use serde::{Deserialize, Serialize};

use super::{ApiMetadata, ErrorKind, ProjectStatusResponse, RuntimeStatus};

/// Stream event categories for newline-delimited JSON output.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum StreamEventKind {
    Started,
    Progress,
    Item,
    Completed,
    Failed,
}

/// A single streaming API event. Each serialized event is one NDJSON line.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApiStreamEvent {
    pub event: StreamEventKind,
    pub operation: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub runtime: Option<RuntimeStatus>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_kind: Option<ErrorKind>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<ApiMetadata>,
}

impl ApiStreamEvent {
    /// Creates a stream event for the project status operation.
    pub fn project_status(
        event: StreamEventKind,
        response: &ProjectStatusResponse,
        message: Option<&str>,
    ) -> Self {
        Self {
            event,
            operation: "project.status".to_owned(),
            message: message.map(str::to_owned),
            project_name: (event == StreamEventKind::Item).then(|| response.project_name.clone()),
            runtime: (event == StreamEventKind::Item).then(|| response.runtime.clone()),
            error_kind: None,
            metadata: Some(response.metadata.clone()),
        }
    }
}
