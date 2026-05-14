use std::{collections::HashMap, time::Instant};

use serde::Deserialize;
use serde_json::{Value, json};

use super::{
    McpMethodError, McpServer,
    audit_bridge::{McpMethodAudit, record_mcp_method_audit},
    elapsed_millis,
};
use crate::interfaces::agent::AgentAuditStatus;

#[derive(Debug, Deserialize)]
struct PromptGetParams {
    name: String,
    #[serde(default)]
    arguments: HashMap<String, Value>,
}

pub(super) fn list_prompts() -> Value {
    json!({
        "prompts": [
            {
                "name": "relay_retrieve_context_prompt",
                "title": "Retrieve Graph Context",
                "description": "Prepare a grounded relay-knowledge retrieval request.",
                "arguments": [
                    {"name": "query", "description": "Question or search text.", "required": true},
                    {"name": "source_scope", "description": "Authorized relay source scope.", "required": false},
                    {"name": "freshness", "description": "allow-stale, wait-until-fresh, or graph-only.", "required": false},
                    {"name": "limit", "description": "Maximum result count within policy.", "required": false}
                ]
            },
            {
                "name": "relay_code_impact_prompt",
                "title": "Analyze Code Impact",
                "description": "Prepare a code impact request for an indexed repository.",
                "arguments": [
                    {"name": "repository", "description": "Registered repository alias.", "required": true},
                    {"name": "base_ref", "description": "Base git ref.", "required": true},
                    {"name": "head_ref", "description": "Head git ref.", "required": true}
                ]
            }
        ]
    })
}

pub(super) async fn get_prompt(
    server: &McpServer,
    params: Value,
    request_id: &str,
) -> Result<Value, McpMethodError> {
    let started = Instant::now();
    let params = serde_json::from_value::<PromptGetParams>(params).map_err(|error| {
        McpMethodError::invalid_params(format!("invalid prompts/get params: {error}"))
    })?;
    let result = match params.name.as_str() {
        "relay_retrieve_context_prompt" => retrieve_context_prompt(&params.arguments),
        "relay_code_impact_prompt" => code_impact_prompt(&params.arguments),
        _ => Err(McpMethodError::invalid_params("unknown prompt name")),
    };
    record_mcp_method_audit(
        server,
        McpMethodAudit {
            operation: "prompts/get",
            request_id,
            status: if result.is_ok() {
                AgentAuditStatus::Completed
            } else {
                AgentAuditStatus::Failed
            },
            source_scope: None,
            result_count: None,
            elapsed_ms: elapsed_millis(started),
            error_kind: result.as_ref().err().map(|error| error.kind),
        },
    )
    .await;

    result
}

fn retrieve_context_prompt(arguments: &HashMap<String, Value>) -> Result<Value, McpMethodError> {
    let query = required_argument(arguments, "query")?;
    let source_scope = optional_argument(arguments, "source_scope").unwrap_or("<authorized-scope>");
    let freshness = optional_argument(arguments, "freshness").unwrap_or("wait-until-fresh");
    let limit = optional_argument(arguments, "limit").unwrap_or("5");
    Ok(prompt_result(
        "Retrieve Graph Context",
        format!(
            "Use relay_retrieve_context with query `{query}`, source_scope `{source_scope}`, freshness `{freshness}`, and limit `{limit}`. Cite returned evidence ids, source spans, graph facts, graph_paths, backend status, and truncation metadata in the answer."
        ),
    ))
}

fn code_impact_prompt(arguments: &HashMap<String, Value>) -> Result<Value, McpMethodError> {
    let repository = required_argument(arguments, "repository")?;
    let base_ref = required_argument(arguments, "base_ref")?;
    let head_ref = required_argument(arguments, "head_ref")?;
    Ok(prompt_result(
        "Analyze Code Impact",
        format!(
            "Use relay_code_impact for repository `{repository}` from base_ref `{base_ref}` to head_ref `{head_ref}`. Summarize changed paths, impacted symbols, stale scope metadata, and any degraded retrieval reason."
        ),
    ))
}

fn prompt_result(description: &str, text: String) -> Value {
    json!({
        "description": description,
        "messages": [
            {
                "role": "user",
                "content": {"type": "text", "text": text}
            }
        ]
    })
}

fn required_argument<'a>(
    arguments: &'a HashMap<String, Value>,
    name: &'static str,
) -> Result<&'a str, McpMethodError> {
    optional_argument(arguments, name).ok_or_else(|| {
        McpMethodError::invalid_params(format!("prompt argument '{name}' is required"))
    })
}

fn optional_argument<'a>(arguments: &'a HashMap<String, Value>, name: &str) -> Option<&'a str> {
    arguments
        .get(name)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
}
