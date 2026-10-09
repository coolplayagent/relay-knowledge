//! Web access to the shared feedback service; publication authority stays local.

use crate::{
    api::{ApiMetadata, RequestContext},
    application::{RelayKnowledgeService, feedback::public_record},
    domain::{GraphVersion, feedback::FeedbackReport},
};
use serde_json::{Value, json};

use super::{WebError, operation_request::string_field};

pub(super) async fn execute(
    service: &RelayKnowledgeService,
    operation: &str,
    payload: &Value,
    context: RequestContext,
) -> Result<(ApiMetadata, Value), WebError> {
    let feedback = service.feedback_service().map_err(WebError::bad_request)?;
    let result = match operation {
        "feedback.report" => {
            let report: FeedbackReport = serde_json::from_value(payload.get("report").cloned().unwrap_or(Value::Null))
                .map_err(|_| WebError::bad_request("report does not match feedback schema v1".into()))?;
            feedback.report(report, &context).await.map(|record| json!({"feedback":public_record(&record)}))
        }
        "feedback.status" => feedback.status(payload.get("id").and_then(Value::as_str)).await,
        "feedback.preview" => feedback.preview(string_field(payload, "id")?).await,
        "feedback.submit" | "feedback.retry" => feedback.submit(string_field(payload, "id")?).await.map(|record| json!({"feedback":public_record(&record)})),
        "feedback.track" => feedback.track(string_field(payload, "id")?).await.map(|record| json!({"feedback":public_record(&record)})),
        _ => return Err(WebError::bad_request("feedback policy and validation authority must be supplied through the local control plane".into())),
    }.map_err(WebError::bad_request)?;
    Ok((
        ApiMetadata::graph_only(&context, GraphVersion::ZERO),
        result,
    ))
}

#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;
