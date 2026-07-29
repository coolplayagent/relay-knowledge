use crate::{
    api::{ApiStreamEvent, CodeRepositoryIndexStartResponse, RequestContext, StreamEventKind},
    application::RelayKnowledgeService,
    domain::{CodeIndexTaskState, CodeRepositorySelector},
};

use super::{CliError, OutputFormat, serialize_line};

#[derive(serde::Serialize)]
pub(super) struct CodeIndexWorkerRunResponse {
    pub(super) claimed: bool,
    pub(super) task: Option<crate::domain::CodeIndexTaskRecord>,
}

pub(super) fn render_index_worker_response(
    response: &CodeIndexWorkerRunResponse,
    format: OutputFormat,
) -> Result<String, CliError> {
    if format != OutputFormat::StreamingJson {
        return serialize_line(response);
    }
    let payload = serde_json::to_value(response)
        .map_err(|error| CliError::RenderFailed(error.to_string()))?;
    let events = [
        index_worker_event(StreamEventKind::Started, "index worker started", None),
        index_worker_event(StreamEventKind::Item, "index worker result", Some(payload)),
        index_worker_event(StreamEventKind::Completed, "index worker completed", None),
    ];
    let mut output = String::new();
    for event in events {
        output.push_str(&serialize_line(&event)?);
    }

    Ok(output)
}

pub(super) async fn finish_started_index_task(
    service: &RelayKnowledgeService,
    response: &mut CodeRepositoryIndexStartResponse,
    selector: CodeRepositorySelector,
    context: RequestContext,
    format: OutputFormat,
) -> Result<(), CliError> {
    let Some(task_id) = response.task.as_ref().map(|task| task.task_id.clone()) else {
        return Ok(());
    };
    if response.task.as_ref().map(|task| task.state) != Some(CodeIndexTaskState::Running) {
        let completed = service
            .run_code_index_task_once(Some(task_id), context.clone())
            .await
            .map_err(|error| CliError::api_failed(error, format))?;
        if let Some(task) = completed {
            response.task = Some(task);
        }
    }
    let requested_ref = selector.ref_selector.clone();
    let status = service
        .code_repository_status(selector.clone(), context)
        .await
        .map_err(|error| CliError::api_failed(error, format))?;
    response.status = status.status;
    response.scope = crate::api::CodeRepositoryScopeMetadata::from_status(
        &response.status,
        &selector,
        requested_ref,
    );
    response.checkpoint = status.checkpoint;

    Ok(())
}

fn index_worker_event(
    event: StreamEventKind,
    message: &str,
    payload: Option<serde_json::Value>,
) -> ApiStreamEvent {
    ApiStreamEvent {
        event,
        operation: "code.repo.index_worker".to_owned(),
        message: Some(message.to_owned()),
        project_name: None,
        runtime: None,
        payload,
        error_kind: None,
        metadata: None,
    }
}
