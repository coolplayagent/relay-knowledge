use crate::{
    api::RequestContext,
    application::RelayKnowledgeService,
    domain::{
        CodeQueryKind, CodeRepositorySetAddMemberRequest, CodeRepositorySetCreateRequest,
        CodeRepositorySetQueryRequest, FreshnessPolicy,
    },
};

use super::{CliError, OutputFormat, parse_freshness, render_response, value_after};

/// Parsed `repo-set` CLI command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RepoSetCommand {
    Create {
        alias: String,
        description: Option<String>,
    },
    Add {
        set_alias: String,
        repository_alias: String,
        ref_selector: String,
        path_filters: Vec<String>,
        language_filters: Vec<String>,
        priority: i32,
    },
    Query {
        set_alias: String,
        query: String,
        kind: CodeQueryKind,
        limit: usize,
        path_filters: Vec<String>,
        language_filters: Vec<String>,
        freshness: FreshnessPolicy,
    },
    Status {
        set_alias: String,
    },
    Refresh {
        set_alias: String,
        async_task: bool,
    },
    RefreshWorker {
        task_id: Option<String>,
    },
}

pub fn parse_repo_set(tokens: &[String]) -> Result<RepoSetCommand, CliError> {
    match tokens.first().map(String::as_str) {
        Some("create") => parse_create(&tokens[1..]),
        Some("add") => parse_add(&tokens[1..]),
        Some("query") => parse_query(&tokens[1..]),
        Some("status") => parse_status(&tokens[1..]),
        Some("refresh") => parse_refresh(&tokens[1..]),
        Some("refresh-worker") => parse_refresh_worker(&tokens[1..]),
        Some(other) => Err(CliError::UnexpectedArgument(other.to_owned())),
        None => Err(CliError::UnexpectedArgument("repo-set".to_owned())),
    }
}

pub async fn run_repo_set(
    service: &RelayKnowledgeService,
    command: RepoSetCommand,
    context: RequestContext,
    format: OutputFormat,
) -> Result<String, CliError> {
    match command {
        RepoSetCommand::Create { alias, description } => {
            let request = CodeRepositorySetCreateRequest::new(alias, description, None)
                .map_err(|error| CliError::ApiFailed(error.to_string()))?;
            let response = service
                .create_code_repository_set(request, context)
                .await
                .map_err(|error| CliError::ApiFailed(error.message))?;

            render_response(
                "code.repo_set.create",
                response.metadata.clone(),
                &response,
                format,
            )
        }
        RepoSetCommand::Add {
            set_alias,
            repository_alias,
            ref_selector,
            path_filters,
            language_filters,
            priority,
        } => {
            let request = CodeRepositorySetAddMemberRequest::new(
                set_alias,
                repository_alias,
                ref_selector,
                path_filters,
                language_filters,
                priority,
            )
            .map_err(|error| CliError::ApiFailed(error.to_string()))?;
            let response = service
                .add_code_repository_set_member(request, context)
                .await
                .map_err(|error| CliError::ApiFailed(error.message))?;

            render_response(
                "code.repo_set.add",
                response.metadata.clone(),
                &response,
                format,
            )
        }
        RepoSetCommand::Query {
            set_alias,
            query,
            kind,
            limit,
            path_filters,
            language_filters,
            freshness,
        } => {
            let request = CodeRepositorySetQueryRequest::new(
                set_alias,
                query,
                kind,
                limit,
                freshness,
                path_filters,
                language_filters,
            )
            .map_err(|error| CliError::ApiFailed(error.to_string()))?;
            let response = service
                .query_code_repository_set(request, context)
                .await
                .map_err(|error| CliError::ApiFailed(error.message))?;

            render_response(
                "code.repo_set.query",
                response.metadata.clone(),
                &response,
                format,
            )
        }
        RepoSetCommand::Status { set_alias } => {
            let response = service
                .code_repository_set_status(set_alias, context)
                .await
                .map_err(|error| CliError::ApiFailed(error.message))?;

            render_response(
                "code.repo_set.status",
                response.metadata.clone(),
                &response,
                format,
            )
        }
        RepoSetCommand::Refresh {
            set_alias,
            async_task,
        } => {
            let response = if async_task {
                service
                    .start_code_repository_set_refresh(set_alias, context)
                    .await
            } else {
                service
                    .refresh_code_repository_set(set_alias, context)
                    .await
            }
            .map_err(|error| CliError::ApiFailed(error.message))?;

            render_response(
                "code.repo_set.refresh",
                response.metadata.clone(),
                &response,
                format,
            )
        }
        RepoSetCommand::RefreshWorker { task_id } => {
            let completed = service
                .run_code_repository_set_refresh_task_once(task_id, context)
                .await
                .map_err(|error| CliError::ApiFailed(error.message))?;
            Ok(match completed {
                Some(task) => serde_json::to_string(&task)
                    .map(|json| format!("{json}\n"))
                    .map_err(|error| CliError::ApiFailed(error.to_string()))?,
                None => String::new(),
            })
        }
    }
}

fn parse_create(tokens: &[String]) -> Result<RepoSetCommand, CliError> {
    let alias = positional(tokens, "<alias>")?;
    let mut description = None;
    let mut index = 1;
    while index < tokens.len() {
        match tokens[index].as_str() {
            "--description" => {
                description = Some(value_after(tokens, index, "--description")?);
                index += 2;
            }
            other => return Err(CliError::UnexpectedArgument(other.to_owned())),
        }
    }

    Ok(RepoSetCommand::Create { alias, description })
}

fn parse_add(tokens: &[String]) -> Result<RepoSetCommand, CliError> {
    let set_alias = positional(tokens, "<set>")?;
    let repository_alias = tokens
        .get(1)
        .filter(|value| !value.starts_with('-'))
        .cloned()
        .ok_or(CliError::MissingValue("<repo-alias>"))?;
    let mut ref_selector = None;
    let mut path_filters = Vec::new();
    let mut language_filters = Vec::new();
    let mut priority = 0;
    let mut index = 2;
    while index < tokens.len() {
        match tokens[index].as_str() {
            "--ref" => {
                ref_selector = Some(value_after(tokens, index, "--ref")?);
                index += 2;
            }
            "--path" => {
                path_filters.push(value_after(tokens, index, "--path")?);
                index += 2;
            }
            "--language" => {
                language_filters.push(value_after(tokens, index, "--language")?);
                index += 2;
            }
            "--priority" => {
                let value = value_after(tokens, index, "--priority")?;
                priority = value
                    .parse::<i32>()
                    .map_err(|_| CliError::InvalidLimit(value.clone()))?;
                index += 2;
            }
            other => return Err(CliError::UnexpectedArgument(other.to_owned())),
        }
    }

    Ok(RepoSetCommand::Add {
        set_alias,
        repository_alias,
        ref_selector: ref_selector.ok_or(CliError::MissingValue("--ref"))?,
        path_filters,
        language_filters,
        priority,
    })
}

fn parse_query(tokens: &[String]) -> Result<RepoSetCommand, CliError> {
    let set_alias = positional(tokens, "<set>")?;
    let mut query = None;
    let mut kind = CodeQueryKind::Hybrid;
    let mut limit = 10;
    let mut path_filters = Vec::new();
    let mut language_filters = Vec::new();
    let mut freshness = FreshnessPolicy::AllowStale;
    let mut index = 1;
    while index < tokens.len() {
        match tokens[index].as_str() {
            "--query" => {
                let (value, next_index) = collect_query_value(tokens, index, "--query")?;
                query = Some(value);
                index = next_index;
            }
            "--kind" => {
                kind = parse_query_kind(&value_after(tokens, index, "--kind")?)?;
                index += 2;
            }
            "--limit" => {
                let value = value_after(tokens, index, "--limit")?;
                limit = value
                    .parse::<usize>()
                    .map_err(|_| CliError::InvalidLimit(value.clone()))?;
                index += 2;
            }
            "--path" => {
                path_filters.push(value_after(tokens, index, "--path")?);
                index += 2;
            }
            "--language" => {
                language_filters.push(value_after(tokens, index, "--language")?);
                index += 2;
            }
            "--freshness" => {
                freshness = parse_freshness(&value_after(tokens, index, "--freshness")?)?;
                index += 2;
            }
            other if !other.starts_with('-') && query.is_none() => {
                let (value, next_index) = collect_positional_query(tokens, index);
                query = Some(value);
                index = next_index;
            }
            other => return Err(CliError::UnexpectedArgument(other.to_owned())),
        }
    }

    Ok(RepoSetCommand::Query {
        set_alias,
        query: query.ok_or(CliError::MissingValue("--query"))?,
        kind,
        limit,
        path_filters,
        language_filters,
        freshness,
    })
}

fn parse_status(tokens: &[String]) -> Result<RepoSetCommand, CliError> {
    Ok(RepoSetCommand::Status {
        set_alias: positional(tokens, "<set>")?,
    })
}

fn parse_refresh(tokens: &[String]) -> Result<RepoSetCommand, CliError> {
    let set_alias = positional(tokens, "<set>")?;
    let mut async_task = false;
    let mut index = 1;
    while index < tokens.len() {
        match tokens[index].as_str() {
            "--async" => {
                async_task = true;
                index += 1;
            }
            other => return Err(CliError::UnexpectedArgument(other.to_owned())),
        }
    }

    Ok(RepoSetCommand::Refresh {
        set_alias,
        async_task,
    })
}

fn parse_refresh_worker(tokens: &[String]) -> Result<RepoSetCommand, CliError> {
    let mut task_id = None;
    let mut index = 0;
    while index < tokens.len() {
        match tokens[index].as_str() {
            "--task-id" => {
                task_id = Some(value_after(tokens, index, "--task-id")?);
                index += 2;
            }
            other => return Err(CliError::UnexpectedArgument(other.to_owned())),
        }
    }

    Ok(RepoSetCommand::RefreshWorker { task_id })
}

fn positional(tokens: &[String], label: &'static str) -> Result<String, CliError> {
    tokens
        .first()
        .filter(|value| !value.starts_with('-'))
        .cloned()
        .ok_or(CliError::MissingValue(label))
}

fn collect_query_value(
    tokens: &[String],
    index: usize,
    flag: &'static str,
) -> Result<(String, usize), CliError> {
    let first = value_after(tokens, index, flag)?;
    let mut values = vec![first];
    let mut next = index + 2;
    while next < tokens.len() && !tokens[next].starts_with('-') {
        values.push(tokens[next].clone());
        next += 1;
    }

    Ok((values.join(" "), next))
}

fn collect_positional_query(tokens: &[String], index: usize) -> (String, usize) {
    let mut values = Vec::new();
    let mut next = index;
    while next < tokens.len() && !tokens[next].starts_with('-') {
        values.push(tokens[next].clone());
        next += 1;
    }

    (values.join(" "), next)
}

fn parse_query_kind(value: &str) -> Result<CodeQueryKind, CliError> {
    match value {
        "hybrid" => Ok(CodeQueryKind::Hybrid),
        "symbol" => Ok(CodeQueryKind::Symbol),
        "definition" => Ok(CodeQueryKind::Definition),
        "references" => Ok(CodeQueryKind::References),
        "callers" => Ok(CodeQueryKind::Callers),
        "callees" => Ok(CodeQueryKind::Callees),
        "imports" => Ok(CodeQueryKind::Imports),
        other => Err(CliError::InvalidCodeQueryKind(other.to_owned())),
    }
}

#[cfg(test)]
#[path = "repo_set_cli_tests.rs"]
mod tests;
