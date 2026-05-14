use crate::api::{ApiMetadata, ApiStreamEvent, ProjectStatusResponse, StreamEventKind};

use super::{CliError, OutputFormat};

/// Renders a project status response in the requested CLI format.
pub fn render_project_status(
    response: &ProjectStatusResponse,
    format: OutputFormat,
) -> Result<String, CliError> {
    match format {
        OutputFormat::Text => render_text("project.status", response),
        OutputFormat::Json => serialize_line(response),
        OutputFormat::Markdown => render_text("project.status", response),
        OutputFormat::StreamingJson => render_streaming_project_status(response),
    }
}

pub(super) fn render_response<T>(
    operation: &str,
    metadata: ApiMetadata,
    response: &T,
    format: OutputFormat,
) -> Result<String, CliError>
where
    T: serde::Serialize,
{
    match format {
        OutputFormat::Text => render_text(operation, response),
        OutputFormat::Json => serialize_line(response),
        OutputFormat::Markdown => render_text(operation, response),
        OutputFormat::StreamingJson => render_streaming_response(operation, metadata, response),
    }
}

pub(super) fn render_text<T>(operation: &str, response: &T) -> Result<String, CliError>
where
    T: serde::Serialize,
{
    let value = serde_json::to_value(response)
        .map_err(|error| CliError::RenderFailed(error.to_string()))?;
    let line = match operation {
        "project.status" => value["project_name"]
            .as_str()
            .unwrap_or("relay-knowledge")
            .to_owned(),
        "knowledge.ingest" => format!(
            "ingested graph_version={} evidence_count={}",
            value["metadata"]["graph_version"].as_u64().unwrap_or(0),
            value["receipt"]["evidence_count"].as_u64().unwrap_or(0)
        ),
        "knowledge.retrieve_context" => {
            format!(
                "results={}",
                value["results"].as_array().map_or(0, Vec::len)
            )
        }
        "graph.inspect" => format!(
            "graph_version={} entities={} evidence={} code_files={} code_symbols={} repo_code_files={} repo_code_symbols={}",
            value["graph"]["graph_version"].as_u64().unwrap_or(0),
            value["graph"]["entity_count"].as_u64().unwrap_or(0),
            value["graph"]["evidence_count"].as_u64().unwrap_or(0),
            value["graph"]["code_file_count"].as_u64().unwrap_or(0),
            value["graph"]["code_symbol_count"].as_u64().unwrap_or(0),
            value["repository_code_totals"]["indexed_file_count"]
                .as_u64()
                .unwrap_or(0),
            value["repository_code_totals"]["symbol_count"]
                .as_u64()
                .unwrap_or(0)
        ),
        "index.refresh" => format!(
            "refreshed_indexes={}",
            value["indexes"].as_array().map_or(0, Vec::len)
        ),
        "worker.status" => format!(
            "workers={}",
            value["workers"].as_array().map_or(0, Vec::len)
        ),
        "worker.run_once" => format!(
            "task={} proposals={}",
            value["task"]["task_id"].as_str().unwrap_or("none"),
            value["proposals"].as_array().map_or(0, Vec::len)
        ),
        "proposal.list" => format!(
            "proposals={}",
            value["proposals"].as_array().map_or(0, Vec::len)
        ),
        "proposal.show" => format!(
            "proposal={} conflicts={}",
            value["proposal"]["proposal_id"]
                .as_str()
                .unwrap_or("unknown"),
            value["conflicts"].as_array().map_or(0, Vec::len)
        ),
        "proposal.accept" | "proposal.reject" | "proposal.supersede" => format!(
            "proposal={} state={}",
            value["proposal"]["proposal_id"]
                .as_str()
                .unwrap_or("unknown"),
            value["proposal"]["state"].as_str().unwrap_or("unknown")
        ),
        "audit.query" => format!(
            "audit_events={}",
            value["events"].as_array().map_or(0, Vec::len)
        ),
        "provider.embedding.probe" => format!(
            "provider={} ok={} model={} dimension={}",
            value["provider"].as_str().unwrap_or("none"),
            value["ok"].as_bool().unwrap_or(false),
            value["model"].as_str().unwrap_or("unknown"),
            value["dimension"].as_u64().unwrap_or(0)
        ),
        "service.health" => format!(
            "healthy={} repo_code_files={} repo_code_symbols={}",
            value["healthy"].as_bool().unwrap_or(false),
            value["repository_code_totals"]["indexed_file_count"]
                .as_u64()
                .unwrap_or(0),
            value["repository_code_totals"]["symbol_count"]
                .as_u64()
                .unwrap_or(0)
        ),
        "service.status" => format!(
            "service={} mode={}",
            value["service_name"].as_str().unwrap_or("relay-knowledge"),
            value["mode"].as_str().unwrap_or("disabled")
        ),
        "code.repo.index" => format!(
            "indexed files={} symbols={} references={} chunks={} degraded={}",
            value["summary"]["indexed_file_count"].as_u64().unwrap_or(0),
            value["summary"]["symbol_count"].as_u64().unwrap_or(0),
            value["summary"]["reference_count"].as_u64().unwrap_or(0),
            value["summary"]["chunk_count"].as_u64().unwrap_or(0),
            value["summary"]["degraded_file_count"]
                .as_u64()
                .unwrap_or(0)
        ),
        "code.repo.scope_preview" => format!(
            "preview files={} bytes={} unsupported={} expected_degraded={}",
            value["preview"]["selected_file_count"]
                .as_u64()
                .unwrap_or(0),
            value["preview"]["selected_byte_count"]
                .as_u64()
                .unwrap_or(0),
            value["preview"]["unsupported_file_count"]
                .as_u64()
                .unwrap_or(0),
            value["preview"]["expected_degraded_file_count"]
                .as_u64()
                .unwrap_or(0)
        ),
        "code.repo.query" => format!(
            "results={}",
            value["results"].as_array().map_or(0, Vec::len)
        ),
        "code.repo.impact" => format!(
            "changed_in_scope={} results={}",
            value["path_groups"]["in_scope_changed_paths"]
                .as_array()
                .map_or(0, Vec::len),
            value["results"].as_array().map_or(0, Vec::len)
        ),
        "code.repo.status" => format!(
            "repo={} files={} symbols={} stale={}",
            value["status"]["alias"].as_str().unwrap_or(""),
            value["status"]["indexed_file_count"].as_u64().unwrap_or(0),
            value["status"]["symbol_count"].as_u64().unwrap_or(0),
            value["status"]["stale"].as_bool().unwrap_or(true)
        ),
        "code.repo.report" => format!(
            "repo={} files={} freshness={}",
            value["report"]["alias"].as_str().unwrap_or(""),
            value["report"]["indexed_file_count"].as_u64().unwrap_or(0),
            value["report"]["freshness_state"]
                .as_str()
                .unwrap_or("unknown")
        ),
        "service.plan" => format!(
            "service_plan={} path={}",
            value["plan"]["action"].as_str().unwrap_or("install"),
            value["plan"]["definition_path"].as_str().unwrap_or("")
        ),
        "service.definition.write" => format!(
            "service_definition_written={}",
            value["written"].as_bool().unwrap_or(false)
        ),
        "service.operator.status" | "service.operator.pause" | "service.operator.resume" => {
            format!(
                "operator={}",
                value["operator"]["state"].as_str().unwrap_or("disabled")
            )
        }
        "setup.doctor" => format!(
            "setup_configuration_ready={} live_health_checked={} checks={} actions={}",
            value["configuration_ready"].as_bool().unwrap_or(false),
            value["live_health_checked"].as_bool().unwrap_or(false),
            value["checks"].as_array().map_or(0, Vec::len),
            value["recommended_actions"].as_array().map_or(0, Vec::len)
        ),
        "setup.profile" => format!(
            "setup_profile={} env_vars={} commands={}",
            value["profile"].as_str().unwrap_or("unknown"),
            value["environment"].as_array().map_or(0, Vec::len),
            value["commands"].as_array().map_or(0, Vec::len)
        ),
        _ => operation.to_owned(),
    };

    Ok(format!("{line}\n"))
}

fn render_streaming_response<T>(
    operation: &str,
    metadata: ApiMetadata,
    response: &T,
) -> Result<String, CliError>
where
    T: serde::Serialize,
{
    let payload = serde_json::to_value(response)
        .map_err(|error| CliError::RenderFailed(error.to_string()))?;
    let events = [
        ApiStreamEvent::operation(
            StreamEventKind::Started,
            operation,
            metadata.clone(),
            Some("operation started"),
            None,
        ),
        ApiStreamEvent::operation(
            StreamEventKind::Item,
            operation,
            metadata.clone(),
            None,
            Some(payload),
        ),
        ApiStreamEvent::operation(
            StreamEventKind::Completed,
            operation,
            metadata,
            Some("operation completed"),
            None,
        ),
    ];
    let mut output = String::new();
    for event in events {
        output.push_str(&serialize_line(&event)?);
    }

    Ok(output)
}

#[cfg(test)]
#[path = "cli_render_tests.rs"]
mod cli_render_tests;

fn render_streaming_project_status(response: &ProjectStatusResponse) -> Result<String, CliError> {
    let events = [
        ApiStreamEvent::project_status(StreamEventKind::Started, response, Some("status started")),
        ApiStreamEvent::project_status(
            StreamEventKind::Progress,
            response,
            Some("runtime configuration loaded"),
        ),
        ApiStreamEvent::project_status(StreamEventKind::Item, response, None),
        ApiStreamEvent::project_status(
            StreamEventKind::Completed,
            response,
            Some("status completed"),
        ),
    ];
    let mut output = String::new();
    for event in events {
        output.push_str(&serialize_line(&event)?);
    }

    Ok(output)
}

pub(super) fn serialize_line<T>(value: &T) -> Result<String, CliError>
where
    T: serde::Serialize,
{
    let line =
        serde_json::to_string(value).map_err(|error| CliError::RenderFailed(error.to_string()))?;

    Ok(format!("{line}\n"))
}
