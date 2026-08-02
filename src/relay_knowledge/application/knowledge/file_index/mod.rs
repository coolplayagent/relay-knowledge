use std::{
    path::PathBuf,
    time::{Instant, SystemTime, UNIX_EPOCH},
};

use crate::{
    api::{
        ApiError, ApiMetadata, FileContentQueryRequest, FileContentQueryResponse,
        FileIndexFreshnessState, FileIndexRequest, FileIndexResponse, FileQueryRequest,
        FileQueryResponse, RequestContext,
    },
    domain::{FreshnessPolicy, GraphVersion},
    storage::{FileContentSearchRequest, FileIndexScanSummary, FileSearchRequest, StorageError},
};

use crate::application::{FileIndexRootConfig, service::RelayKnowledgeService};

mod content;
mod scanner;
#[cfg(test)]
#[path = "test_support_tests.rs"]
mod test_support;

use super::file_freshness::{FileFreshnessContext, file_freshness_diagnostics};
use scanner::{ScanBudget, file_index_root_from_config, scan_roots, summary_from_diagnostics};

pub const DEFAULT_FILE_QUERY_LIMIT: usize = 20;
const MAX_FILE_QUERY_LIMIT: usize = 500;

impl RelayKnowledgeService {
    /// Scans configured or explicit file roots into the local file-location index.
    pub async fn index_files(
        &self,
        request: FileIndexRequest,
        context: RequestContext,
    ) -> Result<FileIndexResponse, ApiError> {
        let configured_scan = request.roots.is_empty();
        let roots = self
            .file_index_roots_from_request(request)
            .map_err(ApiError::invalid_argument)?;
        let active_roots = roots
            .iter()
            .map(file_index_root_from_config)
            .collect::<Vec<_>>();
        let store = self.storage.get().await.map_err(storage_api_error)?;
        let now_ms = current_time_millis();
        let updates = scan_roots(
            roots,
            ScanBudget {
                max_depth: self.runtime.file_index.max_depth,
                max_file_bytes: self.runtime.file_index.max_file_bytes,
                max_files_per_root: self.runtime.file_index.max_files_per_root,
                excludes: self.runtime.file_index.excludes.clone(),
            },
            now_ms,
            self.runtime.file_index.scan_timeout,
        )
        .await
        .map_err(storage_api_error)?;
        let mut summary = FileIndexScanSummary::default();
        for update in updates {
            let status = store
                .replace_file_index_root(update)
                .await
                .map_err(storage_api_error)?;
            summary.root_count = summary.root_count.saturating_add(1);
            summary.indexed_file_count = summary
                .indexed_file_count
                .saturating_add(status.indexed_file_count);
            summary.missing_file_count = summary
                .missing_file_count
                .saturating_add(status.missing_file_count);
            summary.indexed_content_count = summary
                .indexed_content_count
                .saturating_add(status.indexed_content_count);
            summary.skipped_content_count = summary
                .skipped_content_count
                .saturating_add(status.skipped_content_count);
            summary.unchanged_content_count = summary
                .unchanged_content_count
                .saturating_add(status.unchanged_content_count);
            summary.stale_content_cursor_count = summary
                .stale_content_cursor_count
                .saturating_add(status.stale_content_cursor_count);
            summary.scan_error_count = summary
                .scan_error_count
                .saturating_add(status.scan_error_count);
            summary.content_read_error_count = summary
                .content_read_error_count
                .saturating_add(status.content_read_error_count);
            if status.truncated {
                summary.truncated_root_count = summary.truncated_root_count.saturating_add(1);
            }
            summary.roots.push(status);
        }
        if configured_scan {
            let diagnostics = store
                .mark_file_index_roots_unconfigured(active_roots, now_ms)
                .await
                .map_err(storage_api_error)?;
            summary = summary_from_diagnostics(diagnostics);
        }

        Ok(FileIndexResponse {
            metadata: ApiMetadata::graph_only(&context, GraphVersion::ZERO),
            summary,
        })
    }

    /// Runs one scan over configured roots when background file indexing is enabled.
    pub async fn index_configured_files_once(&self) -> Result<FileIndexResponse, ApiError> {
        if self.runtime.file_index.roots.is_empty() {
            let store = self.storage.get().await.map_err(storage_api_error)?;
            let diagnostics = store
                .mark_file_index_roots_unconfigured(Vec::new(), current_time_millis())
                .await
                .map_err(storage_api_error)?;
            return Ok(FileIndexResponse {
                metadata: ApiMetadata::graph_only(
                    &RequestContext::for_interface(crate::api::InterfaceKind::Cli),
                    GraphVersion::ZERO,
                ),
                summary: summary_from_diagnostics(diagnostics),
            });
        }

        self.index_files(
            FileIndexRequest {
                source_scope: None,
                roots: Vec::new(),
            },
            RequestContext::for_interface(crate::api::InterfaceKind::Cli),
        )
        .await
    }

    /// Queries the local file-location index with bounded latency.
    pub async fn query_files(
        &self,
        request: FileQueryRequest,
        context: RequestContext,
    ) -> Result<FileQueryResponse, ApiError> {
        let query = required_query(request.query).map_err(ApiError::invalid_argument)?;
        let limit = bounded_limit(request.limit).map_err(ApiError::invalid_argument)?;
        let store = self.storage.get().await.map_err(storage_api_error)?;
        let started = Instant::now();
        let source_scope =
            normalize_optional_text(request.source_scope).map_err(ApiError::invalid_argument)?;
        let root_id =
            normalize_optional_text(request.root_id).map_err(ApiError::invalid_argument)?;
        let configured_roots = self
            .runtime
            .file_index
            .roots
            .iter()
            .map(file_index_root_from_config)
            .collect::<Vec<_>>();
        let diagnostics = store
            .file_index_diagnostics()
            .await
            .map_err(storage_api_error)?;
        if request.freshness_policy == FreshnessPolicy::GraphOnly {
            let degraded_reason = "graph_only freshness policy selected".to_owned();
            let freshness = file_freshness_diagnostics(FileFreshnessContext {
                file_index_enabled: self.runtime.file_index.enabled,
                configured_roots: &configured_roots,
                diagnostics: &diagnostics,
                freshness_policy: request.freshness_policy,
                source_scope: source_scope.clone(),
                root_id: root_id.clone(),
                graph_version: GraphVersion::ZERO.get(),
                query_degraded_reason: Some(degraded_reason.clone()),
                returned_paths: &[],
                content_required: false,
            });
            return Ok(FileQueryResponse {
                metadata: ApiMetadata::graph_only(&context, GraphVersion::ZERO),
                query,
                source_scope,
                root_id,
                freshness,
                results: Vec::new(),
                truncated: false,
                duration_ms: elapsed_ms(started),
                degraded_reason: Some(degraded_reason),
            });
        }
        let freshness = file_freshness_diagnostics(FileFreshnessContext {
            file_index_enabled: self.runtime.file_index.enabled,
            configured_roots: &configured_roots,
            diagnostics: &diagnostics,
            freshness_policy: request.freshness_policy,
            source_scope: source_scope.clone(),
            root_id: root_id.clone(),
            graph_version: GraphVersion::ZERO.get(),
            query_degraded_reason: None,
            returned_paths: &[],
            content_required: false,
        });
        if request.freshness_policy == FreshnessPolicy::WaitUntilFresh
            && freshness.state != FileIndexFreshnessState::Fresh
        {
            return Err(ApiError::invalid_argument(format!(
                "file index is {}; run files index before querying with wait_until_fresh",
                file_freshness_state_label(freshness.state)
            )));
        }
        let results = match store
            .search_files(FileSearchRequest {
                query: query.clone(),
                source_scope: source_scope.clone(),
                root_id: root_id.clone(),
                limit: limit.saturating_add(1),
                timeout_ms: query_timeout_ms(self.runtime.file_index.query_timeout),
            })
            .await
        {
            Ok(results) => results,
            Err(error) if storage_error_timed_out(&error) => {
                let degraded_reason = "file query timed out".to_owned();
                let freshness = file_freshness_diagnostics(FileFreshnessContext {
                    file_index_enabled: self.runtime.file_index.enabled,
                    configured_roots: &configured_roots,
                    diagnostics: &diagnostics,
                    freshness_policy: request.freshness_policy,
                    source_scope: source_scope.clone(),
                    root_id: root_id.clone(),
                    graph_version: GraphVersion::ZERO.get(),
                    query_degraded_reason: Some(degraded_reason.clone()),
                    returned_paths: &[],
                    content_required: false,
                });
                return Ok(FileQueryResponse {
                    metadata: ApiMetadata::graph_only(&context, GraphVersion::ZERO),
                    query,
                    source_scope,
                    root_id,
                    freshness,
                    results: Vec::new(),
                    truncated: false,
                    duration_ms: elapsed_ms(started),
                    degraded_reason: Some(degraded_reason),
                });
            }
            Err(error) => return Err(storage_api_error(error)),
        };
        let mut results = results;
        let truncated = results.len() > limit;
        results.truncate(limit);
        let result_paths = results
            .iter()
            .map(|hit| hit.path.clone())
            .collect::<Vec<_>>();
        let freshness = file_freshness_diagnostics(FileFreshnessContext {
            file_index_enabled: self.runtime.file_index.enabled,
            configured_roots: &configured_roots,
            diagnostics: &diagnostics,
            freshness_policy: request.freshness_policy,
            source_scope: source_scope.clone(),
            root_id: root_id.clone(),
            graph_version: GraphVersion::ZERO.get(),
            query_degraded_reason: None,
            returned_paths: &result_paths,
            content_required: false,
        });

        Ok(FileQueryResponse {
            metadata: ApiMetadata::graph_only(&context, GraphVersion::ZERO),
            query,
            source_scope,
            root_id,
            freshness,
            results,
            truncated,
            duration_ms: elapsed_ms(started),
            degraded_reason: None,
        })
    }

    /// Queries the local file-content read model with provenance and role isolation.
    pub async fn query_file_content(
        &self,
        request: FileContentQueryRequest,
        context: RequestContext,
    ) -> Result<FileContentQueryResponse, ApiError> {
        let query = required_query(request.query).map_err(ApiError::invalid_argument)?;
        let limit = bounded_limit(request.limit).map_err(ApiError::invalid_argument)?;
        let store = self.storage.get().await.map_err(storage_api_error)?;
        let started = Instant::now();
        let source_scope =
            normalize_optional_text(request.source_scope).map_err(ApiError::invalid_argument)?;
        let root_id =
            normalize_optional_text(request.root_id).map_err(ApiError::invalid_argument)?;
        let configured_roots = self
            .runtime
            .file_index
            .roots
            .iter()
            .map(file_index_root_from_config)
            .collect::<Vec<_>>();
        let diagnostics = store
            .file_index_diagnostics()
            .await
            .map_err(storage_api_error)?;
        if request.freshness_policy == FreshnessPolicy::GraphOnly {
            let degraded_reason = "graph_only freshness policy selected".to_owned();
            let freshness = file_freshness_diagnostics(FileFreshnessContext {
                file_index_enabled: self.runtime.file_index.enabled,
                configured_roots: &configured_roots,
                diagnostics: &diagnostics,
                freshness_policy: request.freshness_policy,
                source_scope: source_scope.clone(),
                root_id: root_id.clone(),
                graph_version: GraphVersion::ZERO.get(),
                query_degraded_reason: Some(degraded_reason.clone()),
                returned_paths: &[],
                content_required: true,
            });
            return Ok(FileContentQueryResponse {
                metadata: ApiMetadata::graph_only(&context, GraphVersion::ZERO),
                query,
                source_scope,
                root_id,
                freshness,
                results: Vec::new(),
                truncated: false,
                duration_ms: elapsed_ms(started),
                degraded_reason: Some(degraded_reason),
            });
        }
        let freshness = file_freshness_diagnostics(FileFreshnessContext {
            file_index_enabled: self.runtime.file_index.enabled,
            configured_roots: &configured_roots,
            diagnostics: &diagnostics,
            freshness_policy: request.freshness_policy,
            source_scope: source_scope.clone(),
            root_id: root_id.clone(),
            graph_version: GraphVersion::ZERO.get(),
            query_degraded_reason: None,
            returned_paths: &[],
            content_required: true,
        });
        if request.freshness_policy == FreshnessPolicy::WaitUntilFresh
            && freshness.state != FileIndexFreshnessState::Fresh
        {
            return Err(ApiError::invalid_argument(format!(
                "file content index is {}; run files index before querying with wait_until_fresh",
                file_freshness_state_label(freshness.state)
            )));
        }
        let results = match store
            .search_file_content(FileContentSearchRequest {
                query: query.clone(),
                source_scope: source_scope.clone(),
                root_id: root_id.clone(),
                authorized_roots: configured_roots.clone(),
                limit: limit.saturating_add(1),
                timeout_ms: query_timeout_ms(self.runtime.file_index.query_timeout),
            })
            .await
        {
            Ok(results) => results,
            Err(error) if storage_error_timed_out(&error) => {
                let degraded_reason = "file content query timed out".to_owned();
                let freshness = file_freshness_diagnostics(FileFreshnessContext {
                    file_index_enabled: self.runtime.file_index.enabled,
                    configured_roots: &configured_roots,
                    diagnostics: &diagnostics,
                    freshness_policy: request.freshness_policy,
                    source_scope: source_scope.clone(),
                    root_id: root_id.clone(),
                    graph_version: GraphVersion::ZERO.get(),
                    query_degraded_reason: Some(degraded_reason.clone()),
                    returned_paths: &[],
                    content_required: true,
                });
                return Ok(FileContentQueryResponse {
                    metadata: ApiMetadata::graph_only(&context, GraphVersion::ZERO),
                    query,
                    source_scope,
                    root_id,
                    freshness,
                    results: Vec::new(),
                    truncated: false,
                    duration_ms: elapsed_ms(started),
                    degraded_reason: Some(degraded_reason),
                });
            }
            Err(error) => return Err(storage_api_error(error)),
        };
        let mut results = results;
        let truncated = results.len() > limit;
        results.truncate(limit);
        let result_paths = results
            .iter()
            .map(|hit| hit.path.clone())
            .collect::<Vec<_>>();
        let freshness = file_freshness_diagnostics(FileFreshnessContext {
            file_index_enabled: self.runtime.file_index.enabled,
            configured_roots: &configured_roots,
            diagnostics: &diagnostics,
            freshness_policy: request.freshness_policy,
            source_scope: source_scope.clone(),
            root_id: root_id.clone(),
            graph_version: GraphVersion::ZERO.get(),
            query_degraded_reason: None,
            returned_paths: &result_paths,
            content_required: true,
        });

        Ok(FileContentQueryResponse {
            metadata: ApiMetadata::graph_only(&context, GraphVersion::ZERO),
            query,
            source_scope,
            root_id,
            freshness,
            results,
            truncated,
            duration_ms: elapsed_ms(started),
            degraded_reason: None,
        })
    }

    fn file_index_roots_from_request(
        &self,
        request: FileIndexRequest,
    ) -> Result<Vec<FileIndexRootConfig>, String> {
        if request.roots.is_empty() {
            if self.runtime.file_index.roots.is_empty() {
                return Err("no file index roots are configured".to_owned());
            }
            return Ok(self.runtime.file_index.roots.clone());
        }

        let scope_id = normalize_optional_text(request.source_scope)?
            .unwrap_or_else(|| "local-files".to_owned());
        if self.runtime.file_index.roots.is_empty() {
            return Err(
                "file index roots must be configured before explicit roots can be scanned"
                    .to_owned(),
            );
        }
        let mut roots = request
            .roots
            .into_iter()
            .map(|root| {
                let root = root.trim();
                if root.is_empty() {
                    Err("file index root must not be empty".to_owned())
                } else {
                    let root_path = PathBuf::from(root);
                    if !root_path.is_absolute() {
                        return Err("file index root must be an absolute path".to_owned());
                    }
                    let requested = FileIndexRootConfig::new(&scope_id, root_path);
                    self.runtime
                        .file_index
                        .roots
                        .iter()
                        .find(|authorized| {
                            authorized.scope_id == requested.scope_id
                                && authorized.root_id == requested.root_id
                        })
                        .cloned()
                        .ok_or_else(|| {
                            format!(
                                "file index root '{root}' is not configured for scope '{scope_id}'"
                            )
                        })
                }
            })
            .collect::<Result<Vec<_>, _>>()?;
        roots.sort_by(|left, right| {
            left.scope_id
                .cmp(&right.scope_id)
                .then(left.root_id.cmp(&right.root_id))
        });
        roots.dedup_by(|left, right| {
            left.scope_id == right.scope_id && left.root_id == right.root_id
        });

        Ok(roots)
    }
}
fn required_query(query: String) -> Result<String, String> {
    let query = query.trim().to_owned();
    if query.is_empty() {
        Err("file query must not be empty".to_owned())
    } else {
        Ok(query)
    }
}

fn bounded_limit(limit: usize) -> Result<usize, String> {
    match limit {
        0 => Err("file query limit must be greater than zero".to_owned()),
        value if value > MAX_FILE_QUERY_LIMIT => Err(format!(
            "file query limit must not exceed {MAX_FILE_QUERY_LIMIT}"
        )),
        value => Ok(value),
    }
}

fn normalize_optional_text(value: Option<String>) -> Result<Option<String>, String> {
    value
        .map(|value| {
            let value = value.trim().to_owned();
            if value.is_empty() {
                Err("optional file query filter must not be empty".to_owned())
            } else {
                Ok(value)
            }
        })
        .transpose()
}

fn current_time_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| {
            u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
        })
}

fn elapsed_ms(started: Instant) -> u64 {
    u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX)
}

fn query_timeout_ms(timeout: std::time::Duration) -> u64 {
    u64::try_from(timeout.as_millis()).unwrap_or(u64::MAX)
}

fn storage_error_timed_out(error: &StorageError) -> bool {
    matches!(
        error,
        StorageError::InvalidInput(message)
            if message.contains("file query timed out")
                || message.contains("file content query timed out")
    )
}

fn file_freshness_state_label(state: FileIndexFreshnessState) -> &'static str {
    match state {
        FileIndexFreshnessState::Fresh => "fresh",
        FileIndexFreshnessState::Pending => "pending",
        FileIndexFreshnessState::Paused => "paused",
        FileIndexFreshnessState::Stale => "stale",
        FileIndexFreshnessState::Degraded => "degraded",
        FileIndexFreshnessState::Overflow => "overflow",
    }
}

fn storage_api_error(error: StorageError) -> ApiError {
    ApiError::storage_unavailable(error.to_string())
}

#[cfg(test)]
#[path = "workflow_tests.rs"]
mod workflow_tests;
