use crate::{
    api::{CodeRepositoryRegisterRequest, RequestContext},
    application::RelayKnowledgeService,
    domain::{
        CodeImpactRequest, CodeIndexMode, CodeIndexRequest, CodeQueryKind, CodeRepositorySelector,
        CodeRetrievalRequest, FreshnessPolicy,
    },
};

use super::{CliError, OutputFormat, parse_freshness, render_response, value_after};

/// Parsed `repo` CLI command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RepoCommand {
    Register {
        root_path: String,
        alias: String,
        path_filters: Vec<String>,
        language_filters: Vec<String>,
    },
    Index {
        alias: String,
        ref_selector: String,
    },
    Update {
        alias: String,
        base_ref: String,
        head_ref: String,
    },
    Query {
        alias: String,
        query: String,
        kind: CodeQueryKind,
        limit: usize,
        ref_selector: String,
        path_filters: Vec<String>,
        language_filters: Vec<String>,
        freshness: FreshnessPolicy,
    },
    Impact {
        alias: String,
        base_ref: String,
        head_ref: String,
        limit: usize,
    },
    Status {
        alias: String,
    },
}

pub fn parse_repo(tokens: &[String]) -> Result<RepoCommand, CliError> {
    match tokens.first().map(String::as_str) {
        Some("register") => parse_register(&tokens[1..]),
        Some("index") => parse_index(&tokens[1..]),
        Some("update") => parse_update(&tokens[1..]),
        Some("query") => parse_query(&tokens[1..]),
        Some("impact") => parse_impact(&tokens[1..]),
        Some("status") => parse_status(&tokens[1..]),
        Some(other) => Err(CliError::UnexpectedArgument(other.to_owned())),
        None => Err(CliError::UnexpectedArgument("repo".to_owned())),
    }
}

pub async fn run_repo(
    service: &RelayKnowledgeService,
    command: RepoCommand,
    context: RequestContext,
    format: OutputFormat,
) -> Result<String, CliError> {
    match command {
        RepoCommand::Register {
            root_path,
            alias,
            path_filters,
            language_filters,
        } => {
            let response = service
                .register_code_repository(
                    CodeRepositoryRegisterRequest {
                        root_path,
                        alias,
                        path_filters,
                        language_filters,
                    },
                    context,
                )
                .await
                .map_err(|error| CliError::ApiFailed(error.message))?;

            render_response(
                "code.repo.register",
                response.metadata.clone(),
                &response,
                format,
            )
        }
        RepoCommand::Index {
            alias,
            ref_selector,
        } => {
            let response = service
                .index_code_repository(
                    CodeIndexRequest {
                        repository: selector(alias, ref_selector, Vec::new(), Vec::new())?,
                        mode: CodeIndexMode::Full,
                        freshness_policy: FreshnessPolicy::WaitUntilFresh,
                    },
                    context,
                )
                .await
                .map_err(|error| CliError::ApiFailed(error.message))?;

            render_response(
                "code.repo.index",
                response.metadata.clone(),
                &response,
                format,
            )
        }
        RepoCommand::Update {
            alias,
            base_ref,
            head_ref,
        } => {
            let response = service
                .index_code_repository(
                    CodeIndexRequest {
                        repository: selector(alias, head_ref.clone(), Vec::new(), Vec::new())?,
                        mode: CodeIndexMode::incremental(base_ref, head_ref)
                            .map_err(|error| CliError::ApiFailed(error.to_string()))?,
                        freshness_policy: FreshnessPolicy::WaitUntilFresh,
                    },
                    context,
                )
                .await
                .map_err(|error| CliError::ApiFailed(error.message))?;

            render_response(
                "code.repo.update",
                response.metadata.clone(),
                &response,
                format,
            )
        }
        RepoCommand::Query {
            alias,
            query,
            kind,
            limit,
            ref_selector,
            path_filters,
            language_filters,
            freshness,
        } => {
            let request = CodeRetrievalRequest::new(
                query,
                selector(alias, ref_selector, path_filters, language_filters)?,
                kind,
                limit,
                freshness,
            )
            .map_err(|error| CliError::ApiFailed(error.to_string()))?;
            let response = service
                .query_code_repository(request, context)
                .await
                .map_err(|error| CliError::ApiFailed(error.message))?;

            render_response(
                "code.repo.query",
                response.metadata.clone(),
                &response,
                format,
            )
        }
        RepoCommand::Impact {
            alias,
            base_ref,
            head_ref,
            limit,
        } => {
            let request = CodeImpactRequest::new(
                selector(alias, head_ref.clone(), Vec::new(), Vec::new())?,
                base_ref,
                head_ref,
                limit,
            )
            .map_err(|error| CliError::ApiFailed(error.to_string()))?;
            let response = service
                .impact_code_repository(request, context)
                .await
                .map_err(|error| CliError::ApiFailed(error.message))?;

            render_response(
                "code.repo.impact",
                response.metadata.clone(),
                &response,
                format,
            )
        }
        RepoCommand::Status { alias } => {
            let response = service
                .code_repository_status(selector(alias, "HEAD", Vec::new(), Vec::new())?, context)
                .await
                .map_err(|error| CliError::ApiFailed(error.message))?;

            render_response(
                "code.repo.status",
                response.metadata.clone(),
                &response,
                format,
            )
        }
    }
}

fn parse_register(tokens: &[String]) -> Result<RepoCommand, CliError> {
    let root_path = tokens
        .first()
        .filter(|value| !value.starts_with('-'))
        .cloned()
        .ok_or(CliError::MissingValue("<path>"))?;
    let mut alias = None;
    let mut path_filters = Vec::new();
    let mut language_filters = Vec::new();
    let mut index = 1;
    while index < tokens.len() {
        match tokens[index].as_str() {
            "--alias" => {
                alias = Some(value_after(tokens, index, "--alias")?);
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
            other => return Err(CliError::UnexpectedArgument(other.to_owned())),
        }
    }

    Ok(RepoCommand::Register {
        root_path,
        alias: alias.ok_or(CliError::MissingValue("--alias"))?,
        path_filters,
        language_filters,
    })
}

fn parse_index(tokens: &[String]) -> Result<RepoCommand, CliError> {
    let alias = positional_alias(tokens)?;
    let mut ref_selector = "HEAD".to_owned();
    let mut index = 1;
    while index < tokens.len() {
        match tokens[index].as_str() {
            "--ref" => {
                ref_selector = value_after(tokens, index, "--ref")?;
                index += 2;
            }
            other => return Err(CliError::UnexpectedArgument(other.to_owned())),
        }
    }

    Ok(RepoCommand::Index {
        alias,
        ref_selector,
    })
}

fn parse_update(tokens: &[String]) -> Result<RepoCommand, CliError> {
    let alias = positional_alias(tokens)?;
    let (base_ref, head_ref, _) = parse_base_head_limit(tokens, 1, 50)?;

    Ok(RepoCommand::Update {
        alias,
        base_ref: base_ref.ok_or(CliError::MissingValue("--base"))?,
        head_ref: head_ref.ok_or(CliError::MissingValue("--head"))?,
    })
}

fn parse_query(tokens: &[String]) -> Result<RepoCommand, CliError> {
    let alias = positional_alias(tokens)?;
    let mut query = None;
    let mut kind = CodeQueryKind::Hybrid;
    let mut limit = 10;
    let mut ref_selector = "HEAD".to_owned();
    let mut path_filters = Vec::new();
    let mut language_filters = Vec::new();
    let mut freshness = FreshnessPolicy::AllowStale;
    let mut index = 1;
    while index < tokens.len() {
        match tokens[index].as_str() {
            "--query" => {
                query = Some(value_after(tokens, index, "--query")?);
                index += 2;
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
            "--ref" => {
                ref_selector = value_after(tokens, index, "--ref")?;
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
                query = Some(other.to_owned());
                index += 1;
            }
            other => return Err(CliError::UnexpectedArgument(other.to_owned())),
        }
    }

    Ok(RepoCommand::Query {
        alias,
        query: query.ok_or(CliError::MissingValue("--query"))?,
        kind,
        limit,
        ref_selector,
        path_filters,
        language_filters,
        freshness,
    })
}

fn parse_impact(tokens: &[String]) -> Result<RepoCommand, CliError> {
    let alias = positional_alias(tokens)?;
    let (base_ref, head_ref, limit) = parse_base_head_limit(tokens, 1, 100)?;

    Ok(RepoCommand::Impact {
        alias,
        base_ref: base_ref.ok_or(CliError::MissingValue("--base"))?,
        head_ref: head_ref.ok_or(CliError::MissingValue("--head"))?,
        limit,
    })
}

fn parse_status(tokens: &[String]) -> Result<RepoCommand, CliError> {
    Ok(RepoCommand::Status {
        alias: positional_alias(tokens)?,
    })
}

fn parse_base_head_limit(
    tokens: &[String],
    start_index: usize,
    default_limit: usize,
) -> Result<(Option<String>, Option<String>, usize), CliError> {
    let mut base_ref = None;
    let mut head_ref = None;
    let mut limit = default_limit;
    let mut index = start_index;
    while index < tokens.len() {
        match tokens[index].as_str() {
            "--base" => {
                base_ref = Some(value_after(tokens, index, "--base")?);
                index += 2;
            }
            "--head" => {
                head_ref = Some(value_after(tokens, index, "--head")?);
                index += 2;
            }
            "--limit" => {
                let value = value_after(tokens, index, "--limit")?;
                limit = value
                    .parse::<usize>()
                    .map_err(|_| CliError::InvalidLimit(value.clone()))?;
                index += 2;
            }
            other => return Err(CliError::UnexpectedArgument(other.to_owned())),
        }
    }

    Ok((base_ref, head_ref, limit))
}

fn positional_alias(tokens: &[String]) -> Result<String, CliError> {
    tokens
        .first()
        .filter(|value| !value.starts_with('-'))
        .cloned()
        .ok_or(CliError::MissingValue("<alias>"))
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
        "impact" => Ok(CodeQueryKind::Impact),
        other => Err(CliError::InvalidCodeQueryKind(other.to_owned())),
    }
}

fn selector(
    alias: String,
    ref_selector: impl Into<String>,
    path_filters: Vec<String>,
    language_filters: Vec<String>,
) -> Result<CodeRepositorySelector, CliError> {
    CodeRepositorySelector::new(alias, ref_selector, path_filters, language_filters)
        .map_err(|error| CliError::ApiFailed(error.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_repo_query_with_kind_filters_and_freshness() {
        let command = parse_repo(&[
            "query".to_owned(),
            "core".to_owned(),
            "--query".to_owned(),
            "RetryPolicy".to_owned(),
            "--kind".to_owned(),
            "references".to_owned(),
            "--path".to_owned(),
            "src".to_owned(),
            "--language".to_owned(),
            "rust".to_owned(),
            "--freshness".to_owned(),
            "wait-until-fresh".to_owned(),
        ])
        .expect("repo query should parse");

        assert_eq!(
            command,
            RepoCommand::Query {
                alias: "core".to_owned(),
                query: "RetryPolicy".to_owned(),
                kind: CodeQueryKind::References,
                limit: 10,
                ref_selector: "HEAD".to_owned(),
                path_filters: vec!["src".to_owned()],
                language_filters: vec!["rust".to_owned()],
                freshness: FreshnessPolicy::WaitUntilFresh,
            }
        );
    }
}
