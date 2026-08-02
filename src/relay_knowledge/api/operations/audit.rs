use serde::{Deserialize, Serialize};

use crate::{api::ApiMetadata, domain::AuditEventRecord};

/// Durable audit sink health surfaced in service diagnostics.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuditSinkStatus {
    pub durable: bool,
    pub event_count: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
}

/// Durable audit query request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuditQueryApiRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub operation: Option<String>,
    pub limit: usize,
}

/// Durable audit query response.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuditQueryResponse {
    pub metadata: ApiMetadata,
    pub events: Vec<AuditEventRecord>,
}
