use reqwest::StatusCode;
use serde::{Serialize, de::DeserializeOwned};

use crate::{
    api::{
        ApiError, CodeRepositoryFeatureFlagsResponse, CodeRepositoryImpactResponse,
        CodeRepositoryIndexStartResponse, CodeRepositoryQueryResponse,
        CodeRepositoryReportResponse, CodeRepositoryScopePreviewResponse,
        CodeRepositoryStatusResponse, ErrorKind, RequestContext, SoftwareGlobalResponse,
    },
    domain::{
        CodeFeatureFlagRequest, CodeImpactRequest, CodeIndexMode, CodeIndexRequest,
        CodeRetrievalRequest, FreshnessPolicy, SoftwareGlobalRequest,
    },
    env::NetworkEnvOverrides,
    net::{NetworkConfig, http},
};

use super::{
    CliAction, CliError, OutputFormat,
    cli_render::render_response,
    repo_cli::{self, RepoCommand},
};
use crate::interfaces::code_index_mode::{mode_for_index_ref, selector_for_index_request};

pub(super) fn supports(action: &CliAction) -> bool {
    matches!(
        action,
        CliAction::Repo(
            RepoCommand::Index { .. }
                | RepoCommand::ScopePreview { .. }
                | RepoCommand::Query { .. }
                | RepoCommand::FeatureFlags { .. }
                | RepoCommand::Impact { .. }
                | RepoCommand::Report { .. }
                | RepoCommand::Software { .. }
                | RepoCommand::Status { .. }
        )
    )
}

pub(super) fn blocks_local_fallback(action: &CliAction) -> bool {
    matches!(
        action,
        CliAction::Repo(RepoCommand::IndexReset { .. } | RepoCommand::IndexWorker { .. })
    )
}

pub(super) async fn run_remote(
    network: &NetworkEnvOverrides,
    base_url: &str,
    action: &CliAction,
    context: RequestContext,
    format: OutputFormat,
) -> Result<Option<String>, CliError> {
    let CliAction::Repo(command) = action else {
        return Ok(None);
    };
    let client = RemoteCliClient::new(network, base_url, context, format)?;

    match command {
        RepoCommand::Index {
            alias,
            ref_selector,
            dry_run,
        } => {
            let selected_mode = mode_for_index_ref(ref_selector);
            let mode = if *dry_run {
                CodeIndexMode::Full
            } else {
                selected_mode.clone()
            };
            let selector = repo_cli::selector(
                alias.clone(),
                ref_selector.clone(),
                Vec::new(),
                Vec::new(),
                format,
            )?;
            let request = CodeIndexRequest {
                repository: selector_for_index_request(selector, &selected_mode),
                mode,
                workspace_detection: Default::default(),
                freshness_policy: FreshnessPolicy::AllowStale,
            };
            if *dry_run {
                let response = client
                    .post_repository::<_, CodeRepositoryScopePreviewResponse>(
                        alias,
                        "scope/preview",
                        &request,
                    )
                    .await?;

                return render_response(
                    "code.repo.scope_preview",
                    response.metadata.clone(),
                    &response,
                    format,
                )
                .map(Some);
            }

            let response = client
                .post_repository::<_, CodeRepositoryIndexStartResponse>(alias, "index", &request)
                .await?;

            render_response(
                "code.repo.index",
                response.metadata.clone(),
                &response,
                format,
            )
            .map(Some)
        }
        RepoCommand::ScopePreview {
            alias,
            ref_selector,
        } => {
            let request = CodeIndexRequest {
                repository: repo_cli::selector(
                    alias.clone(),
                    ref_selector.clone(),
                    Vec::new(),
                    Vec::new(),
                    format,
                )?,
                mode: CodeIndexMode::Full,
                workspace_detection: Default::default(),
                freshness_policy: FreshnessPolicy::AllowStale,
            };
            let response = client
                .post_repository::<_, CodeRepositoryScopePreviewResponse>(
                    alias,
                    "scope/preview",
                    &request,
                )
                .await?;

            render_response(
                "code.repo.scope_preview",
                response.metadata.clone(),
                &response,
                format,
            )
            .map(Some)
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
                query.clone(),
                repo_cli::selector(
                    alias.clone(),
                    ref_selector.clone(),
                    path_filters.clone(),
                    language_filters.clone(),
                    format,
                )?,
                *kind,
                *limit,
                *freshness,
            )
            .map_err(|error| CliError::invalid_api_argument(error.to_string(), format))?;
            request.exclude_generated = *exclude_generated;
            let response = client
                .post_repository::<_, CodeRepositoryQueryResponse>(alias, "query", &request)
                .await?;

            render_response(
                "code.repo.query",
                response.metadata.clone(),
                &response,
                format,
            )
            .map(Some)
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
                query.clone(),
                repo_cli::selector(
                    alias.clone(),
                    ref_selector.clone(),
                    path_filters.clone(),
                    language_filters.clone(),
                    format,
                )?,
                *limit,
                *freshness,
            )
            .map_err(|error| CliError::invalid_api_argument(error.to_string(), format))?;
            let response = client
                .post_repository::<_, CodeRepositoryFeatureFlagsResponse>(
                    alias,
                    "feature-flags",
                    &request,
                )
                .await?;

            render_response(
                "code.repo.feature_flags",
                response.metadata.clone(),
                &response,
                format,
            )
            .map(Some)
        }
        RepoCommand::Impact {
            alias,
            base_ref,
            head_ref,
            limit,
        } => {
            let request = CodeImpactRequest::new(
                repo_cli::selector(
                    alias.clone(),
                    head_ref.clone(),
                    Vec::new(),
                    Vec::new(),
                    format,
                )?,
                base_ref.clone(),
                head_ref.clone(),
                *limit,
            )
            .map_err(|error| CliError::invalid_api_argument(error.to_string(), format))?;
            let response = client
                .post_repository::<_, CodeRepositoryImpactResponse>(alias, "impact", &request)
                .await?;

            render_response(
                "code.repo.impact",
                response.metadata.clone(),
                &response,
                format,
            )
            .map(Some)
        }
        RepoCommand::Report { alias } => {
            let response = client
                .get_repository::<CodeRepositoryReportResponse>(alias, "report")
                .await?;

            repo_cli::render_report_response(&response, format).map(Some)
        }
        RepoCommand::Software {
            alias,
            ref_selector,
            kind,
            freshness,
            limit,
        } => {
            let request = SoftwareGlobalRequest::new(
                repo_cli::selector(
                    alias.clone(),
                    ref_selector.clone(),
                    Vec::new(),
                    Vec::new(),
                    format,
                )?,
                *kind,
                *freshness,
                *limit,
            )
            .map_err(|error| CliError::invalid_api_argument(error.to_string(), format))?;
            let response = client
                .post_repository::<_, SoftwareGlobalResponse>(alias, "software", &request)
                .await?;

            render_response(
                "code.repo.software",
                response.metadata.clone(),
                &response,
                format,
            )
            .map(Some)
        }
        RepoCommand::Status { alias } => {
            let response = client
                .get_repository_status::<CodeRepositoryStatusResponse>(alias, "HEAD")
                .await?;

            render_response(
                "code.repo.status",
                response.metadata.clone(),
                &response,
                format,
            )
            .map(Some)
        }
        RepoCommand::IndexReset { .. } | RepoCommand::IndexWorker { .. } => {
            Err(CliError::ApiFailed(
                "remote CLI mode does not support repo index reset or repo index-worker; run maintenance on the service host"
                    .to_owned(),
            ))
        }
        _ => Ok(None),
    }
}

struct RemoteCliClient {
    base_url: String,
    client: reqwest::Client,
    context: RequestContext,
    format: OutputFormat,
}

impl RemoteCliClient {
    fn new(
        network: &NetworkEnvOverrides,
        base_url: &str,
        context: RequestContext,
        format: OutputFormat,
    ) -> Result<Self, CliError> {
        let network = NetworkConfig::from_overrides(network)
            .map_err(|error| CliError::RuntimeConfigFailed(error.to_string()))?;
        let client = http::outbound_json_client(&network.http)
            .map_err(|error| CliError::RuntimeConfigFailed(error.to_string()))?;
        let base_url = normalize_base_url(base_url, format)?;

        Ok(Self {
            base_url,
            client,
            context,
            format,
        })
    }

    async fn post_repository<T, R>(
        &self,
        alias: &str,
        suffix: &str,
        request: &T,
    ) -> Result<R, CliError>
    where
        T: Serialize + ?Sized,
        R: DeserializeOwned,
    {
        let url = repository_url(&self.base_url, alias, suffix, self.format)?;
        let response = self
            .client
            .post(url)
            .header("x-relay-request-id", &self.context.request_id)
            .header("x-relay-trace-id", &self.context.trace_id)
            .json(request)
            .send()
            .await
            .map_err(|error| transport_error(error, self.format))?;

        decode_response(response, self.format).await
    }

    async fn get_repository_status<R>(&self, alias: &str, ref_selector: &str) -> Result<R, CliError>
    where
        R: DeserializeOwned,
    {
        let mut url = repository_url(&self.base_url, alias, "status", self.format)?;
        url.query_pairs_mut().append_pair("ref", ref_selector);
        let response = self
            .client
            .get(url)
            .header("x-relay-request-id", &self.context.request_id)
            .header("x-relay-trace-id", &self.context.trace_id)
            .send()
            .await
            .map_err(|error| transport_error(error, self.format))?;

        decode_response(response, self.format).await
    }

    async fn get_repository<R>(&self, alias: &str, suffix: &str) -> Result<R, CliError>
    where
        R: DeserializeOwned,
    {
        let url = repository_url(&self.base_url, alias, suffix, self.format)?;
        let response = self
            .client
            .get(url)
            .header("x-relay-request-id", &self.context.request_id)
            .header("x-relay-trace-id", &self.context.trace_id)
            .send()
            .await
            .map_err(|error| transport_error(error, self.format))?;

        decode_response(response, self.format).await
    }
}

fn normalize_base_url(value: &str, format: OutputFormat) -> Result<String, CliError> {
    let trimmed = value.trim().trim_end_matches('/');
    let parsed = reqwest::Url::parse(trimmed)
        .map_err(|error| invalid_remote_url(error.to_string(), format))?;
    if !matches!(parsed.scheme(), "http" | "https") || parsed.host_str().is_none() {
        return Err(invalid_remote_url(
            "remote base URL must use http:// or https:// with a host".to_owned(),
            format,
        ));
    }
    if parsed.query().is_some() || parsed.fragment().is_some() {
        return Err(invalid_remote_url(
            "remote base URL must not include query or fragment".to_owned(),
            format,
        ));
    }

    Ok(trimmed.to_owned())
}

fn repository_url(
    base_url: &str,
    alias: &str,
    suffix: &str,
    format: OutputFormat,
) -> Result<reqwest::Url, CliError> {
    let alias = alias.trim();
    if alias.is_empty() {
        return Err(CliError::invalid_api_argument(
            "remote repository alias must not be empty",
            format,
        ));
    }

    let mut url = reqwest::Url::parse(base_url)
        .map_err(|error| invalid_remote_url(error.to_string(), format))?;
    {
        let mut segments = url.path_segments_mut().map_err(|_| {
            invalid_remote_url(
                "remote base URL cannot be a base for path segments".to_owned(),
                format,
            )
        })?;
        for segment in ["api", "v1", "code", "repositories", alias] {
            segments.push(segment);
        }
        for segment in suffix.split('/') {
            segments.push(segment);
        }
    }

    Ok(url)
}

async fn decode_response<R>(
    response: reqwest::Response,
    format: OutputFormat,
) -> Result<R, CliError>
where
    R: DeserializeOwned,
{
    let status = response.status();
    let bytes = response
        .bytes()
        .await
        .map_err(|error| transport_error(error, format))?;
    if !status.is_success() {
        if let Ok(error) = serde_json::from_slice::<ApiError>(&bytes) {
            return Err(CliError::api_failed(error, format));
        }

        return Err(CliError::api_failed(
            status_error(status, String::from_utf8_lossy(&bytes)),
            format,
        ));
    }

    serde_json::from_slice::<R>(&bytes).map_err(|error| {
        CliError::api_failed(
            ApiError {
                error_kind: ErrorKind::Internal,
                message: format!("remote service returned invalid JSON: {error}"),
                metadata: None,
            },
            format,
        )
    })
}

fn transport_error(error: reqwest::Error, format: OutputFormat) -> CliError {
    let error_kind = if error.is_timeout() {
        ErrorKind::Timeout
    } else {
        ErrorKind::StorageUnavailable
    };

    CliError::api_failed(
        ApiError {
            error_kind,
            message: format!("remote service request failed: {error}"),
            metadata: None,
        },
        format,
    )
}

fn invalid_remote_url(message: String, format: OutputFormat) -> CliError {
    CliError::invalid_api_argument(format!("invalid remote service URL: {message}"), format)
}

fn status_error(status: StatusCode, body: std::borrow::Cow<'_, str>) -> ApiError {
    let error_kind = match status {
        StatusCode::BAD_REQUEST => ErrorKind::InvalidArgument,
        StatusCode::REQUEST_TIMEOUT | StatusCode::GATEWAY_TIMEOUT => ErrorKind::Timeout,
        StatusCode::SERVICE_UNAVAILABLE => ErrorKind::StorageUnavailable,
        _ => ErrorKind::Internal,
    };
    let detail = body.trim();
    let message = if detail.is_empty() {
        format!("remote service returned HTTP {status}")
    } else {
        format!("remote service returned HTTP {status}: {detail}")
    };

    ApiError {
        error_kind,
        message,
        metadata: None,
    }
}
