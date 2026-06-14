use serde_json::{Value, json};

use crate::{
    api::{FileContentQueryRequest, FileIndexRequest, FileQueryRequest, RequestContext},
    application::RelayKnowledgeService,
    domain::FreshnessPolicy,
};

use super::{
    WebError, optional_string_array_field, optional_string_field, parse_freshness, string_field,
    usize_field,
};

pub(super) async fn dispatch_file_operation(
    service: &RelayKnowledgeService,
    operation: &str,
    payload: &Value,
    context: RequestContext,
) -> Result<(crate::api::ApiMetadata, Value), WebError> {
    match operation {
        "files.index" => {
            let response = service
                .index_files(file_index_request(payload)?, context)
                .await?;
            Ok((response.metadata.clone(), json!(response)))
        }
        "files.query" => {
            let response = service
                .query_files(file_query_request(payload)?, context)
                .await?;
            Ok((response.metadata.clone(), json!(response)))
        }
        "files.content" => {
            let response = service
                .query_file_content(file_content_request(payload)?, context)
                .await?;
            Ok((response.metadata.clone(), json!(response)))
        }
        _ => Err(WebError::bad_request(format!(
            "unsupported file operation '{operation}'"
        ))),
    }
}

pub(super) fn file_index_request(payload: &Value) -> Result<FileIndexRequest, WebError> {
    Ok(FileIndexRequest {
        source_scope: optional_string_field(payload, "source_scope"),
        roots: optional_string_array_field(payload, "roots")?,
    })
}

pub(super) fn file_query_request(payload: &Value) -> Result<FileQueryRequest, WebError> {
    Ok(FileQueryRequest {
        query: string_field(payload, "query")?.to_owned(),
        source_scope: optional_string_field(payload, "source_scope"),
        root_id: optional_string_field(payload, "root_id"),
        limit: usize_field(payload, "limit")?,
        freshness_policy: optional_string_field(payload, "freshness")
            .map(|value| parse_freshness(&value))
            .transpose()?
            .unwrap_or(FreshnessPolicy::AllowStale),
    })
}

pub(super) fn file_content_request(payload: &Value) -> Result<FileContentQueryRequest, WebError> {
    Ok(FileContentQueryRequest {
        query: string_field(payload, "query")?.to_owned(),
        source_scope: optional_string_field(payload, "source_scope"),
        root_id: optional_string_field(payload, "root_id"),
        limit: usize_field(payload, "limit")?,
        freshness_policy: optional_string_field(payload, "freshness")
            .map(|value| parse_freshness(&value))
            .transpose()?
            .unwrap_or(FreshnessPolicy::AllowStale),
    })
}
