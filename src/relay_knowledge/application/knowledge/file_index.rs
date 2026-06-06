use std::{
    collections::VecDeque,
    path::{Path, PathBuf},
    sync::{Arc, OnceLock},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use tokio::sync::{Semaphore, oneshot};

use crate::{
    api::{
        ApiError, ApiMetadata, FileIndexFreshnessState, FileIndexRequest, FileIndexResponse,
        FileQueryRequest, FileQueryResponse, RequestContext,
    },
    domain::{FreshnessPolicy, GraphVersion},
    storage::{
        FileIndexEntry, FileIndexRoot, FileIndexRootUpdate, FileIndexScanSummary,
        FileSearchRequest, StorageError,
    },
};

use crate::application::{FileIndexRootConfig, service::RelayKnowledgeService};

use super::file_freshness::{FileFreshnessContext, file_freshness_diagnostics};

pub const DEFAULT_FILE_QUERY_LIMIT: usize = 20;
const MAX_FILE_QUERY_LIMIT: usize = 500;
const MAX_CONCURRENT_FILE_SCANS: usize = 4;
static FILE_SCAN_LIMITER: OnceLock<Arc<Semaphore>> = OnceLock::new();

#[derive(Clone)]
struct ScanBudget {
    max_depth: usize,
    max_file_bytes: u64,
    max_files_per_root: usize,
    excludes: Vec<String>,
}

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
            summary.scan_error_count = summary
                .scan_error_count
                .saturating_add(status.scan_error_count);
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
                returned_hits: &[],
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
            returned_hits: &[],
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
                    returned_hits: &[],
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
        let freshness = file_freshness_diagnostics(FileFreshnessContext {
            file_index_enabled: self.runtime.file_index.enabled,
            configured_roots: &configured_roots,
            diagnostics: &diagnostics,
            freshness_policy: request.freshness_policy,
            source_scope: source_scope.clone(),
            root_id: root_id.clone(),
            graph_version: GraphVersion::ZERO.get(),
            query_degraded_reason: None,
            returned_hits: &results,
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

async fn scan_roots(
    roots: Vec<FileIndexRootConfig>,
    budget: ScanBudget,
    now_ms: u64,
    scan_timeout: Duration,
) -> Result<Vec<FileIndexRootUpdate>, StorageError> {
    let mut updates = Vec::with_capacity(roots.len());
    for root in roots {
        updates.push(scan_root_with_timeout(root, budget.clone(), now_ms, scan_timeout).await?);
    }

    Ok(updates)
}

async fn scan_root_with_timeout(
    root: FileIndexRootConfig,
    budget: ScanBudget,
    now_ms: u64,
    scan_timeout: Duration,
) -> Result<FileIndexRootUpdate, StorageError> {
    if scan_timeout.is_zero() {
        return Ok(timed_out_file_index_root_update(root, now_ms));
    }
    let permit = match file_scan_limiter().try_acquire_owned() {
        Ok(permit) => permit,
        Err(_) => return Ok(scan_worker_busy_file_index_root_update(root, now_ms)),
    };
    let timeout_root = root.clone();
    let (sender, receiver) = oneshot::channel();
    std::thread::Builder::new()
        .name("relay-file-index-scan".to_owned())
        .spawn(move || {
            let _permit = permit;
            let _ = sender.send(scan_root(root, &budget, now_ms));
        })?;

    match tokio::time::timeout(scan_timeout, receiver).await {
        Ok(Ok(result)) => result,
        Ok(Err(_)) => Err(StorageError::InvalidInput(
            "file index scan worker stopped before reporting".to_owned(),
        )),
        Err(_) => Ok(timed_out_file_index_root_update(timeout_root, now_ms)),
    }
}

fn file_scan_limiter() -> Arc<Semaphore> {
    Arc::clone(
        FILE_SCAN_LIMITER.get_or_init(|| Arc::new(Semaphore::new(MAX_CONCURRENT_FILE_SCANS))),
    )
}

fn scan_worker_busy_file_index_root_update(
    root: FileIndexRootConfig,
    now_ms: u64,
) -> FileIndexRootUpdate {
    FileIndexRootUpdate {
        root: storage_root(root.scope_id, root.root_id, &root.root_path),
        entries: Vec::new(),
        scan_error_count: 1,
        truncated: true,
        last_error: Some("file index scan worker is still busy".to_owned()),
        now_ms,
    }
}

fn timed_out_file_index_root_update(root: FileIndexRootConfig, now_ms: u64) -> FileIndexRootUpdate {
    FileIndexRootUpdate {
        root: storage_root(root.scope_id, root.root_id, &root.root_path),
        entries: Vec::new(),
        scan_error_count: 1,
        truncated: true,
        last_error: Some("file index scan timed out".to_owned()),
        now_ms,
    }
}

fn scan_root(
    root: FileIndexRootConfig,
    budget: &ScanBudget,
    now_ms: u64,
) -> Result<FileIndexRootUpdate, StorageError> {
    let root_path = root.root_path;
    let mut entries = Vec::new();
    let mut scan_error_count = 0usize;
    let mut truncated = false;
    let mut last_error = None;
    let canonical_root = match std::fs::canonicalize(&root_path) {
        Ok(path) => path,
        Err(error) => {
            return Ok(FileIndexRootUpdate {
                root: storage_root(root.scope_id, root.root_id, &root_path),
                entries,
                scan_error_count: 1,
                truncated: false,
                last_error: Some(error.to_string()),
                now_ms,
            });
        }
    };
    let mut pending = VecDeque::from([(canonical_root.clone(), 0usize)]);

    while let Some((directory, depth)) = pending.pop_front() {
        if entries.len() >= budget.max_files_per_root {
            truncated = true;
            break;
        }
        if depth > budget.max_depth {
            truncated = true;
            continue;
        }
        let read_dir = match std::fs::read_dir(&directory) {
            Ok(read_dir) => read_dir,
            Err(error) => {
                scan_error_count = scan_error_count.saturating_add(1);
                last_error = Some(error.to_string());
                continue;
            }
        };
        for child in read_dir {
            if entries.len() >= budget.max_files_per_root {
                truncated = true;
                pending.clear();
                break;
            }
            let child = match child {
                Ok(child) => child,
                Err(error) => {
                    scan_error_count = scan_error_count.saturating_add(1);
                    last_error = Some(error.to_string());
                    continue;
                }
            };
            let path = child.path();
            if excluded(&path, &budget.excludes) {
                continue;
            }
            let file_type = match child.file_type() {
                Ok(file_type) => file_type,
                Err(error) => {
                    scan_error_count = scan_error_count.saturating_add(1);
                    last_error = Some(error.to_string());
                    continue;
                }
            };
            if file_type.is_symlink() {
                continue;
            }
            if file_type.is_dir() {
                pending.push_back((path, depth.saturating_add(1)));
                continue;
            }
            let metadata = match child.metadata() {
                Ok(metadata) => metadata,
                Err(error) => {
                    scan_error_count = scan_error_count.saturating_add(1);
                    last_error = Some(error.to_string());
                    continue;
                }
            };
            if file_type.is_file() && metadata.len() <= budget.max_file_bytes {
                entries.push(file_entry(
                    &root.scope_id,
                    &root.root_id,
                    &canonical_root,
                    &path,
                    &metadata,
                ));
            }
        }
    }

    Ok(FileIndexRootUpdate {
        root: storage_root(root.scope_id, root.root_id, &canonical_root),
        entries,
        scan_error_count,
        truncated,
        last_error,
        now_ms,
    })
}

fn file_index_root_from_config(root: &FileIndexRootConfig) -> FileIndexRoot {
    FileIndexRoot {
        scope_id: root.scope_id.clone(),
        root_id: root.root_id.clone(),
        root_path: root.root_path.to_string_lossy().to_string(),
    }
}

fn summary_from_diagnostics(
    diagnostics: crate::storage::FileIndexDiagnostics,
) -> FileIndexScanSummary {
    FileIndexScanSummary {
        root_count: diagnostics.root_count,
        indexed_file_count: diagnostics.indexed_file_count,
        missing_file_count: diagnostics.missing_file_count,
        scan_error_count: diagnostics.scan_error_count,
        truncated_root_count: diagnostics.truncated_root_count,
        roots: diagnostics.roots,
    }
}

fn file_entry(
    scope_id: &str,
    root_id: &str,
    root: &Path,
    path: &Path,
    metadata: &std::fs::Metadata,
) -> FileIndexEntry {
    let relative_path = path.strip_prefix(root).unwrap_or(path);
    let file_name = path
        .file_name()
        .map(|value| value.to_string_lossy().to_string())
        .unwrap_or_default();
    let extension = path
        .extension()
        .map(|value| value.to_string_lossy().to_ascii_lowercase());
    let parent_dir = path
        .parent()
        .map(|value| value.to_string_lossy().to_string())
        .unwrap_or_default();
    let modified_at_ms = metadata
        .modified()
        .ok()
        .and_then(system_time_millis)
        .unwrap_or_default();

    FileIndexEntry {
        scope_id: scope_id.to_owned(),
        root_id: root_id.to_owned(),
        path: path.to_string_lossy().to_string(),
        relative_path: relative_path.to_string_lossy().to_string(),
        file_name,
        extension,
        parent_dir,
        size_bytes: metadata.len(),
        modified_at_ms,
        fingerprint: format!("{}:{modified_at_ms}", metadata.len()),
    }
}

fn storage_root(scope_id: String, root_id: String, root_path: &Path) -> FileIndexRoot {
    FileIndexRoot {
        scope_id,
        root_id,
        root_path: root_path.to_string_lossy().to_string(),
    }
}

fn excluded(path: &Path, configured: &[String]) -> bool {
    let Some(name) = path.file_name().map(|value| value.to_string_lossy()) else {
        return false;
    };
    if name.starts_with('.') {
        return true;
    }
    let lower = name.to_ascii_lowercase();
    matches!(
        lower.as_str(),
        "target" | "node_modules" | ".git" | "__pycache__" | "tmp" | "temp" | "cache"
    ) || configured
        .iter()
        .any(|pattern| lower.contains(&pattern.to_ascii_lowercase()))
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

fn system_time_millis(time: SystemTime) -> Option<u64> {
    time.duration_since(UNIX_EPOCH)
        .ok()
        .map(|duration| u64::try_from(duration.as_millis()).unwrap_or(u64::MAX))
}

fn elapsed_ms(started: Instant) -> u64 {
    u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX)
}

fn query_timeout_ms(timeout: std::time::Duration) -> u64 {
    u64::try_from(timeout.as_millis()).unwrap_or(u64::MAX)
}

fn storage_error_timed_out(error: &StorageError) -> bool {
    matches!(error, StorageError::InvalidInput(message) if message.contains("file query timed out"))
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
mod tests {
    use std::{
        fs,
        path::{Path, PathBuf},
        time::{SystemTime, UNIX_EPOCH},
    };

    use crate::{
        application::RuntimeConfiguration,
        env::{EnvironmentConfig, PlatformKind},
        storage::{KnowledgeStore, SqliteGraphStore},
    };

    use super::*;

    #[tokio::test]
    async fn scan_roots_respects_budget_excludes_and_metadata() {
        let fixture = TempFixture::new("scan-budget");
        fixture.write("docs/report.pdf", "pdf");
        fixture.write("target/generated.txt", "generated");
        fixture.write(".hidden/secret.txt", "secret");
        fixture.write("deep/a/b/c/too-deep.txt", "deep");
        fixture.write("large.bin", "too large for budget");
        fixture.write("notes/skipme.txt", "configured exclusion");
        #[cfg(unix)]
        std::os::unix::fs::symlink("/", fixture.path().join("escape"))
            .expect("symlink fixture should be created");

        let updates = scan_roots(
            vec![FileIndexRootConfig::new(
                "local-files",
                fixture.path().to_path_buf(),
            )],
            ScanBudget {
                max_depth: 2,
                max_file_bytes: 8,
                max_files_per_root: 10,
                excludes: vec!["skipme".to_owned()],
            },
            42,
            Duration::from_secs(30),
        )
        .await
        .expect("scan should complete");

        let update = updates.into_iter().next().expect("one root is scanned");
        assert_eq!(update.root.scope_id, "local-files");
        assert_eq!(update.now_ms, 42);
        assert!(update.truncated);
        assert_eq!(update.scan_error_count, 0);
        assert_eq!(update.entries.len(), 1);
        let entry = &update.entries[0];
        assert_eq!(entry.file_name, "report.pdf");
        assert_eq!(entry.extension.as_deref(), Some("pdf"));
        assert!(entry.relative_path.ends_with("docs/report.pdf"));
        assert!(entry.parent_dir.ends_with("docs"));
        assert_eq!(entry.size_bytes, 3);
        assert!(entry.fingerprint.starts_with("3:"));
    }

    #[tokio::test]
    async fn scan_roots_reports_missing_roots_and_file_count_truncation() {
        let fixture = TempFixture::new("scan-truncated");
        fixture.write("first.txt", "one");
        fixture.write("second.txt", "two");
        let missing = fixture.path().join("missing");

        let updates = scan_roots(
            vec![
                FileIndexRootConfig::new("local-files", fixture.path().to_path_buf()),
                FileIndexRootConfig::new("local-files", missing),
            ],
            ScanBudget {
                max_depth: 4,
                max_file_bytes: 128,
                max_files_per_root: 1,
                excludes: Vec::new(),
            },
            7,
            Duration::from_secs(30),
        )
        .await
        .expect("scan should complete");

        let truncated = updates
            .iter()
            .find(|update| update.root.root_path == fixture.path().to_string_lossy())
            .expect("fixture root should be present");
        assert!(truncated.truncated);
        assert_eq!(truncated.entries.len(), 1);

        let missing = updates
            .iter()
            .find(|update| update.root.root_path.ends_with("missing"))
            .expect("missing root should be reported");
        assert_eq!(missing.scan_error_count, 1);
        assert!(missing.entries.is_empty());
        assert!(missing.last_error.is_some());
    }

    #[tokio::test]
    async fn scan_timeout_returns_degraded_root_update() {
        let fixture = TempFixture::new("scan-timeout");

        let update = scan_root_with_timeout(
            FileIndexRootConfig::new("local-files", fixture.path().to_path_buf()),
            ScanBudget {
                max_depth: 4,
                max_file_bytes: 128,
                max_files_per_root: 1,
                excludes: Vec::new(),
            },
            9,
            Duration::ZERO,
        )
        .await
        .expect("timeout update should be produced");

        assert_eq!(update.scan_error_count, 1);
        assert!(update.truncated);
        assert!(update.entries.is_empty());
        assert_eq!(
            update.last_error.as_deref(),
            Some("file index scan timed out")
        );
    }

    #[test]
    fn scan_worker_busy_update_reports_bounded_backpressure() {
        let update = scan_worker_busy_file_index_root_update(
            FileIndexRootConfig::new("local-files", PathBuf::from("/opt/docs")),
            11,
        );

        assert_eq!(update.scan_error_count, 1);
        assert!(update.truncated);
        assert!(update.entries.is_empty());
        assert_eq!(
            update.last_error.as_deref(),
            Some("file index scan worker is still busy")
        );
        assert_eq!(update.now_ms, 11);
    }

    #[test]
    fn query_validation_helpers_reject_unbounded_inputs() {
        assert_eq!(required_query("  quarter  ".to_owned()).unwrap(), "quarter");
        assert!(required_query(" \t ".to_owned()).is_err());
        assert_eq!(bounded_limit(1).unwrap(), 1);
        assert!(bounded_limit(0).is_err());
        assert!(bounded_limit(MAX_FILE_QUERY_LIMIT + 1).is_err());
        assert_eq!(
            normalize_optional_text(Some(" root ".to_owned())).unwrap(),
            Some("root".to_owned())
        );
        assert!(normalize_optional_text(Some(" ".to_owned())).is_err());
        assert_eq!(normalize_optional_text(None).unwrap(), None);
    }

    #[test]
    fn query_timeout_helpers_map_runtime_budget_and_storage_errors() {
        assert_eq!(query_timeout_ms(std::time::Duration::from_millis(125)), 125);
        assert!(storage_error_timed_out(&StorageError::InvalidInput(
            "file query timed out waiting for storage lock".to_owned()
        )));
        assert!(!storage_error_timed_out(&StorageError::InvalidInput(
            "different validation failure".to_owned()
        )));
    }

    #[tokio::test]
    async fn explicit_roots_must_match_authorized_runtime_roots() {
        let fixture = TempFixture::new("authorized-roots");
        let service = service_for_root(fixture.path()).await;
        let authorized = service
            .file_index_roots_from_request(FileIndexRequest {
                source_scope: Some("local-files".to_owned()),
                roots: vec![fixture.path().join(".").to_string_lossy().to_string()],
            })
            .expect("configured root spelling should be authorized");
        assert_eq!(authorized.len(), 1);

        let denied = service
            .file_index_roots_from_request(FileIndexRequest {
                source_scope: Some("local-files".to_owned()),
                roots: vec![fixture.path().join("other").to_string_lossy().to_string()],
            })
            .expect_err("unconfigured root should be denied");
        assert!(denied.contains("is not configured"));

        let relative = service
            .file_index_roots_from_request(FileIndexRequest {
                source_scope: Some("local-files".to_owned()),
                roots: vec!["relative/docs".to_owned()],
            })
            .expect_err("relative roots should be denied");
        assert!(relative.contains("absolute path"));
    }

    #[tokio::test]
    async fn same_path_roots_remain_distinct_across_scopes() {
        let fixture = TempFixture::new("scope-roots");
        let home = fixture.path().join("home");
        let documents = home.join("Documents");
        fs::create_dir_all(&documents).expect("documents directory should be created");
        let environment = EnvironmentConfig::from_pairs(
            PlatformKind::Unix,
            [
                ("HOME", home.to_string_lossy().to_string()),
                ("TMPDIR", "/tmp".to_owned()),
                (
                    "RELAY_KNOWLEDGE_FILE_INDEX_ROOTS",
                    documents.to_string_lossy().to_string(),
                ),
                (
                    "RELAY_KNOWLEDGE_FILE_INDEX_SCAN_TIMEOUT_MS",
                    "120000".to_owned(),
                ),
            ],
        )
        .expect("environment should parse");
        let runtime = RuntimeConfiguration::from_environment(&environment)
            .await
            .expect("runtime should compose");
        assert_eq!(runtime.file_index.scan_timeout, Duration::from_secs(120));

        let matching_roots = runtime
            .file_index
            .roots
            .iter()
            .filter(|root| root.root_path.as_path() == documents.as_path())
            .collect::<Vec<_>>();
        assert_eq!(matching_roots.len(), 2);
        assert_ne!(matching_roots[0].scope_id, matching_roots[1].scope_id);
    }

    struct TempFixture {
        root: PathBuf,
    }

    impl TempFixture {
        fn new(name: &str) -> Self {
            let suffix = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("time should be valid")
                .as_nanos();
            let root = std::env::temp_dir().join(format!(
                "relay-knowledge-{name}-{}-{suffix}",
                std::process::id()
            ));
            fs::create_dir_all(&root).expect("fixture root should be created");

            Self { root }
        }

        fn path(&self) -> &Path {
            &self.root
        }

        fn write(&self, relative: &str, content: &str) {
            let path = self.root.join(relative);
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).expect("fixture parent should be created");
            }
            fs::write(path, content).expect("fixture file should be written");
        }
    }

    impl Drop for TempFixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    async fn service_for_root(root: &Path) -> RelayKnowledgeService {
        let home = root.join("home");
        fs::create_dir_all(&home).expect("home should be created");
        let relay_home = root.join("relay");
        let environment = EnvironmentConfig::from_pairs(
            PlatformKind::Unix,
            [
                ("HOME", home.to_string_lossy().to_string()),
                ("TMPDIR", "/tmp".to_owned()),
                (
                    "RELAY_KNOWLEDGE_HOME",
                    relay_home.to_string_lossy().to_string(),
                ),
                (
                    "RELAY_KNOWLEDGE_FILE_INDEX_ROOTS",
                    root.to_string_lossy().to_string(),
                ),
            ],
        )
        .expect("environment should parse");
        let runtime = RuntimeConfiguration::from_environment(&environment)
            .await
            .expect("runtime should compose");
        let store = Arc::new(SqliteGraphStore::open_in_memory().expect("store should open"))
            as Arc<dyn KnowledgeStore>;

        RelayKnowledgeService::with_store(runtime, store)
    }
}
