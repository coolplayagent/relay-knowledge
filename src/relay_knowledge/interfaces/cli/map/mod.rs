//! Knowledge-map CLI command contracts, parsing, and execution.

use crate::{
    api::{ApiError, ApiMetadata},
    application::{KnowledgeMapService, KnowledgeMapServiceError, KnowledgeMapSourceAddRequest},
    domain::{
        DirectoryLoadHint, DirectoryRelation, DirectoryRelationKind, DirectoryUpdateRule,
        GraphVersion, KnowledgeMapChange, KnowledgeMapSourceKind, RepositoryMapDirectory,
        RepositoryMapDirectoryChange, RepositoryMapType,
    },
};
use serde::Serialize;

use super::{CliAction, CliError, OutputFormat, command::value_after};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MapCommand {
    Init {
        selection: MapSelection,
    },
    Show {
        selection: MapSelection,
        topic: Option<String>,
        directory: Option<String>,
    },
    History {
        selection: MapSelection,
        from_version: u64,
        limit: usize,
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
    DirectoryAdd {
        map_type: RepositoryMapType,
        directory: RepositoryMapDirectory,
    },
    DirectoryUpdate {
        map_type: RepositoryMapType,
        change: RepositoryMapDirectoryChange,
    },
    DirectoryRemove {
        map_type: RepositoryMapType,
        directory: String,
    },
    MigrateToV3,
    MigrateRollback,
    Validate {
        selection: MapSelection,
    },
    AgentSnippet,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MapSelection {
    All,
    One(RepositoryMapType),
}

impl MapCommand {
    pub(crate) fn needs_repository_root(&self) -> bool {
        !matches!(self, Self::AgentSnippet)
    }
}

pub(super) fn parse_map(tokens: &[String]) -> Result<CliAction, CliError> {
    match tokens.first().map(String::as_str) {
        Some("init") => {
            let (selection, remaining) = extract_selection(&tokens[1..], false)?;
            if !remaining.is_empty() {
                return Err(CliError::UnexpectedArgument(remaining[0].clone()));
            }
            Ok(CliAction::Map(MapCommand::Init { selection }))
        }
        Some("show") => parse_show(&tokens[1..]),
        Some("history") => parse_history(&tokens[1..]),
        Some("route") => parse_route(&tokens[1..]),
        Some("source") => parse_source(&tokens[1..]),
        Some("directory") => parse_directory(&tokens[1..]),
        Some("migrate") => parse_migrate(&tokens[1..]),
        Some("validate") => {
            let (selection, remaining) = extract_selection(&tokens[1..], false)?;
            if !remaining.is_empty() {
                return Err(CliError::UnexpectedArgument(remaining[0].clone()));
            }
            Ok(CliAction::Map(MapCommand::Validate { selection }))
        }
        Some("agent-snippet") if tokens.len() == 1 => Ok(CliAction::Map(MapCommand::AgentSnippet)),
        other => Err(CliError::UnexpectedArgument(
            other.unwrap_or("map").to_owned(),
        )),
    }
}

pub(crate) async fn run_map(
    command: MapCommand,
    service: Option<&KnowledgeMapService>,
    context: crate::api::RequestContext,
    format: OutputFormat,
) -> Result<String, CliError> {
    match command {
        MapCommand::Init { selection } => {
            let service = map_service(service, format)?;
            let mut results = Vec::new();
            for selected in selected_services(service, selection) {
                results.push(
                    selected
                        .init(&context)
                        .await
                        .map_err(|error| map_error("repository map init failed", error, format))?,
                );
            }
            render_selected(
                "knowledge.map.init",
                "repository.map.init",
                selection,
                results,
                &context,
                format,
            )
        }
        MapCommand::Show {
            selection,
            topic,
            directory,
        } => {
            let service = map_service(service, format)?;
            let mut results = Vec::new();
            for selected in selected_services(service, selection) {
                results.push(
                    selected
                        .show_filtered(&context, topic.clone(), directory.clone())
                        .await
                        .map_err(|error| map_error("repository map show failed", error, format))?,
                );
            }
            render_selected(
                "knowledge.map.show",
                "repository.map.show",
                selection,
                results,
                &context,
                format,
            )
        }
        MapCommand::History {
            selection,
            from_version,
            limit,
        } => {
            let service = map_service(service, format)?;
            let mut results = Vec::new();
            for selected in selected_services(service, selection) {
                results.push(
                    selected
                        .history(&context, from_version, limit)
                        .await
                        .map_err(|error| {
                            map_error("repository map history failed", error, format)
                        })?,
                );
            }
            render_selected(
                "knowledge.map.history",
                "repository.map.history",
                selection,
                results,
                &context,
                format,
            )
        }
        MapCommand::Route { topic } => {
            let service = map_service(service, format)?;
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
            let service = map_service(service, format)?;
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
            let service = map_service(service, format)?;
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
            let service = map_service(service, format)?;
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
        MapCommand::DirectoryAdd {
            map_type,
            directory,
        } => {
            let selected = map_service(service, format)?.for_type(map_type);
            let response = selected
                .add_directory(&context, directory)
                .await
                .map_err(|error| map_error("repository map directory add failed", error, format))?;
            super::render_response(
                "repository.map.directory.add",
                response.metadata.clone(),
                &response,
                format,
            )
        }
        MapCommand::DirectoryUpdate { map_type, change } => {
            let selected = map_service(service, format)?.for_type(map_type);
            let response = selected
                .update_directory(&context, change)
                .await
                .map_err(|error| {
                    map_error("repository map directory update failed", error, format)
                })?;
            super::render_response(
                "repository.map.directory.update",
                response.metadata.clone(),
                &response,
                format,
            )
        }
        MapCommand::DirectoryRemove {
            map_type,
            directory,
        } => {
            let selected = map_service(service, format)?.for_type(map_type);
            let response = selected
                .remove_directory(&context, directory)
                .await
                .map_err(|error| {
                    map_error("repository map directory remove failed", error, format)
                })?;
            super::render_response(
                "repository.map.directory.remove",
                response.metadata.clone(),
                &response,
                format,
            )
        }
        MapCommand::MigrateToV3 => {
            let selected = map_service(service, format)?.for_type(RepositoryMapType::Knowledge);
            let response = selected
                .migrate_to_v3(&context)
                .await
                .map_err(|error| map_error("knowledge map v3 migration failed", error, format))?;
            super::render_response(
                "repository.map.migrate",
                response.metadata.clone(),
                &response,
                format,
            )
        }
        MapCommand::MigrateRollback => {
            let selected = map_service(service, format)?.for_type(RepositoryMapType::Knowledge);
            let response = selected
                .rollback_v3(&context)
                .await
                .map_err(|error| map_error("knowledge map v3 rollback failed", error, format))?;
            super::render_response(
                "repository.map.migrate",
                response.metadata.clone(),
                &response,
                format,
            )
        }
        MapCommand::Validate { selection } => {
            let service = map_service(service, format)?;
            let mut results = Vec::new();
            for selected in selected_services(service, selection) {
                results.push(
                    selected.validate(&context).await.map_err(|error| {
                        map_error("repository map validate failed", error, format)
                    })?,
                );
            }
            render_selected(
                "knowledge.map.validate",
                "repository.map.validate",
                selection,
                results,
                &context,
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

#[derive(Serialize)]
struct RepositoryMapBatchResponse<T> {
    metadata: ApiMetadata,
    results: Vec<T>,
}

fn render_selected<T: Serialize>(
    single_operation: &'static str,
    batch_operation: &'static str,
    selection: MapSelection,
    mut results: Vec<T>,
    context: &crate::api::RequestContext,
    format: OutputFormat,
) -> Result<String, CliError> {
    if selection != MapSelection::All {
        let response = results.pop().expect("one selected map response");
        return super::render_response(
            single_operation,
            ApiMetadata::graph_only(context, GraphVersion::ZERO),
            &response,
            format,
        );
    }
    let response = RepositoryMapBatchResponse {
        metadata: ApiMetadata::graph_only(context, GraphVersion::ZERO),
        results,
    };
    super::render_response(
        batch_operation,
        response.metadata.clone(),
        &response,
        format,
    )
}

fn selected_services(
    service: &KnowledgeMapService,
    selection: MapSelection,
) -> Vec<KnowledgeMapService> {
    match selection {
        MapSelection::All => vec![
            service.for_type(RepositoryMapType::Codespec),
            service.for_type(RepositoryMapType::Knowledge),
        ],
        MapSelection::One(map_type) => vec![service.for_type(map_type)],
    }
}

fn map_service(
    service: Option<&KnowledgeMapService>,
    format: OutputFormat,
) -> Result<&KnowledgeMapService, CliError> {
    service.ok_or_else(|| {
        CliError::api_failed(
            ApiError::invalid_argument("knowledge map repository root was not resolved"),
            format,
        )
    })
}

fn map_error(
    prefix: &'static str,
    error: KnowledgeMapServiceError,
    format: OutputFormat,
) -> CliError {
    let message = format!("{prefix}: {error}");
    let api_error = if matches!(error, KnowledgeMapServiceError::LockTimeout(_)) {
        ApiError::timeout(message)
    } else {
        ApiError::invalid_argument(message)
    };
    CliError::api_failed(api_error, format)
}

fn parse_show(tokens: &[String]) -> Result<CliAction, CliError> {
    let (selection, tokens) = extract_selection(tokens, false)?;
    let mut topic = None;
    let mut directory = None;
    let mut index = 0;
    while index < tokens.len() {
        match tokens[index].as_str() {
            "--topic" => {
                topic = Some(value_after(&tokens, index, "--topic")?);
                index += 2;
            }
            "--directory" => {
                directory = Some(value_after(&tokens, index, "--directory")?);
                index += 2;
            }
            other => return Err(CliError::UnexpectedArgument(other.to_owned())),
        }
    }
    Ok(CliAction::Map(MapCommand::Show {
        selection,
        topic,
        directory,
    }))
}

fn parse_route(tokens: &[String]) -> Result<CliAction, CliError> {
    let (selection, tokens) = extract_selection(tokens, true)?;
    require_knowledge_selection(selection)?;
    if tokens.len() == 1 && !tokens[0].starts_with('-') {
        return Ok(CliAction::Map(MapCommand::Route {
            topic: tokens[0].clone(),
        }));
    }
    Err(CliError::MissingValue("topic"))
}

fn parse_history(tokens: &[String]) -> Result<CliAction, CliError> {
    const DEFAULT_HISTORY_PAGE_SIZE: usize = 64;

    let (selection, tokens) = extract_selection(tokens, false)?;
    let mut from_version = 1;
    let mut limit = DEFAULT_HISTORY_PAGE_SIZE;
    let mut index = 0;
    while index < tokens.len() {
        match tokens[index].as_str() {
            "--from" => {
                let value = value_after(&tokens, index, "--from")?;
                from_version = value
                    .parse::<u64>()
                    .map_err(|_| CliError::UnexpectedArgument(value))?;
                index += 2;
            }
            "--limit" => {
                let value = value_after(&tokens, index, "--limit")?;
                limit = value
                    .parse::<usize>()
                    .map_err(|_| CliError::InvalidLimit(value))?;
                index += 2;
            }
            other => return Err(CliError::UnexpectedArgument(other.to_owned())),
        }
    }
    Ok(CliAction::Map(MapCommand::History {
        selection,
        from_version,
        limit,
    }))
}

fn parse_source(tokens: &[String]) -> Result<CliAction, CliError> {
    let (selection, tokens) = extract_selection(tokens, true)?;
    require_knowledge_selection(selection)?;
    match tokens.first().map(String::as_str) {
        Some("add") => parse_source_add(&tokens[1..]),
        Some("update") => parse_source_update(&tokens[1..]),
        Some("remove") => parse_source_remove(&tokens[1..]),
        other => Err(CliError::UnexpectedArgument(
            other.unwrap_or("source").to_owned(),
        )),
    }
}

fn parse_directory(tokens: &[String]) -> Result<CliAction, CliError> {
    let (selection, tokens) = extract_selection(tokens, true)?;
    let MapSelection::One(map_type) = selection else {
        return Err(CliError::UnexpectedArgument("all".to_owned()));
    };
    match tokens.first().map(String::as_str) {
        Some("add") => parse_directory_add(map_type, &tokens[1..]),
        Some("update") => parse_directory_update(map_type, &tokens[1..]),
        Some("remove") => parse_directory_remove(map_type, &tokens[1..]),
        other => Err(CliError::UnexpectedArgument(
            other.unwrap_or("directory").to_owned(),
        )),
    }
}

fn parse_directory_add(
    map_type: RepositoryMapType,
    tokens: &[String],
) -> Result<CliAction, CliError> {
    let fields = parse_directory_fields(tokens)?;
    Ok(CliAction::Map(MapCommand::DirectoryAdd {
        map_type,
        directory: RepositoryMapDirectory {
            directory: fields
                .directory
                .ok_or(CliError::MissingValue("--directory"))?,
            purpose: fields.purpose.ok_or(CliError::MissingValue("--purpose"))?,
            content_scope: fields.content_scope,
            key_files: fields.key_files,
            load_hint: fields
                .load_hint
                .ok_or(CliError::MissingValue("--load-hint"))?,
            relations: fields.relations,
            update_rule: fields
                .update_rule
                .ok_or(CliError::MissingValue("--update-rule"))?,
        },
    }))
}

fn parse_directory_update(
    map_type: RepositoryMapType,
    tokens: &[String],
) -> Result<CliAction, CliError> {
    let fields = parse_directory_fields(tokens)?;
    let content_scope = (!fields.content_scope.is_empty()).then_some(fields.content_scope);
    let key_files = (!fields.key_files.is_empty()).then_some(fields.key_files);
    let relations = (!fields.relations.is_empty()).then_some(fields.relations);
    if fields.purpose.is_none()
        && content_scope.is_none()
        && key_files.is_none()
        && fields.load_hint.is_none()
        && relations.is_none()
        && fields.update_rule.is_none()
    {
        return Err(CliError::MissingValue("directory update field"));
    }
    Ok(CliAction::Map(MapCommand::DirectoryUpdate {
        map_type,
        change: RepositoryMapDirectoryChange {
            directory: fields
                .directory
                .ok_or(CliError::MissingValue("--directory"))?,
            purpose: fields.purpose,
            content_scope,
            key_files,
            load_hint: fields.load_hint,
            relations,
            update_rule: fields.update_rule,
        },
    }))
}

fn parse_directory_remove(
    map_type: RepositoryMapType,
    tokens: &[String],
) -> Result<CliAction, CliError> {
    if tokens.len() == 2 && tokens[0] == "--directory" {
        return Ok(CliAction::Map(MapCommand::DirectoryRemove {
            map_type,
            directory: tokens[1].clone(),
        }));
    }
    Err(CliError::MissingValue("--directory"))
}

#[derive(Default)]
struct DirectoryFields {
    directory: Option<String>,
    purpose: Option<String>,
    content_scope: Vec<String>,
    key_files: Vec<String>,
    load_hint: Option<DirectoryLoadHint>,
    relations: Vec<DirectoryRelation>,
    update_rule: Option<DirectoryUpdateRule>,
}

fn parse_directory_fields(tokens: &[String]) -> Result<DirectoryFields, CliError> {
    let mut fields = DirectoryFields::default();
    let mut index = 0;
    while index < tokens.len() {
        let option = tokens[index].as_str();
        let value = match option {
            "--directory" => value_after(tokens, index, "--directory")?,
            "--purpose" => value_after(tokens, index, "--purpose")?,
            "--content-scope" => value_after(tokens, index, "--content-scope")?,
            "--key-file" => value_after(tokens, index, "--key-file")?,
            "--load-hint" => value_after(tokens, index, "--load-hint")?,
            "--relation" => value_after(tokens, index, "--relation")?,
            "--update-rule" => value_after(tokens, index, "--update-rule")?,
            other => return Err(CliError::UnexpectedArgument(other.to_owned())),
        };
        match option {
            "--directory" => fields.directory = Some(value),
            "--purpose" => fields.purpose = Some(value),
            "--content-scope" => fields.content_scope.push(value),
            "--key-file" => fields.key_files.push(value),
            "--load-hint" => fields.load_hint = Some(parse_load_hint(&value)?),
            "--relation" => fields.relations.push(parse_relation(&value)?),
            "--update-rule" => fields.update_rule = Some(parse_update_rule(&value)?),
            _ => unreachable!("directory option was checked above"),
        }
        index += 2;
    }
    Ok(fields)
}

fn parse_migrate(tokens: &[String]) -> Result<CliAction, CliError> {
    let (selection, tokens) = extract_selection(tokens, true)?;
    require_knowledge_selection(selection)?;
    match tokens.as_slice() {
        [operation] if operation == "--to-v3" => Ok(CliAction::Map(MapCommand::MigrateToV3)),
        [operation] if operation == "--rollback" => Ok(CliAction::Map(MapCommand::MigrateRollback)),
        _ => Err(CliError::MissingValue("--to-v3 or --rollback")),
    }
}

fn extract_selection(
    tokens: &[String],
    require_concrete: bool,
) -> Result<(MapSelection, Vec<String>), CliError> {
    let mut selection = None;
    let mut remaining = Vec::new();
    let mut index = 0;
    while index < tokens.len() {
        if tokens[index] == "--type" {
            let value = value_after(tokens, index, "--type")?;
            if selection.is_some() {
                return Err(CliError::UnexpectedArgument("--type".to_owned()));
            }
            selection = Some(match value.as_str() {
                "all" if !require_concrete => MapSelection::All,
                "knowledge" => MapSelection::One(RepositoryMapType::Knowledge),
                "codespec" => MapSelection::One(RepositoryMapType::Codespec),
                other => return Err(CliError::UnexpectedArgument(other.to_owned())),
            });
            index += 2;
        } else {
            remaining.push(tokens[index].clone());
            index += 1;
        }
    }
    let selection = selection.unwrap_or(MapSelection::All);
    if require_concrete && selection == MapSelection::All {
        return Err(CliError::MissingValue("--type"));
    }
    Ok((selection, remaining))
}

fn require_knowledge_selection(selection: MapSelection) -> Result<(), CliError> {
    if selection == MapSelection::One(RepositoryMapType::Knowledge) {
        Ok(())
    } else {
        Err(CliError::UnexpectedArgument(
            "operation requires --type knowledge".to_owned(),
        ))
    }
}

fn parse_load_hint(value: &str) -> Result<DirectoryLoadHint, CliError> {
    match value {
        "always" => Ok(DirectoryLoadHint::Always),
        "task_match" => Ok(DirectoryLoadHint::TaskMatch),
        "on_demand" => Ok(DirectoryLoadHint::OnDemand),
        other => Err(CliError::UnexpectedArgument(other.to_owned())),
    }
}

fn parse_update_rule(value: &str) -> Result<DirectoryUpdateRule, CliError> {
    match value {
        "reviewed" => Ok(DirectoryUpdateRule::Reviewed),
        "generated" => Ok(DirectoryUpdateRule::Generated),
        "external_sync" => Ok(DirectoryUpdateRule::ExternalSync),
        other => Err(CliError::UnexpectedArgument(other.to_owned())),
    }
}

fn parse_relation(value: &str) -> Result<DirectoryRelation, CliError> {
    let (kind, target) = value
        .split_once('=')
        .ok_or_else(|| CliError::UnexpectedArgument(value.to_owned()))?;
    let kind = match kind {
        "depends_on" => DirectoryRelationKind::DependsOn,
        "implements" => DirectoryRelationKind::Implements,
        "documents" => DirectoryRelationKind::Documents,
        "tests" => DirectoryRelationKind::Tests,
        "operates" => DirectoryRelationKind::Operates,
        "related_to" => DirectoryRelationKind::RelatedTo,
        _ => return Err(CliError::UnexpectedArgument(value.to_owned())),
    };
    Ok(DirectoryRelation {
        kind,
        target: target.to_owned(),
    })
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

#[cfg(test)]
mod mod_tests;
