use serde::{Deserialize, Serialize};

use crate::{
    api::ApiMetadata,
    domain::{CommitReceipt, ProposalConflictRecord, ProposalRecord, ProposalState},
};

/// Proposal list filter.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProposalListApiRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub state: Option<ProposalState>,
    pub limit: usize,
}

/// Proposal list response.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProposalListResponse {
    pub metadata: ApiMetadata,
    pub proposals: Vec<ProposalRecord>,
}

/// Proposal detail response with conflict lifecycle.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProposalShowResponse {
    pub metadata: ApiMetadata,
    pub proposal: ProposalRecord,
    pub conflicts: Vec<ProposalConflictRecord>,
    pub payload: serde_json::Value,
}

/// Manual proposal decision request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProposalDecisionApiRequest {
    pub actor: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

/// Manual proposal decision response.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProposalDecisionResponse {
    pub metadata: ApiMetadata,
    pub proposal: ProposalRecord,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub receipt: Option<CommitReceipt>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub index_refresh_error: Option<String>,
}
