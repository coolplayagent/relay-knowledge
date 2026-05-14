use crate::{
    api::{ApiMetadata, RequestContext},
    application::RelayKnowledgeService,
    domain::GraphVersion,
};

use super::{CliAction, CliError, OutputFormat, render_response};

/// Named setup profiles exposed by the CLI.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SetupProfile {
    Local,
    AgentReadonly,
    Service,
    ExternalEmbedding,
}

impl SetupProfile {
    fn parse(value: &str) -> Result<Self, CliError> {
        match value {
            "local" => Ok(Self::Local),
            "agent-readonly" => Ok(Self::AgentReadonly),
            "service" => Ok(Self::Service),
            "external-embedding" => Ok(Self::ExternalEmbedding),
            other => Err(CliError::UnexpectedArgument(other.to_owned())),
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Local => "local",
            Self::AgentReadonly => "agent-readonly",
            Self::Service => "service",
            Self::ExternalEmbedding => "external-embedding",
        }
    }
}

#[derive(Debug, Clone, serde::Serialize)]
struct SetupDoctorResponse {
    metadata: ApiMetadata,
    configuration_ready: bool,
    live_health_checked: bool,
    live_health_commands: Vec<&'static str>,
    checks: Vec<SetupCheck>,
    recommended_actions: Vec<SetupAction>,
}

#[derive(Debug, Clone, serde::Serialize)]
struct SetupCheck {
    name: &'static str,
    state: &'static str,
    message: String,
}

#[derive(Debug, Clone, serde::Serialize)]
struct SetupAction {
    command: &'static str,
    reason: String,
}

#[derive(Debug, Clone, serde::Serialize)]
struct SetupProfileResponse {
    metadata: ApiMetadata,
    profile: &'static str,
    summary: &'static str,
    environment: Vec<SetupEnvVar>,
    commands: Vec<&'static str>,
    notes: Vec<&'static str>,
}

#[derive(Debug, Clone, serde::Serialize)]
struct SetupEnvVar {
    name: &'static str,
    value: &'static str,
    required: bool,
    meaning: &'static str,
}

pub(super) fn run_setup_action(
    service: &RelayKnowledgeService,
    action: &CliAction,
    context: RequestContext,
    format: OutputFormat,
) -> Result<Option<String>, CliError> {
    let output = match *action {
        CliAction::SetupDoctor => {
            let response = setup_doctor(service, context);
            render_response("setup.doctor", response.metadata.clone(), &response, format)?
        }
        CliAction::SetupProfile { profile } => {
            let response = setup_profile(
                profile,
                ApiMetadata::graph_only(&context, GraphVersion::ZERO),
            );
            render_response(
                "setup.profile",
                response.metadata.clone(),
                &response,
                format,
            )?
        }
        _ => return Ok(None),
    };

    Ok(Some(output))
}

pub(super) fn parse_setup(tokens: &[String]) -> Result<CliAction, CliError> {
    match tokens.first().map(String::as_str) {
        Some("doctor") if tokens.len() == 1 => Ok(CliAction::SetupDoctor),
        Some("profile") => {
            let value = tokens.get(1).ok_or(CliError::MissingValue("profile"))?;
            if tokens.len() > 2 {
                return Err(CliError::UnexpectedArgument(tokens[2].clone()));
            }
            Ok(CliAction::SetupProfile {
                profile: SetupProfile::parse(value)?,
            })
        }
        Some(other) => Err(CliError::UnexpectedArgument(other.to_owned())),
        None => Err(CliError::UnexpectedArgument("setup".to_owned())),
    }
}

fn setup_doctor(service: &RelayKnowledgeService, context: RequestContext) -> SetupDoctorResponse {
    let (status, agent_protocols) = service.runtime_diagnostics(context);

    let mut checks = Vec::new();
    let mut actions = Vec::new();

    let runtime_paths_ready = !status.runtime.config_dir.is_empty()
        && !status.runtime.data_dir.is_empty()
        && !status.runtime.log_dir.is_empty();
    checks.push(SetupCheck {
        name: "runtime_paths",
        state: if runtime_paths_ready {
            "ok"
        } else {
            "action-required"
        },
        message: format!(
            "config={}, data={}, logs={}",
            status.runtime.config_dir, status.runtime.data_dir, status.runtime.log_dir
        ),
    });
    let network_budget_ready = status.runtime.http_max_request_body_bytes > 0
        && status.runtime.qos_max_connections > 0
        && status.runtime.qos_max_in_flight_requests > 0
        && status.runtime.qos_max_queue_depth > 0;
    checks.push(SetupCheck {
        name: "network_budget",
        state: if network_budget_ready {
            "ok"
        } else {
            "action-required"
        },
        message: format!(
            "bind={}, body_bytes={}, connections={}, in_flight={}, queue={}",
            status.runtime.http_bind,
            status.runtime.http_max_request_body_bytes,
            status.runtime.qos_max_connections,
            status.runtime.qos_max_in_flight_requests,
            status.runtime.qos_max_queue_depth
        ),
    });
    let retrieval_backends_ready = status.runtime.embedding_dimension > 0
        && !status.runtime.semantic_backend_mode.is_empty()
        && !status.runtime.vector_backend_mode.is_empty();
    checks.push(SetupCheck {
        name: "retrieval_backends",
        state: if retrieval_backends_ready {
            "ok"
        } else {
            "action-required"
        },
        message: format!(
            "semantic={}, vector={}, text_model={}, dimension={}",
            status.runtime.semantic_backend_mode,
            status.runtime.vector_backend_mode,
            status.runtime.text_embedding_model,
            status.runtime.embedding_dimension
        ),
    });

    let mcp_policy_ready = !agent_protocols.mcp_streamable_http_enabled
        || agent_protocols
            .policy
            .allowed_scope_count
            .saturating_add(usize::from(agent_protocols.policy.allow_unspecified_scope))
            > 0;
    checks.push(SetupCheck {
        name: "mcp_scope_policy",
        state: if mcp_policy_ready {
            "ok"
        } else {
            "action-required"
        },
        message: format!(
            "enabled={}, allowed_scopes={}, allow_unspecified_scope={}",
            agent_protocols.mcp_streamable_http_enabled,
            agent_protocols.policy.allowed_scope_count,
            agent_protocols.policy.allow_unspecified_scope
        ),
    });
    if !mcp_policy_ready {
        actions.push(SetupAction {
            command:
                "RELAY_KNOWLEDGE_MCP_ALLOWED_SCOPES=docs relay-knowledge service run --mcp streamable-http",
            reason: "MCP graph tools require an explicit scope policy before use".to_owned(),
        });
    }

    let service_directory_ready = !status.runtime.service_dir.is_empty();
    checks.push(SetupCheck {
        name: "service_directory",
        state: if service_directory_ready {
            "ok"
        } else {
            "action-required"
        },
        message: format!("service_dir={}", status.runtime.service_dir),
    });
    let worker_budget_ready = status.runtime.worker_max_in_flight > 0;
    checks.push(SetupCheck {
        name: "worker_budget",
        state: if worker_budget_ready {
            "ok"
        } else {
            "action-required"
        },
        message: format!(
            "max_in_flight={}, silent_updates={}",
            status.runtime.worker_max_in_flight, status.runtime.silent_updates_enabled
        ),
    });

    let configuration_ready = checks.iter().all(|check| check.state == "ok");

    actions.push(SetupAction {
        command: "relay-knowledge health --format json",
        reason: "setup doctor is storage-free; run health for live storage and index readiness"
            .to_owned(),
    });
    actions.push(SetupAction {
        command: "relay-knowledge service doctor --format json",
        reason: "setup doctor does not inspect installed service state or worker queue health"
            .to_owned(),
    });

    SetupDoctorResponse {
        metadata: status.metadata,
        configuration_ready,
        live_health_checked: false,
        live_health_commands: vec![
            "relay-knowledge health --format json",
            "relay-knowledge service doctor --format json",
        ],
        checks,
        recommended_actions: actions,
    }
}

fn setup_profile(profile: SetupProfile, metadata: ApiMetadata) -> SetupProfileResponse {
    match profile {
        SetupProfile::Local => SetupProfileResponse {
            metadata,
            profile: profile.as_str(),
            summary: "Zero-configuration local CLI and Web diagnostics.",
            environment: vec![SetupEnvVar {
                name: "RELAY_KNOWLEDGE_HOME",
                value: "/absolute/path/to/relay-knowledge-home",
                required: false,
                meaning: "Optional isolated runtime root for demos or repros.",
            }],
            commands: vec![
                "relay-knowledge setup doctor --format json",
                "relay-knowledge status --format json",
                "relay-knowledge ingest --source docs --content \"Rust async services isolate blocking SQLite work\" --entity Rust --format json",
                "relay-knowledge query SQLite --source docs --freshness wait-until-fresh --format json",
            ],
            notes: vec!["Without RELAY_KNOWLEDGE_HOME, platform runtime directories are used."],
        },
        SetupProfile::AgentReadonly => SetupProfileResponse {
            metadata,
            profile: profile.as_str(),
            summary: "Local MCP Streamable HTTP access for read-only graph retrieval.",
            environment: vec![
                SetupEnvVar {
                    name: "RELAY_KNOWLEDGE_MCP_ALLOWED_SCOPES",
                    value: "docs",
                    required: true,
                    meaning: "Comma-separated source scopes that agent tools may read.",
                },
                SetupEnvVar {
                    name: "RELAY_KNOWLEDGE_MCP_ALLOW_INDEX_REFRESH",
                    value: "false",
                    required: false,
                    meaning: "Keep agent-triggered index refresh hidden unless explicitly needed.",
                },
                SetupEnvVar {
                    name: "RELAY_KNOWLEDGE_AGENT_AUDIT_SINK_ENABLED",
                    value: "true",
                    required: false,
                    meaning: "Mirror bounded agent audit events to the path-owned JSONL sink.",
                },
            ],
            commands: vec![
                "relay-knowledge setup doctor --format json",
                "relay-knowledge service run --mcp streamable-http",
            ],
            notes: vec!["Do not combine remote binds with unspecified scope for this profile."],
        },
        SetupProfile::Service => SetupProfileResponse {
            metadata,
            profile: profile.as_str(),
            summary: "Platform service-manager installation preview and operator control.",
            environment: vec![SetupEnvVar {
                name: "RELAY_KNOWLEDGE_SILENT_UPDATES_ENABLED",
                value: "true",
                required: false,
                meaning: "Enable configured background refresh and worker maintenance scopes.",
            }],
            commands: vec![
                "relay-knowledge service plan install --format json",
                "relay-knowledge service definition write --format json",
                "relay-knowledge service operator status --format json",
                "relay-knowledge service doctor --format json",
            ],
            notes: vec![
                "The CLI writes service definitions but does not execute privileged install commands.",
            ],
        },
        SetupProfile::ExternalEmbedding => SetupProfileResponse {
            metadata,
            profile: profile.as_str(),
            summary: "External OpenAI-compatible semantic/vector embedding worker metadata.",
            environment: vec![
                SetupEnvVar {
                    name: "RELAY_KNOWLEDGE_SEMANTIC_BACKEND",
                    value: "external",
                    required: true,
                    meaning: "Enable external semantic read-model metadata.",
                },
                SetupEnvVar {
                    name: "RELAY_KNOWLEDGE_VECTOR_BACKEND",
                    value: "external",
                    required: true,
                    meaning: "Enable external vector read-model metadata.",
                },
                SetupEnvVar {
                    name: "RELAY_KNOWLEDGE_LLM_PROVIDER",
                    value: "openai_compatible",
                    required: true,
                    meaning: "Select the provider contract.",
                },
                SetupEnvVar {
                    name: "RELAY_KNOWLEDGE_EMBEDDING_BASE_URL",
                    value: "https://api.example.com/v1",
                    required: true,
                    meaning: "Embedding provider base URL.",
                },
                SetupEnvVar {
                    name: "RELAY_KNOWLEDGE_EMBEDDING_API_KEY",
                    value: "<secret>",
                    required: true,
                    meaning: "Provider API key; diagnostics only report whether it is configured.",
                },
                SetupEnvVar {
                    name: "RELAY_KNOWLEDGE_TEXT_EMBEDDING_MODEL",
                    value: "text-embed-3-small",
                    required: true,
                    meaning: "Text embedding model identity stored with cursor metadata.",
                },
                SetupEnvVar {
                    name: "RELAY_KNOWLEDGE_IMAGE_EMBEDDING_MODEL",
                    value: "clip-vit-b32",
                    required: false,
                    meaning: "Image embedding model identity for multimodal workers.",
                },
                SetupEnvVar {
                    name: "RELAY_KNOWLEDGE_EMBEDDING_DIMENSION",
                    value: "1536",
                    required: true,
                    meaning: "Vector dimension recorded in backend and cursor metadata.",
                },
            ],
            commands: vec![
                "relay-knowledge provider probe --format json",
                "relay-knowledge index refresh --kind semantic --kind vector --format json",
                "relay-knowledge health --format json",
            ],
            notes: vec![
                "Query hot paths read derived models; they do not synchronously call the external provider.",
            ],
        },
    }
}

#[cfg(test)]
#[path = "setup_cli_tests.rs"]
mod setup_cli_tests;
