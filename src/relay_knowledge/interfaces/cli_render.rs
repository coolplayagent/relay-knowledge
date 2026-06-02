use crate::{
    api::{ApiMetadata, ApiStreamEvent, ProjectStatusResponse, StreamEventKind},
    project::KNOWLEDGE_MAP_RELATIVE_PATH,
};

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
        "files.index" => format!(
            "file_roots={} indexed_files={} missing_files={} scan_errors={} truncated_roots={}",
            value["summary"]["root_count"].as_u64().unwrap_or(0),
            value["summary"]["indexed_file_count"].as_u64().unwrap_or(0),
            value["summary"]["missing_file_count"].as_u64().unwrap_or(0),
            value["summary"]["scan_error_count"].as_u64().unwrap_or(0),
            value["summary"]["truncated_root_count"]
                .as_u64()
                .unwrap_or(0)
        ),
        "files.query" => format!(
            "results={} truncated={} duration_ms={}",
            value["results"].as_array().map_or(0, Vec::len),
            value["truncated"].as_bool().unwrap_or(false),
            value["duration_ms"].as_u64().unwrap_or(0)
        ),
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
        "knowledge.map.init"
        | "knowledge.map.source.add"
        | "knowledge.map.source.update"
        | "knowledge.map.source.remove" => format!(
            "knowledge_map={} version={}",
            value["path"]
                .as_str()
                .unwrap_or(KNOWLEDGE_MAP_RELATIVE_PATH),
            value["map_version"].as_u64().unwrap_or(0)
        ),
        "knowledge.map.show" => format!(
            "knowledge_map={} topics={} sources={} routes={}",
            value["path"]
                .as_str()
                .unwrap_or(KNOWLEDGE_MAP_RELATIVE_PATH),
            value["map"]["topics"].as_array().map_or(0, Vec::len),
            value["map"]["sources"].as_array().map_or(0, Vec::len),
            value["map"]["routes"].as_array().map_or(0, Vec::len)
        ),
        "knowledge.map.route" => format!(
            "topic={} sources={}",
            value["topic"].as_str().unwrap_or("unknown"),
            value["sources"].as_array().map_or(0, Vec::len)
        ),
        "knowledge.map.validate" => format!(
            "knowledge_map_valid={} diagnostics={}",
            value["valid"].as_bool().unwrap_or(false),
            value["diagnostics"].as_array().map_or(0, Vec::len)
        ),
        "knowledge.map.agent_snippet" => value["snippet"].as_str().unwrap_or("").to_owned(),
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
        "code.repo.index" => {
            if let Some(task) = value["task"].as_object() {
                format!(
                    "index task={} state={} scope={}",
                    task.get("task_id")
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or("unknown"),
                    task.get("state")
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or("queued"),
                    task.get("source_scope")
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or("unknown")
                )
            } else {
                format!(
                    "indexed files={} symbols={} references={} chunks={} degraded={}",
                    value["summary"]["indexed_file_count"].as_u64().unwrap_or(0),
                    value["summary"]["symbol_count"].as_u64().unwrap_or(0),
                    value["summary"]["reference_count"].as_u64().unwrap_or(0),
                    value["summary"]["chunk_count"].as_u64().unwrap_or(0),
                    value["summary"]["degraded_file_count"]
                        .as_u64()
                        .unwrap_or(0)
                )
            }
        }
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
        "code.repo.feature_flags" => format!(
            "feature_flags={} degraded={}",
            value["flags"].as_array().map_or(0, Vec::len),
            value["degraded_reason"].as_str().unwrap_or("none")
        ),
        "code.repo.impact" => format!(
            "changed_in_scope={} results={}",
            value["path_groups"]["in_scope_changed_paths"]
                .as_array()
                .map_or(0, Vec::len),
            value["results"].as_array().map_or(0, Vec::len)
        ),
        "code.repo.status" => format!(
            "repo={} files={} symbols={} stale={} task={} checkpoint={}",
            value["status"]["alias"].as_str().unwrap_or(""),
            value["status"]["indexed_file_count"].as_u64().unwrap_or(0),
            value["status"]["symbol_count"].as_u64().unwrap_or(0),
            value["status"]["stale"].as_bool().unwrap_or(true),
            value["active_task"]["state"].as_str().unwrap_or("none"),
            value["checkpoint"]["state"].as_str().unwrap_or("none")
        ),
        "code.repo.report" => format!(
            "repo={} files={} freshness={}",
            value["report"]["alias"].as_str().unwrap_or(""),
            value["report"]["indexed_file_count"].as_u64().unwrap_or(0),
            value["report"]["freshness_state"]
                .as_str()
                .unwrap_or("unknown")
        ),
        "code.repo.software" => format!(
            "software scope={} components={} dependency_usages={} sdk_usages={} files={} topics={} relationships={} build_targets={} iac_resources={} design_elements={} stale={}",
            value["status"]["source_scope"]
                .as_str()
                .unwrap_or("unknown"),
            value["components"].as_array().map_or(0, Vec::len),
            value["dependency_usages"].as_array().map_or(0, Vec::len),
            value["sdk_usages"].as_array().map_or(0, Vec::len),
            value["files"].as_array().map_or(0, Vec::len),
            value["topics"].as_array().map_or(0, Vec::len),
            value["relationships"].as_array().map_or(0, Vec::len),
            value["build_targets"].as_array().map_or(0, Vec::len),
            value["iac_resources"].as_array().map_or(0, Vec::len),
            value["design_elements"].as_array().map_or(0, Vec::len),
            value["status"]["stale"].as_bool().unwrap_or(true)
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
