use std::{
    collections::{BTreeSet, VecDeque},
    path::Path,
    sync::{Arc, OnceLock},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use tokio::sync::{Semaphore, oneshot};

use crate::{
    application::FileIndexRootConfig,
    domain::GraphVersion,
    storage::{
        FileIndexEntry, FileIndexRoot, FileIndexRootUpdate, FileIndexScanSummary, StorageError,
    },
};

use super::content::{
    FileContentEntryResult, MAX_CONTENT_INDEX_BYTES, file_content_entry,
    reserve_content_read_with_budget, text_content_extension,
};

const MAX_CONCURRENT_FILE_SCANS: usize = 4;
const MAX_CONTENT_SCAN_BYTES: usize = 64 * 1024 * 1024;
static FILE_SCAN_LIMITER: OnceLock<Arc<Semaphore>> = OnceLock::new();

#[derive(Clone)]
pub(super) struct ScanBudget {
    pub(super) max_depth: usize,
    pub(super) max_file_bytes: u64,
    pub(super) max_files_per_root: usize,
    pub(super) excludes: Vec<String>,
}

pub(super) async fn scan_roots(
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

pub(super) async fn scan_root_with_timeout(
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

pub(super) fn scan_worker_busy_file_index_root_update(
    root: FileIndexRootConfig,
    now_ms: u64,
) -> FileIndexRootUpdate {
    FileIndexRootUpdate {
        root: storage_root(root.scope_id, root.root_id, &root.root_path),
        entries: Vec::new(),
        processed_content_paths: BTreeSet::new(),
        content_entries: Vec::new(),
        scan_error_count: 1,
        truncated: true,
        content_truncated: false,
        content_read_error_count: 0,
        last_error: Some("file index scan worker is still busy".to_owned()),
        now_ms,
    }
}

fn timed_out_file_index_root_update(root: FileIndexRootConfig, now_ms: u64) -> FileIndexRootUpdate {
    FileIndexRootUpdate {
        root: storage_root(root.scope_id, root.root_id, &root.root_path),
        entries: Vec::new(),
        processed_content_paths: BTreeSet::new(),
        content_entries: Vec::new(),
        scan_error_count: 1,
        truncated: true,
        content_truncated: false,
        content_read_error_count: 0,
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
    let mut processed_content_paths = BTreeSet::new();
    let mut content_entries = Vec::new();
    let mut content_scan_bytes = 0usize;
    let mut scan_error_count = 0usize;
    let mut truncated = false;
    let mut content_truncated = false;
    let mut content_read_error_count = 0usize;
    let mut last_error = None;
    let canonical_root = match std::fs::canonicalize(&root_path) {
        Ok(path) => path,
        Err(error) => {
            return Ok(FileIndexRootUpdate {
                root: storage_root(root.scope_id, root.root_id, &root_path),
                entries,
                processed_content_paths,
                content_entries,
                scan_error_count: 1,
                truncated: false,
                content_truncated: false,
                content_read_error_count: 0,
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
                let entry = file_entry(
                    &root.scope_id,
                    &root.root_id,
                    &canonical_root,
                    &path,
                    &metadata,
                );
                if text_content_extension(entry.extension.as_deref()) {
                    if metadata.len() > MAX_CONTENT_INDEX_BYTES {
                        processed_content_paths.insert(entry.path.clone());
                    } else if content_scan_bytes < MAX_CONTENT_SCAN_BYTES {
                        if reserve_content_read_with_budget(
                            &mut content_scan_bytes,
                            metadata.len(),
                            MAX_CONTENT_SCAN_BYTES,
                        ) {
                            content_truncated = true;
                        } else {
                            match file_content_entry(
                                &entry,
                                &metadata,
                                &canonical_root,
                                now_ms,
                                GraphVersion::ZERO.get(),
                            ) {
                                FileContentEntryResult::Indexed(content_entry) => {
                                    processed_content_paths.insert(entry.path.clone());
                                    content_entries.push(*content_entry);
                                }
                                FileContentEntryResult::Skipped => {
                                    processed_content_paths.insert(entry.path.clone());
                                }
                                FileContentEntryResult::ReadFailed => {
                                    content_read_error_count =
                                        content_read_error_count.saturating_add(1);
                                    last_error.get_or_insert_with(|| {
                                        "file content read failed".to_owned()
                                    });
                                }
                            }
                        }
                    } else if content_scan_bytes >= MAX_CONTENT_SCAN_BYTES {
                        content_truncated = true;
                    }
                }
                entries.push(entry);
            }
        }
    }

    Ok(FileIndexRootUpdate {
        root: storage_root(root.scope_id, root.root_id, &canonical_root),
        entries,
        processed_content_paths,
        content_entries,
        scan_error_count,
        truncated,
        content_truncated,
        content_read_error_count,
        last_error,
        now_ms,
    })
}

pub(super) fn file_index_root_from_config(root: &FileIndexRootConfig) -> FileIndexRoot {
    FileIndexRoot {
        scope_id: root.scope_id.clone(),
        root_id: root.root_id.clone(),
        root_path: root.root_path.to_string_lossy().to_string(),
    }
}

pub(super) fn summary_from_diagnostics(
    diagnostics: crate::storage::FileIndexDiagnostics,
) -> FileIndexScanSummary {
    FileIndexScanSummary {
        root_count: diagnostics.root_count,
        indexed_file_count: diagnostics.indexed_file_count,
        missing_file_count: diagnostics.missing_file_count,
        indexed_content_count: diagnostics.indexed_content_count,
        skipped_content_count: diagnostics.skipped_content_count,
        unchanged_content_count: diagnostics.unchanged_content_count,
        stale_content_cursor_count: diagnostics.stale_content_cursor_count,
        scan_error_count: diagnostics.scan_error_count,
        content_read_error_count: diagnostics.content_read_error_count,
        truncated_root_count: diagnostics.truncated_root_count,
        roots: diagnostics.roots,
    }
}

pub(super) fn file_entry(
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
fn system_time_millis(time: SystemTime) -> Option<u64> {
    time.duration_since(UNIX_EPOCH)
        .ok()
        .map(|duration| u64::try_from(duration.as_millis()).unwrap_or(u64::MAX))
}

#[cfg(test)]
#[path = "mod_tests.rs"]
mod scanner_tests;
