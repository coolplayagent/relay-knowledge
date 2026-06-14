use crate::{
    api::RequestContext,
    application::RelayKnowledgeService,
    domain::{CodebaseViewKind, CodebaseViewRequest, FreshnessPolicy},
};

use super::{CliError, OutputFormat, parse_freshness, render_response, value_after};

/// Parsed `repo view` command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepoViewCommand {
    pub alias: String,
    pub kind: CodebaseViewKind,
    pub limit: usize,
    pub ref_selector: String,
    pub path_filters: Vec<String>,
    pub language_filters: Vec<String>,
    pub freshness: FreshnessPolicy,
    pub changed_paths: Vec<String>,
}

pub(super) fn parse_view(tokens: &[String]) -> Result<RepoViewCommand, CliError> {
    let alias = tokens
        .first()
        .filter(|value| !value.starts_with('-'))
        .cloned()
        .ok_or(CliError::MissingValue("<alias>"))?;
    let mut kind = CodebaseViewKind::ArchitectureLayers;
    let mut limit = 20;
    let mut ref_selector = "HEAD".to_owned();
    let mut path_filters = Vec::new();
    let mut language_filters = Vec::new();
    let mut freshness = FreshnessPolicy::AllowStale;
    let mut changed_paths = Vec::new();
    let mut index = 1;
    while index < tokens.len() {
        match tokens[index].as_str() {
            "--kind" => {
                kind = parse_view_kind(&value_after(tokens, index, "--kind")?)?;
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
            "--changed-path" => {
                changed_paths.push(value_after(tokens, index, "--changed-path")?);
                index += 2;
            }
            other => return Err(CliError::UnexpectedArgument(other.to_owned())),
        }
    }

    Ok(RepoViewCommand {
        alias,
        kind,
        limit,
        ref_selector,
        path_filters,
        language_filters,
        freshness,
        changed_paths,
    })
}

pub(super) async fn run_view(
    service: &RelayKnowledgeService,
    command: RepoViewCommand,
    context: RequestContext,
    format: OutputFormat,
) -> Result<String, CliError> {
    let request = request(command, format)?;
    let response = service
        .codebase_view(request, context)
        .await
        .map_err(|error| CliError::api_failed(error, format))?;

    render_response(
        "code.repo.view",
        response.metadata.clone(),
        &response,
        format,
    )
}

pub(crate) fn request(
    command: RepoViewCommand,
    format: OutputFormat,
) -> Result<CodebaseViewRequest, CliError> {
    CodebaseViewRequest::new(
        super::repo_cli::selector(
            command.alias,
            command.ref_selector,
            command.path_filters,
            command.language_filters,
            format,
        )?,
        command.kind,
        command.freshness,
        command.limit,
        command.changed_paths,
    )
    .map_err(|error| CliError::invalid_api_argument(error.to_string(), format))
}

pub(super) fn parse_view_kind(value: &str) -> Result<CodebaseViewKind, CliError> {
    match value {
        "architecture-layers" | "architecture_layers" => Ok(CodebaseViewKind::ArchitectureLayers),
        "business-domains" | "business_domains" => Ok(CodebaseViewKind::BusinessDomains),
        "dependency-tour" | "dependency_tour" => Ok(CodebaseViewKind::DependencyTour),
        "process-flow" | "process_flow" => Ok(CodebaseViewKind::ProcessFlow),
        "affected-scope" | "affected_scope" => Ok(CodebaseViewKind::AffectedScope),
        other => Err(CliError::UnexpectedArgument(format!("--kind {other}"))),
    }
}
