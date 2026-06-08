use crate::{
    api::{
        ApiStreamEvent, CodeRepositoryIndexStartResponse, CodeRepositoryRegisterRequest,
        CodeRepositoryReportResponse, RequestContext, StreamEventKind,
    },
    application::RelayKnowledgeService,
    domain::{
        CodeFeatureFlagRequest, CodeImpactRequest, CodeIndexMode, CodeIndexRequest,
        CodeIndexTaskState, CodeQueryKind, CodeRepositorySelector, CodeRetrievalRequest,
        FreshnessPolicy, SoftwareGlobalKind, SoftwareGlobalRequest,
    },
};

use super::{
    CliError, OutputFormat, parse_freshness, render_response, serialize_line, value_after,
};

#[path = "repo_cli_report.rs"]
mod repo_cli_report;

use repo_cli_report::render_markdown_report;

/// Parsed `repo` CLI command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RepoCommand {
    Register {
        root_path: String,
        alias: String,
        path_filters: Vec<String>,
        language_filters: Vec<String>,
    },
    Remove {
        alias: String,
    },
    Index {
        alias: String,
        ref_selector: String,
        dry_run: bool,
    },
    IndexReset {
        alias: String,
    },
    IndexWorker {
        task_id: Option<String>,
    },
    ScopePreview {
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
        exclude_generated: bool,
    },
    FeatureFlags {
        alias: String,
        query: Option<String>,
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
    Report {
        alias: String,
    },
    Software {
        alias: String,
        ref_selector: String,
        kind: SoftwareGlobalKind,
        freshness: FreshnessPolicy,
        limit: usize,
    },
}

#[derive(serde::Serialize)]
struct CodeIndexWorkerRunResponse {
    claimed: bool,
    task: Option<crate::domain::CodeIndexTaskRecord>,
}

pub fn parse_repo(tokens: &[String]) -> Result<RepoCommand, CliError> {
    match tokens.first().map(String::as_str) {
        Some("register") => parse_register(&tokens[1..]),
        Some("remove") => parse_remove(&tokens[1..]),
        Some("index") => parse_index(&tokens[1..]),
        Some("index-worker") => parse_index_worker(&tokens[1..]),
        Some("scope") => parse_scope(&tokens[1..]),
        Some("update") => parse_update(&tokens[1..]),
        Some("query") => parse_query(&tokens[1..]),
        Some("feature-flags") => parse_feature_flags(&tokens[1..]),
        Some("impact") => parse_impact(&tokens[1..]),
        Some("status") => parse_status(&tokens[1..]),
        Some("report") => parse_report(&tokens[1..]),
        Some("software") => parse_software(&tokens[1..]),
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
                .map_err(|error| CliError::api_failed(error, format))?;

            render_response(
                "code.repo.register",
                response.metadata.clone(),
                &response,
                format,
            )
        }
        RepoCommand::Remove { alias } => {
            let response = service
                .remove_code_repository(alias, context)
                .await
                .map_err(|error| CliError::api_failed(error, format))?;

            render_response(
                "code.repo.remove",
                response.metadata.clone(),
                &response,
                format,
            )
        }
        RepoCommand::Index {
            alias,
            ref_selector,
            dry_run,
        } => {
            let selector = selector(alias, ref_selector, Vec::new(), Vec::new(), format)?;
            let request = CodeIndexRequest {
                repository: selector.clone(),
                mode: CodeIndexMode::Full,
                workspace_detection: Default::default(),
                freshness_policy: FreshnessPolicy::AllowStale,
            };
            if dry_run {
                let response = service
                    .preview_code_repository_scope(request, context)
                    .await
                    .map_err(|error| CliError::api_failed(error, format))?;

                return render_response(
                    "code.repo.scope_preview",
                    response.metadata.clone(),
                    &response,
                    format,
                );
            }
            let worker_context = context.clone();
            let mut response = service
                .start_code_repository_index(request, context)
                .await
                .map_err(|error| CliError::api_failed(error, format))?;
            finish_started_index_task(service, &mut response, selector, worker_context, format)
                .await?;

            render_response(
                "code.repo.index",
                response.metadata.clone(),
                &response,
                format,
            )
        }
        RepoCommand::IndexReset { alias } => {
            let response = service
                .reset_code_repository_index_tasks(alias, context)
                .await
                .map_err(|error| CliError::api_failed(error, format))?;

            render_response(
                "code.repo.index_reset",
                response.metadata.clone(),
                &response,
                format,
            )
        }
        RepoCommand::IndexWorker { task_id } => {
            let completed = service
                .run_code_index_task_once(task_id, context)
                .await
                .map_err(|error| CliError::api_failed(error, format))?;
            let response = CodeIndexWorkerRunResponse {
                claimed: completed.is_some(),
                task: completed,
            };
            render_index_worker_response(&response, format)
        }
        RepoCommand::ScopePreview {
            alias,
            ref_selector,
        } => {
            let response = service
                .preview_code_repository_scope(
                    CodeIndexRequest {
                        repository: selector(alias, ref_selector, Vec::new(), Vec::new(), format)?,
                        mode: CodeIndexMode::Full,
                        workspace_detection: Default::default(),
                        freshness_policy: FreshnessPolicy::AllowStale,
                    },
                    context,
                )
                .await
                .map_err(|error| CliError::api_failed(error, format))?;

            render_response(
                "code.repo.scope_preview",
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
                        repository: selector(
                            alias,
                            head_ref.clone(),
                            Vec::new(),
                            Vec::new(),
                            format,
                        )?,
                        mode: CodeIndexMode::incremental(base_ref, head_ref).map_err(|error| {
                            CliError::invalid_api_argument(error.to_string(), format)
                        })?,
                        workspace_detection: Default::default(),
                        freshness_policy: FreshnessPolicy::WaitUntilFresh,
                    },
                    context,
                )
                .await
                .map_err(|error| CliError::api_failed(error, format))?;

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
            exclude_generated,
        } => {
            let mut request = CodeRetrievalRequest::new(
                query,
                selector(alias, ref_selector, path_filters, language_filters, format)?,
                kind,
                limit,
                freshness,
            )
            .map_err(|error| CliError::invalid_api_argument(error.to_string(), format))?;
            request.exclude_generated = exclude_generated;
            let response = service
                .query_code_repository(request, context)
                .await
                .map_err(|error| CliError::api_failed(error, format))?;

            render_response(
                "code.repo.query",
                response.metadata.clone(),
                &response,
                format,
            )
        }
        RepoCommand::FeatureFlags {
            alias,
            query,
            limit,
            ref_selector,
            path_filters,
            language_filters,
            freshness,
        } => {
            let request = CodeFeatureFlagRequest::new(
                query,
                selector(alias, ref_selector, path_filters, language_filters, format)?,
                limit,
                freshness,
            )
            .map_err(|error| CliError::invalid_api_argument(error.to_string(), format))?;
            let response = service
                .query_code_repository_feature_flags(request, context)
                .await
                .map_err(|error| CliError::api_failed(error, format))?;

            render_response(
                "code.repo.feature_flags",
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
                selector(alias, head_ref.clone(), Vec::new(), Vec::new(), format)?,
                base_ref,
                head_ref,
                limit,
            )
            .map_err(|error| CliError::invalid_api_argument(error.to_string(), format))?;
            let response = service
                .impact_code_repository(request, context)
                .await
                .map_err(|error| CliError::api_failed(error, format))?;

            render_response(
                "code.repo.impact",
                response.metadata.clone(),
                &response,
                format,
            )
        }
        RepoCommand::Status { alias } => {
            let response = service
                .code_repository_status(
                    selector(alias, "HEAD", Vec::new(), Vec::new(), format)?,
                    context,
                )
                .await
                .map_err(|error| CliError::api_failed(error, format))?;

            render_response(
                "code.repo.status",
                response.metadata.clone(),
                &response,
                format,
            )
        }
        RepoCommand::Report { alias } => {
            let response = service
                .code_repository_report(
                    selector(alias, "HEAD", Vec::new(), Vec::new(), format)?,
                    context,
                )
                .await
                .map_err(|error| CliError::api_failed(error, format))?;
            render_report_response(&response, format)
        }
        RepoCommand::Software {
            alias,
            ref_selector,
            kind,
            freshness,
            limit,
        } => {
            let request = SoftwareGlobalRequest::new(
                selector(alias, ref_selector, Vec::new(), Vec::new(), format)?,
                kind,
                freshness,
                limit,
            )
            .map_err(|error| CliError::invalid_api_argument(error.to_string(), format))?;
            let response = service
                .software_global_projection(request, context)
                .await
                .map_err(|error| CliError::api_failed(error, format))?;

            render_response(
                "code.repo.software",
                response.metadata.clone(),
                &response,
                format,
            )
        }
    }
}

pub(super) fn render_report_response(
    response: &CodeRepositoryReportResponse,
    format: OutputFormat,
) -> Result<String, CliError> {
    if format == OutputFormat::Markdown {
        return render_markdown_report(response);
    }

    render_response(
        "code.repo.report",
        response.metadata.clone(),
        response,
        format,
    )
}

fn render_index_worker_response(
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

async fn finish_started_index_task(
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
        alias: alias.unwrap_or_default(),
        path_filters,
        language_filters,
    })
}

fn parse_remove(tokens: &[String]) -> Result<RepoCommand, CliError> {
    let alias = positional_alias(tokens)?;
    if let Some(extra) = tokens.get(1) {
        return Err(CliError::UnexpectedArgument(extra.clone()));
    }

    Ok(RepoCommand::Remove { alias })
}

fn parse_index(tokens: &[String]) -> Result<RepoCommand, CliError> {
    if tokens.first().map(String::as_str) == Some("--reset") {
        let alias = tokens
            .get(1)
            .filter(|value| !value.starts_with('-'))
            .cloned()
            .ok_or(CliError::MissingValue("<alias>"))?;
        if let Some(extra) = tokens.get(2) {
            return Err(CliError::UnexpectedArgument(extra.clone()));
        }

        return Ok(RepoCommand::IndexReset { alias });
    }

    let alias = positional_alias(tokens)?;
    let mut ref_selector = "HEAD".to_owned();
    let mut ref_was_set = false;
    let mut dry_run = false;
    let mut reset = false;
    let mut index = 1;
    while index < tokens.len() {
        match tokens[index].as_str() {
            "--ref" => {
                ref_selector = value_after(tokens, index, "--ref")?;
                ref_was_set = true;
                index += 2;
            }
            "--dry-run" => {
                dry_run = true;
                index += 1;
            }
            "--reset" => {
                reset = true;
                index += 1;
            }
            other => return Err(CliError::UnexpectedArgument(other.to_owned())),
        }
    }
    if reset {
        if dry_run {
            return Err(CliError::UnexpectedArgument("--dry-run".to_owned()));
        }
        if ref_was_set {
            return Err(CliError::UnexpectedArgument("--ref".to_owned()));
        }

        return Ok(RepoCommand::IndexReset { alias });
    }

    Ok(RepoCommand::Index {
        alias,
        ref_selector,
        dry_run,
    })
}

fn parse_index_worker(tokens: &[String]) -> Result<RepoCommand, CliError> {
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

    Ok(RepoCommand::IndexWorker { task_id })
}

fn parse_scope(tokens: &[String]) -> Result<RepoCommand, CliError> {
    if tokens.first().map(String::as_str) != Some("preview") {
        return Err(CliError::UnexpectedArgument(
            tokens
                .first()
                .cloned()
                .unwrap_or_else(|| "scope".to_owned()),
        ));
    }
    let alias = tokens
        .get(1)
        .filter(|value| !value.starts_with('-'))
        .cloned()
        .ok_or(CliError::MissingValue("<alias>"))?;
    let mut ref_selector = "HEAD".to_owned();
    let mut index = 2;
    while index < tokens.len() {
        match tokens[index].as_str() {
            "--ref" => {
                ref_selector = value_after(tokens, index, "--ref")?;
                index += 2;
            }
            other => return Err(CliError::UnexpectedArgument(other.to_owned())),
        }
    }

    Ok(RepoCommand::ScopePreview {
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
    let mut exclude_generated = false;
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
            "--exclude-generated" => {
                exclude_generated = true;
                index += 1;
            }
            other if !other.starts_with('-') && query.is_none() => {
                let (value, next_index) = collect_positional_query(tokens, index);
                query = Some(value);
                index = next_index;
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
        exclude_generated,
    })
}

fn parse_feature_flags(tokens: &[String]) -> Result<RepoCommand, CliError> {
    let alias = positional_alias(tokens)?;
    let mut query = None;
    let mut limit = 50;
    let mut ref_selector = "HEAD".to_owned();
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
                let (value, next_index) = collect_positional_query(tokens, index);
                query = Some(value);
                index = next_index;
            }
            other => return Err(CliError::UnexpectedArgument(other.to_owned())),
        }
    }

    Ok(RepoCommand::FeatureFlags {
        alias,
        query,
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

fn parse_report(tokens: &[String]) -> Result<RepoCommand, CliError> {
    Ok(RepoCommand::Report {
        alias: positional_alias(tokens)?,
    })
}

fn parse_software(tokens: &[String]) -> Result<RepoCommand, CliError> {
    let alias = positional_alias(tokens)?;
    let mut ref_selector = "HEAD".to_owned();
    let mut kind = SoftwareGlobalKind::All;
    let mut freshness = FreshnessPolicy::AllowStale;
    let mut limit = 100;
    let mut index = 1;
    while index < tokens.len() {
        match tokens[index].as_str() {
            "--ref" => {
                ref_selector = value_after(tokens, index, "--ref")?;
                index += 2;
            }
            "--kind" => {
                kind = parse_software_kind(&value_after(tokens, index, "--kind")?)?;
                index += 2;
            }
            "--freshness" => {
                freshness = parse_freshness(&value_after(tokens, index, "--freshness")?)?;
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

    Ok(RepoCommand::Software {
        alias,
        ref_selector,
        kind,
        freshness,
        limit,
    })
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
        "sbom" => Ok(CodeQueryKind::Sbom),
        other => Err(CliError::InvalidCodeQueryKind(other.to_owned())),
    }
}

fn parse_software_kind(value: &str) -> Result<SoftwareGlobalKind, CliError> {
    match value {
        "dependencies" => Ok(SoftwareGlobalKind::Dependencies),
        "sdks" => Ok(SoftwareGlobalKind::Sdks),
        "files" => Ok(SoftwareGlobalKind::Files),
        "topics" => Ok(SoftwareGlobalKind::Topics),
        "relationships" => Ok(SoftwareGlobalKind::Relationships),
        "build" => Ok(SoftwareGlobalKind::Build),
        "iac" => Ok(SoftwareGlobalKind::Iac),
        "design" => Ok(SoftwareGlobalKind::Design),
        "all" => Ok(SoftwareGlobalKind::All),
        other => Err(CliError::InvalidSoftwareKind(other.to_owned())),
    }
}

pub(super) fn selector(
    alias: String,
    ref_selector: impl Into<String>,
    path_filters: Vec<String>,
    language_filters: Vec<String>,
    format: OutputFormat,
) -> Result<CodeRepositorySelector, CliError> {
    CodeRepositorySelector::new(alias, ref_selector, path_filters, language_filters)
        .map_err(|error| CliError::invalid_api_argument(error.to_string(), format))
}

#[cfg(test)]
#[path = "repo_cli_tests.rs"]
mod tests;
