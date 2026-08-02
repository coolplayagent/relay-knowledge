//! Local-file CLI parsing, execution, and bounded refresh-loop ownership.

use crate::{
    api::{FileContentQueryRequest, FileIndexRequest, FileQueryRequest, RequestContext},
    application::{DEFAULT_FILE_QUERY_LIMIT, RelayKnowledgeService},
};

use super::{
    CliAction, CliError, OutputFormat,
    command::{parse_freshness, value_after},
    render::render_response,
};

pub(super) fn parse_files(tokens: &[String]) -> Result<CliAction, CliError> {
    match tokens.first().map(String::as_str) {
        Some("index") => parse_files_index(&tokens[1..]),
        Some("query") => parse_files_query(&tokens[1..]),
        Some("content") => parse_files_content_query(&tokens[1..]),
        other => Err(CliError::UnexpectedArgument(
            other.unwrap_or("files").to_owned(),
        )),
    }
}

pub(super) async fn run_files(
    service: &RelayKnowledgeService,
    action: &CliAction,
    context: RequestContext,
    format: OutputFormat,
) -> Result<Option<String>, CliError> {
    match action {
        CliAction::FilesIndex {
            source_scope,
            roots,
        } => {
            let response = service
                .index_files(
                    FileIndexRequest {
                        source_scope: source_scope.clone(),
                        roots: roots.clone(),
                    },
                    context,
                )
                .await
                .map_err(|error| CliError::api_failed(error, format))?;

            render_response("files.index", response.metadata.clone(), &response, format).map(Some)
        }
        CliAction::FilesQuery {
            query,
            source_scope,
            root_id,
            limit,
            freshness,
        } => {
            let response = service
                .query_files(
                    FileQueryRequest {
                        query: query.clone(),
                        source_scope: source_scope.clone(),
                        root_id: root_id.clone(),
                        limit: *limit,
                        freshness_policy: *freshness,
                    },
                    context,
                )
                .await
                .map_err(|error| CliError::api_failed(error, format))?;

            render_response("files.query", response.metadata.clone(), &response, format).map(Some)
        }
        CliAction::FilesContentQuery {
            query,
            source_scope,
            root_id,
            limit,
            freshness,
        } => {
            let response = service
                .query_file_content(
                    FileContentQueryRequest {
                        query: query.clone(),
                        source_scope: source_scope.clone(),
                        root_id: root_id.clone(),
                        limit: *limit,
                        freshness_policy: *freshness,
                    },
                    context,
                )
                .await
                .map_err(|error| CliError::api_failed(error, format))?;

            render_response(
                "files.content",
                response.metadata.clone(),
                &response,
                format,
            )
            .map(Some)
        }
        _ => Ok(None),
    }
}

pub(super) async fn run_file_index_loop(
    service: RelayKnowledgeService,
    interval: std::time::Duration,
    mut shutdown: tokio::sync::watch::Receiver<bool>,
) {
    loop {
        tokio::select! {
            _ = shutdown.changed() => break,
            _ = service.index_configured_files_once() => {}
        }
        tokio::select! {
            _ = shutdown.changed() => break,
            _ = tokio::time::sleep(interval) => {}
        }
    }
}

fn parse_files_index(tokens: &[String]) -> Result<CliAction, CliError> {
    let mut source_scope = None;
    let mut roots = Vec::new();
    let mut index = 0;
    while index < tokens.len() {
        match tokens[index].as_str() {
            "--source" => {
                source_scope = Some(value_after(tokens, index, "--source")?);
                index += 2;
            }
            "--root" => {
                roots.push(value_after(tokens, index, "--root")?);
                index += 2;
            }
            other => return Err(CliError::UnexpectedArgument(other.to_owned())),
        }
    }

    Ok(CliAction::FilesIndex {
        source_scope,
        roots,
    })
}

fn parse_files_query(tokens: &[String]) -> Result<CliAction, CliError> {
    parse_file_text_query(tokens, false)
}

fn parse_files_content_query(tokens: &[String]) -> Result<CliAction, CliError> {
    parse_file_text_query(tokens, true)
}

fn parse_file_text_query(tokens: &[String], content: bool) -> Result<CliAction, CliError> {
    let mut query = None;
    let mut source_scope = None;
    let mut root_id = None;
    let mut limit = DEFAULT_FILE_QUERY_LIMIT;
    let mut freshness = crate::domain::FreshnessPolicy::AllowStale;
    let mut index = 0;

    while index < tokens.len() {
        match tokens[index].as_str() {
            "--" if query.is_none() => {
                query = Some(value_after(tokens, index, "query")?);
                index += 2;
            }
            "--source" => {
                source_scope = Some(value_after(tokens, index, "--source")?);
                index += 2;
            }
            "--root" => {
                root_id = Some(value_after(tokens, index, "--root")?);
                index += 2;
            }
            "--limit" => {
                let value = value_after(tokens, index, "--limit")?;
                limit = value
                    .parse::<usize>()
                    .map_err(|_| CliError::InvalidLimit(value.clone()))?;
                index += 2;
            }
            "--freshness" => {
                freshness = parse_freshness(&value_after(tokens, index, "--freshness")?)?;
                index += 2;
            }
            other if !other.starts_with('-') && query.is_none() => {
                let mut values = vec![other.to_owned()];
                index += 1;
                while index < tokens.len() && !tokens[index].starts_with('-') {
                    values.push(tokens[index].clone());
                    index += 1;
                }
                query = Some(values.join(" "));
            }
            other => return Err(CliError::UnexpectedArgument(other.to_owned())),
        }
    }

    let query = query.ok_or(CliError::MissingValue("query"))?;
    if content {
        Ok(CliAction::FilesContentQuery {
            query,
            source_scope,
            root_id,
            limit,
            freshness,
        })
    } else {
        Ok(CliAction::FilesQuery {
            query,
            source_scope,
            root_id,
            limit,
            freshness,
        })
    }
}

#[cfg(test)]
mod mod_tests;
