use crate::{
    api::ApiError,
    application::{
        KnowledgeMapService, KnowledgeMapServiceError, KnowledgeMapSourceAddRequest,
        knowledge_map_service,
    },
    domain::{KnowledgeMapChange, KnowledgeMapSourceKind},
};

use super::{CliAction, CliError, OutputFormat, value_after};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MapCommand {
    Init,
    Show {
        topic: Option<String>,
    },
    Route {
        topic: String,
    },
    SourceAdd {
        request: KnowledgeMapSourceAddRequest,
    },
    SourceUpdate {
        change: KnowledgeMapChange,
    },
    SourceRemove {
        id: String,
    },
    Validate,
    AgentSnippet,
}

pub(super) fn parse_map(tokens: &[String]) -> Result<CliAction, CliError> {
    match tokens.first().map(String::as_str) {
        Some("init") if tokens.len() == 1 => Ok(CliAction::Map(MapCommand::Init)),
        Some("show") => parse_show(&tokens[1..]),
        Some("route") => parse_route(&tokens[1..]),
        Some("source") => parse_source(&tokens[1..]),
        Some("validate") if tokens.len() == 1 => Ok(CliAction::Map(MapCommand::Validate)),
        Some("agent-snippet") if tokens.len() == 1 => Ok(CliAction::Map(MapCommand::AgentSnippet)),
        other => Err(CliError::UnexpectedArgument(
            other.unwrap_or("map").to_owned(),
        )),
    }
}

pub(super) async fn run_map(
    command: MapCommand,
    context: crate::api::RequestContext,
    format: OutputFormat,
) -> Result<String, CliError> {
    match command {
        MapCommand::Init => {
            let service = map_service(format)?;
            let response = service
                .init(&context)
                .await
                .map_err(|error| map_error("knowledge map init failed", error, format))?;
            super::render_response(
                "knowledge.map.init",
                response.metadata.clone(),
                &response,
                format,
            )
        }
        MapCommand::Show { topic } => {
            let service = map_service(format)?;
            let response = service
                .show(&context, topic)
                .await
                .map_err(|error| map_error("knowledge map show failed", error, format))?;
            super::render_response(
                "knowledge.map.show",
                response.metadata.clone(),
                &response,
                format,
            )
        }
        MapCommand::Route { topic } => {
            let service = map_service(format)?;
            let response = service
                .route(&context, topic)
                .await
                .map_err(|error| map_error("knowledge map route failed", error, format))?;
            super::render_response(
                "knowledge.map.route",
                response.metadata.clone(),
                &response,
                format,
            )
        }
        MapCommand::SourceAdd { request } => {
            let service = map_service(format)?;
            let response = service
                .add_source(&context, request)
                .await
                .map_err(|error| map_error("knowledge map source add failed", error, format))?;
            super::render_response(
                "knowledge.map.source.add",
                response.metadata.clone(),
                &response,
                format,
            )
        }
        MapCommand::SourceUpdate { change } => {
            let service = map_service(format)?;
            let response = service
                .update_source(&context, change)
                .await
                .map_err(|error| map_error("knowledge map source update failed", error, format))?;
            super::render_response(
                "knowledge.map.source.update",
                response.metadata.clone(),
                &response,
                format,
            )
        }
        MapCommand::SourceRemove { id } => {
            let service = map_service(format)?;
            let response = service
                .remove_source(&context, id)
                .await
                .map_err(|error| map_error("knowledge map source remove failed", error, format))?;
            super::render_response(
                "knowledge.map.source.remove",
                response.metadata.clone(),
                &response,
                format,
            )
        }
        MapCommand::Validate => {
            let service = map_service(format)?;
            let response = service
                .validate(&context)
                .await
                .map_err(|error| map_error("knowledge map validate failed", error, format))?;
            super::render_response(
                "knowledge.map.validate",
                response.metadata.clone(),
                &response,
                format,
            )
        }
        MapCommand::AgentSnippet => {
            let response =
                KnowledgeMapService::new(std::path::PathBuf::new()).agent_snippet(&context);
            super::render_response(
                "knowledge.map.agent_snippet",
                response.metadata.clone(),
                &response,
                format,
            )
        }
    }
}

fn map_service(format: OutputFormat) -> Result<KnowledgeMapService, CliError> {
    knowledge_map_service()
        .map_err(ApiError::invalid_argument)
        .map_err(|error| CliError::api_failed(error, format))
}

fn map_error(
    prefix: &'static str,
    error: KnowledgeMapServiceError,
    format: OutputFormat,
) -> CliError {
    CliError::api_failed(
        ApiError::invalid_argument(format!("{prefix}: {error}")),
        format,
    )
}

fn parse_show(tokens: &[String]) -> Result<CliAction, CliError> {
    let mut topic = None;
    let mut index = 0;
    while index < tokens.len() {
        match tokens[index].as_str() {
            "--topic" => {
                topic = Some(value_after(tokens, index, "--topic")?);
                index += 2;
            }
            other => return Err(CliError::UnexpectedArgument(other.to_owned())),
        }
    }
    Ok(CliAction::Map(MapCommand::Show { topic }))
}

fn parse_route(tokens: &[String]) -> Result<CliAction, CliError> {
    if tokens.len() == 1 && !tokens[0].starts_with('-') {
        return Ok(CliAction::Map(MapCommand::Route {
            topic: tokens[0].clone(),
        }));
    }
    Err(CliError::MissingValue("topic"))
}

fn parse_source(tokens: &[String]) -> Result<CliAction, CliError> {
    match tokens.first().map(String::as_str) {
        Some("add") => parse_source_add(&tokens[1..]),
        Some("update") => parse_source_update(&tokens[1..]),
        Some("remove") => parse_source_remove(&tokens[1..]),
        other => Err(CliError::UnexpectedArgument(
            other.unwrap_or("source").to_owned(),
        )),
    }
}

fn parse_source_add(tokens: &[String]) -> Result<CliAction, CliError> {
    let mut id = None;
    let mut topic = None;
    let mut kind = None;
    let mut uri = None;
    let mut source_scope = None;
    let mut description = None;
    let mut index = 0;

    while index < tokens.len() {
        match tokens[index].as_str() {
            "--id" => {
                id = Some(value_after(tokens, index, "--id")?);
                index += 2;
            }
            "--topic" => {
                topic = Some(value_after(tokens, index, "--topic")?);
                index += 2;
            }
            "--kind" => {
                kind = Some(source_kind(&value_after(tokens, index, "--kind")?)?);
                index += 2;
            }
            "--uri" => {
                uri = Some(value_after(tokens, index, "--uri")?);
                index += 2;
            }
            "--scope" => {
                source_scope = Some(value_after(tokens, index, "--scope")?);
                index += 2;
            }
            "--description" => {
                description = Some(value_after(tokens, index, "--description")?);
                index += 2;
            }
            other => return Err(CliError::UnexpectedArgument(other.to_owned())),
        }
    }

    Ok(CliAction::Map(MapCommand::SourceAdd {
        request: KnowledgeMapSourceAddRequest {
            id: id.ok_or(CliError::MissingValue("--id"))?,
            topic: topic.ok_or(CliError::MissingValue("--topic"))?,
            kind: kind.ok_or(CliError::MissingValue("--kind"))?,
            uri: uri.ok_or(CliError::MissingValue("--uri"))?,
            source_scope,
            description,
        },
    }))
}

fn parse_source_update(tokens: &[String]) -> Result<CliAction, CliError> {
    let mut id = None;
    let mut topic = None;
    let mut kind = None;
    let mut uri = None;
    let mut source_scope = None;
    let mut description = None;
    let mut index = 0;

    while index < tokens.len() {
        match tokens[index].as_str() {
            "--id" => {
                id = Some(value_after(tokens, index, "--id")?);
                index += 2;
            }
            "--topic" => {
                topic = Some(value_after(tokens, index, "--topic")?);
                index += 2;
            }
            "--kind" => {
                kind = Some(source_kind(&value_after(tokens, index, "--kind")?)?);
                index += 2;
            }
            "--uri" => {
                uri = Some(value_after(tokens, index, "--uri")?);
                index += 2;
            }
            "--scope" => {
                source_scope = Some(value_after(tokens, index, "--scope")?);
                index += 2;
            }
            "--description" => {
                description = Some(value_after(tokens, index, "--description")?);
                index += 2;
            }
            other => return Err(CliError::UnexpectedArgument(other.to_owned())),
        }
    }

    Ok(CliAction::Map(MapCommand::SourceUpdate {
        change: KnowledgeMapChange {
            id: id.ok_or(CliError::MissingValue("--id"))?,
            topic,
            kind,
            uri,
            source_scope,
            description,
        },
    }))
}

fn parse_source_remove(tokens: &[String]) -> Result<CliAction, CliError> {
    let mut id = None;
    let mut index = 0;
    while index < tokens.len() {
        match tokens[index].as_str() {
            "--id" => {
                id = Some(value_after(tokens, index, "--id")?);
                index += 2;
            }
            other => return Err(CliError::UnexpectedArgument(other.to_owned())),
        }
    }
    Ok(CliAction::Map(MapCommand::SourceRemove {
        id: id.ok_or(CliError::MissingValue("--id"))?,
    }))
}

pub(super) fn source_kind(value: &str) -> Result<KnowledgeMapSourceKind, CliError> {
    match value {
        "repo" => Ok(KnowledgeMapSourceKind::Repo),
        "file" => Ok(KnowledgeMapSourceKind::File),
        "doc" => Ok(KnowledgeMapSourceKind::Doc),
        "config" => Ok(KnowledgeMapSourceKind::Config),
        "db" => Ok(KnowledgeMapSourceKind::Db),
        "ci" => Ok(KnowledgeMapSourceKind::Ci),
        "runtime" => Ok(KnowledgeMapSourceKind::Runtime),
        "wiki" => Ok(KnowledgeMapSourceKind::Wiki),
        "monitoring" => Ok(KnowledgeMapSourceKind::Monitoring),
        other => Err(CliError::InvalidMapSourceKind(other.to_owned())),
    }
}
