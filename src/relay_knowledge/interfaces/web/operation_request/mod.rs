use serde_json::Value;

use crate::{
    api::{
        CodeRepositoryRegisterRequest, GraphInspectionRequest, HybridRetrievalRequest,
        IndexRefreshRequest, IngestEvidence, IngestRequest, ProposalDecisionApiRequest,
    },
    domain::{
        CodeFeatureFlagRequest, CodeGraphContextRequest, CodeImpactRequest, CodeQueryKind,
        CodeRepositorySelector, CodeRepositorySetAddMemberRequest, CodeRepositorySetCreateRequest,
        CodeRepositorySetQueryRequest, CodeRepositorySetRemoveMemberRequest, CodeRetrievalRequest,
        FreshnessPolicy, IndexKind, ProposalState, SoftwareGlobalKind, SoftwareGlobalRequest,
        WorkerKind,
    },
};

use super::WebError;

#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;
pub(super) fn retrieve_request(payload: &Value) -> Result<HybridRetrievalRequest, WebError> {
    Ok(HybridRetrievalRequest {
        query: string_field(payload, "query")?.to_owned(),
        source_scope: optional_string_field(payload, "source_scope"),
        freshness: parse_freshness(string_field(payload, "freshness")?)?,
        limit: usize_field(payload, "limit")?,
    })
}

pub(super) fn ingest_request(payload: &Value) -> Result<IngestRequest, WebError> {
    Ok(IngestRequest {
        source_scope: string_field(payload, "source_scope")?.to_owned(),
        evidence: vec![IngestEvidence {
            id: None,
            source_path: None,
            span: None,
            confidence: None,
            status: None,
            content: string_field(payload, "content")?.to_owned(),
            entity_labels: string_array_field(payload, "entity_labels")?,
            extraction: None,
        }],
        relations: Vec::new(),
        claims: Vec::new(),
        events: Vec::new(),
    })
}

pub(super) fn graph_request(payload: &Value) -> GraphInspectionRequest {
    GraphInspectionRequest {
        source_scope: optional_string_field(payload, "source_scope"),
    }
}

pub(super) fn index_request(payload: &Value) -> Result<IndexRefreshRequest, WebError> {
    Ok(IndexRefreshRequest {
        kinds: string_array_field(payload, "kinds")?
            .into_iter()
            .map(|kind| parse_index_kind(&kind))
            .collect::<Result<Vec<_>, _>>()?,
    })
}

pub(super) struct KnowledgeMapHistoryPage {
    pub(super) repository: String,
    pub(super) from_version: u64,
    pub(super) limit: usize,
}

pub(super) fn knowledge_map_history_page(
    payload: &Value,
) -> Result<KnowledgeMapHistoryPage, WebError> {
    let repository = string_field(payload, "repository")?.trim().to_owned();
    let from_version = payload
        .get("from_version")
        .and_then(Value::as_u64)
        .filter(|value| *value > 0)
        .ok_or_else(|| {
            WebError::bad_request("from_version must be a positive integer".to_owned())
        })?;
    Ok(KnowledgeMapHistoryPage {
        repository,
        from_version,
        limit: usize_field(payload, "limit")?,
    })
}

pub(super) fn code_register_request(
    payload: &Value,
) -> Result<CodeRepositoryRegisterRequest, WebError> {
    Ok(CodeRepositoryRegisterRequest {
        root_path: string_field(payload, "root_path")?.to_owned(),
        alias: code_register_alias(payload)?,
        path_filters: optional_string_array_field(payload, "path_filters")?,
        language_filters: optional_string_array_field(payload, "language_filters")?,
    })
}

fn code_register_alias(payload: &Value) -> Result<String, WebError> {
    match payload.get("alias") {
        Some(Value::String(alias)) => Ok(alias.trim().to_owned()),
        Some(_) => Err(WebError::bad_request("alias must be a string".to_owned())),
        None => Ok(String::new()),
    }
}

pub(super) fn code_query_request(payload: &Value) -> Result<CodeRetrievalRequest, WebError> {
    let mut request = CodeRetrievalRequest::new(
        string_field(payload, "query")?,
        code_selector(payload)?,
        parse_code_query_kind(string_field(payload, "kind")?)?,
        usize_field(payload, "limit")?,
        parse_freshness(string_field(payload, "freshness")?)?,
    )
    .map_err(|error| WebError::bad_request(error.to_string()))?;
    request.exclude_generated = optional_bool_field(payload, "exclude_generated")?.unwrap_or(false);
    Ok(request)
}

pub(super) fn code_context_request(payload: &Value) -> Result<CodeGraphContextRequest, WebError> {
    CodeGraphContextRequest::new(
        code_selector(payload)?,
        string_field(payload, "query")?,
        usize_field(payload, "limit")?,
        parse_freshness(string_field(payload, "freshness")?)?,
        usize_field(payload, "max_context_bytes")?,
        optional_bool_field(payload, "include_code")?.unwrap_or(true),
        optional_bool_field(payload, "exclude_generated")?.unwrap_or(false),
    )
    .map_err(|error| WebError::bad_request(error.to_string()))
}

pub(super) fn code_feature_flag_request(
    payload: &Value,
) -> Result<CodeFeatureFlagRequest, WebError> {
    CodeFeatureFlagRequest::new(
        optional_string_field(payload, "query"),
        code_selector(payload)?,
        usize_field(payload, "limit")?,
        parse_freshness(string_field(payload, "freshness")?)?,
    )
    .map_err(|error| WebError::bad_request(error.to_string()))
}

pub(super) fn code_impact_request(payload: &Value) -> Result<CodeImpactRequest, WebError> {
    CodeImpactRequest::new(
        code_selector(payload)?,
        string_field(payload, "base_ref")?,
        string_field(payload, "head_ref")?,
        usize_field(payload, "limit")?,
    )
    .map_err(|error| WebError::bad_request(error.to_string()))
}

pub(super) fn code_software_request(payload: &Value) -> Result<SoftwareGlobalRequest, WebError> {
    SoftwareGlobalRequest::new(
        code_selector(payload)?,
        parse_software_kind(string_field(payload, "kind")?)?,
        parse_freshness(string_field(payload, "freshness")?)?,
        usize_field(payload, "limit")?,
    )
    .map_err(|error| WebError::bad_request(error.to_string()))
}

pub(super) fn code_selector(payload: &Value) -> Result<CodeRepositorySelector, WebError> {
    CodeRepositorySelector::new(
        string_field(payload, "alias")?,
        optional_string_field(payload, "ref").unwrap_or_else(|| "HEAD".to_owned()),
        optional_string_array_field(payload, "path_filters")?,
        optional_string_array_field(payload, "language_filters")?,
    )
    .map_err(|error| WebError::bad_request(error.to_string()))
}

pub(super) fn code_repository_set_create_request(
    payload: &Value,
) -> Result<CodeRepositorySetCreateRequest, WebError> {
    CodeRepositorySetCreateRequest::new(
        string_field(payload, "set_alias")?,
        optional_string_field(payload, "description"),
        optional_string_field(payload, "default_ref_policy_json"),
    )
    .map_err(|error| WebError::bad_request(error.to_string()))
}

pub(super) fn code_repository_set_add_request(
    payload: &Value,
) -> Result<CodeRepositorySetAddMemberRequest, WebError> {
    CodeRepositorySetAddMemberRequest::new(
        string_field(payload, "set_alias")?,
        string_field(payload, "repository_alias")?,
        string_field(payload, "ref")?,
        optional_string_array_field(payload, "path_filters")?,
        optional_string_array_field(payload, "language_filters")?,
        optional_i32_field(payload, "priority")?.unwrap_or(0),
    )
    .map_err(|error| WebError::bad_request(error.to_string()))
}

pub(super) fn code_repository_set_remove_request(
    payload: &Value,
) -> Result<CodeRepositorySetRemoveMemberRequest, WebError> {
    CodeRepositorySetRemoveMemberRequest::new(
        string_field(payload, "set_alias")?,
        string_field(payload, "repository_alias")?,
    )
    .map_err(|error| WebError::bad_request(error.to_string()))
}

pub(super) fn code_repository_set_query_request(
    payload: &Value,
) -> Result<CodeRepositorySetQueryRequest, WebError> {
    let mut request = CodeRepositorySetQueryRequest::new(
        string_field(payload, "set_alias")?,
        string_field(payload, "query")?,
        parse_code_query_kind(string_field(payload, "kind")?)?,
        usize_field(payload, "limit")?,
        parse_freshness(string_field(payload, "freshness")?)?,
        optional_string_array_field(payload, "path_filters")?,
        optional_string_array_field(payload, "language_filters")?,
    )
    .map_err(|error| WebError::bad_request(error.to_string()))?;
    request.exclude_generated = optional_bool_field(payload, "exclude_generated")?.unwrap_or(false);
    Ok(request)
}

pub(super) fn string_field<'a>(
    payload: &'a Value,
    field: &'static str,
) -> Result<&'a str, WebError> {
    payload
        .get(field)
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| WebError::bad_request(format!("{field} is required")))
}

pub(super) fn optional_string_field(payload: &Value, field: &'static str) -> Option<String> {
    payload
        .get(field)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

fn string_array_field(payload: &Value, field: &'static str) -> Result<Vec<String>, WebError> {
    payload
        .get(field)
        .and_then(Value::as_array)
        .ok_or_else(|| WebError::bad_request(format!("{field} must be an array")))?
        .iter()
        .map(|item| {
            item.as_str()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(ToOwned::to_owned)
                .ok_or_else(|| {
                    WebError::bad_request(format!("{field} contains a non-string value"))
                })
        })
        .collect()
}

pub(super) fn optional_string_array_field(
    payload: &Value,
    field: &'static str,
) -> Result<Vec<String>, WebError> {
    if payload.get(field).is_none() {
        return Ok(Vec::new());
    }

    string_array_field(payload, field)
}

pub(super) fn usize_field(payload: &Value, field: &'static str) -> Result<usize, WebError> {
    payload
        .get(field)
        .and_then(Value::as_u64)
        .and_then(|value| usize::try_from(value).ok())
        .filter(|value| *value > 0)
        .ok_or_else(|| WebError::bad_request(format!("{field} must be a positive integer")))
}

fn i32_field(payload: &Value, field: &'static str) -> Result<i32, WebError> {
    payload
        .get(field)
        .and_then(Value::as_i64)
        .and_then(|value| i32::try_from(value).ok())
        .ok_or_else(|| WebError::bad_request(format!("{field} must be an integer")))
}

fn optional_i32_field(payload: &Value, field: &'static str) -> Result<Option<i32>, WebError> {
    if payload.get(field).is_none() {
        return Ok(None);
    }

    i32_field(payload, field).map(Some)
}

pub(super) fn optional_bool_field(
    payload: &Value,
    field: &'static str,
) -> Result<Option<bool>, WebError> {
    if payload.get(field).is_none() {
        return Ok(None);
    }

    payload
        .get(field)
        .and_then(Value::as_bool)
        .map(Some)
        .ok_or_else(|| WebError::bad_request(format!("{field} must be a boolean")))
}

pub(super) fn parse_freshness(value: &str) -> Result<FreshnessPolicy, WebError> {
    match value {
        "allow-stale" => Ok(FreshnessPolicy::AllowStale),
        "wait-until-fresh" => Ok(FreshnessPolicy::WaitUntilFresh),
        "graph-only" => Ok(FreshnessPolicy::GraphOnly),
        other => Err(WebError::bad_request(format!(
            "unsupported freshness '{other}'"
        ))),
    }
}

fn parse_index_kind(value: &str) -> Result<IndexKind, WebError> {
    match value {
        "bm25" => Ok(IndexKind::Bm25),
        "semantic" => Ok(IndexKind::Semantic),
        "vector" => Ok(IndexKind::Vector),
        other => Err(WebError::bad_request(format!(
            "unsupported index kind '{other}'"
        ))),
    }
}

fn parse_code_query_kind(value: &str) -> Result<CodeQueryKind, WebError> {
    match value {
        "hybrid" => Ok(CodeQueryKind::Hybrid),
        "symbol" => Ok(CodeQueryKind::Symbol),
        "definition" => Ok(CodeQueryKind::Definition),
        "references" => Ok(CodeQueryKind::References),
        "callers" => Ok(CodeQueryKind::Callers),
        "callees" => Ok(CodeQueryKind::Callees),
        "imports" => Ok(CodeQueryKind::Imports),
        "sbom" => Ok(CodeQueryKind::Sbom),
        other => Err(WebError::bad_request(format!(
            "unsupported code query kind '{other}'"
        ))),
    }
}

fn parse_software_kind(value: &str) -> Result<SoftwareGlobalKind, WebError> {
    match value {
        "dependencies" => Ok(SoftwareGlobalKind::Dependencies),
        "sdks" => Ok(SoftwareGlobalKind::Sdks),
        "files" => Ok(SoftwareGlobalKind::Files),
        "topics" => Ok(SoftwareGlobalKind::Topics),
        "relationships" => Ok(SoftwareGlobalKind::Relationships),
        "build" => Ok(SoftwareGlobalKind::Build),
        "iac" => Ok(SoftwareGlobalKind::Iac),
        "design" => Ok(SoftwareGlobalKind::Design),
        "all" => Ok(SoftwareGlobalKind::All),
        other => Err(WebError::bad_request(format!(
            "unsupported software kind '{other}'"
        ))),
    }
}

pub(super) fn optional_worker_kind(payload: &Value) -> Result<Option<WorkerKind>, WebError> {
    optional_string_field(payload, "kind")
        .map(|kind| {
            WorkerKind::parse(&kind)
                .map_err(|_| WebError::bad_request(format!("unsupported worker kind '{kind}'")))
        })
        .transpose()
}

pub(super) fn optional_proposal_state(payload: &Value) -> Result<Option<ProposalState>, WebError> {
    optional_string_field(payload, "state")
        .map(|state| {
            ProposalState::parse(&state)
                .map_err(|_| WebError::bad_request(format!("unsupported proposal state '{state}'")))
        })
        .transpose()
}

pub(super) fn proposal_decision_request(
    payload: &Value,
) -> Result<ProposalDecisionApiRequest, WebError> {
    Ok(ProposalDecisionApiRequest {
        actor: string_field(payload, "actor")?.to_owned(),
        reason: optional_string_field(payload, "reason"),
    })
}
