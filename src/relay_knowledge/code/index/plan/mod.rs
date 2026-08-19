//! Plans bounded index batches, row budgets, and workspace metadata.

use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
    sync::atomic::{AtomicUsize, Ordering},
    thread,
};

use crate::domain::{
    CodeFileFingerprint, CodeIndexBatch, CodeIndexMode, CodeIndexResourceBudget,
    CodeIndexSession, CodeIndexSnapshot, CodeMonorepoWorkspace, CodeRepositoryRegistration,
    CodeRepositorySelector, CodeWorkspaceDetectionConfig, code_snapshot_scope_id,
};

use super::{
    CodeIndexError,
    changes::GitTreeEntry,
    identity, parse_indexed_file,
    scope::scoped_source_snapshot,
    snapshot::{SnapshotBuild, SnapshotScopeFilters, detect_workspaces_for_source_snapshot},
    source::{
        RepositorySourceKind, ensure_filesystem_blobs_match_content_hashes,
        ensure_filesystem_paths_match_content_hashes, filesystem_content_hashes_for_paths,
        filesystem_tree_hash_from_path_hashes, source_snapshot_batch_bytes,
    },
};

const GIT_BLOB_FETCH_GROUP: usize = CodeIndexResourceBudget::DEFAULT_MAX_FILES_PER_BATCH;
const MIN_PARALLEL_PARSE_FILES: usize = 12;
const MIN_PARALLEL_PARSE_BYTES: usize = 256 * 1024;
const TARGET_PARSE_FILES_PER_WORKER: usize = 16;
const TARGET_PARSE_BYTES_PER_WORKER: usize = 512 * 1024;

/// Blocking plan for a checkpointed full repository index.
#[derive(Debug, Clone)]
pub struct CodeIndexPlan {
    registration: CodeRepositoryRegistration,
    root: PathBuf,
    commit: String,
    tree_hash: String,
    source_scope: String,
    path_filters: Vec<String>,
    language_filters: Vec<String>,
    source_kind: RepositorySourceKind,
    filesystem_path_hashes: BTreeMap<String, String>,
    paths: Vec<GitTreeEntry>,
    workspaces: Vec<CodeMonorepoWorkspace>,
    cursor: usize,
    next_batch_index: usize,
    resource_budget: CodeIndexResourceBudget,
    /// When set, the plan yields pre-parsed incremental snapshot data
    /// instead of fetching and parsing git blobs.
    incremental: Option<IncrementalPlanData>,
}

/// Pre-parsed incremental snapshot data split into bounded batches.
#[derive(Debug, Clone)]
struct IncrementalPlanData {
    snapshot: CodeIndexSnapshot,
    cursor: usize,
}

impl CodeIndexPlan {
    /// Returns the durable session metadata that storage checkpoints.
    pub fn session(&self) -> CodeIndexSession {
        let (full_replace, base_resolved_commit_sha, total_path_count, changed_path_count, skipped_unchanged_count, deleted_paths, changed_paths, tombstones) =
            match &self.incremental {
                Some(data) => {
                    let snapshot = &data.snapshot;
                    let changed_paths = snapshot
                        .files
                        .iter()
                        .map(|file| file.path.clone())
                        .collect::<Vec<_>>();
                    (
                        false,
                        snapshot.base_resolved_commit_sha.clone(),
                        snapshot.files.len(),
                        snapshot.changed_path_count,
                        snapshot.skipped_unchanged_count,
                        snapshot.deleted_paths.clone(),
                        changed_paths,
                        snapshot.tombstones.clone(),
                    )
                }
                None => (
                    true,
                    None,
                    self.paths.len(),
                    self.paths.len(),
                    0,
                    Vec::new(),
                    Vec::new(),
                    Vec::new(),
                ),
            };
        CodeIndexSession {
            repository_id: self.registration.repository_id.clone(),
            source_scope: self.source_scope.clone(),
            base_resolved_commit_sha,
            resolved_commit_sha: self.commit.clone(),
            tree_hash: self.tree_hash.clone(),
            path_filters: self.path_filters.clone(),
            language_filters: self.language_filters.clone(),
            full_replace,
            total_path_count,
            changed_path_count,
            skipped_unchanged_count,
            deleted_paths,
            changed_paths,
            tombstones,
            workspaces: self.workspaces.clone(),
            resource_budget: self.resource_budget,
        }
    }

    /// Parses the next bounded file batch without retaining prior batches.
    pub fn parse_next_batch(mut self) -> Result<(Self, Option<CodeIndexBatch>), CodeIndexError> {
        if let Some(incremental) = &mut self.incremental {
            let snapshot = &incremental.snapshot;
            if incremental.cursor >= snapshot.files.len() {
                return Ok((self, None));
            }
            let batch_end = (incremental.cursor + self.resource_budget.max_files_per_batch)
                .min(snapshot.files.len());
            let batch_paths: BTreeSet<&str> = snapshot.files[incremental.cursor..batch_end]
                .iter()
                .map(|file| file.path.as_str())
                .collect();
            let parsed_byte_count: usize = snapshot.files[incremental.cursor..batch_end]
                .iter()
                .map(|file| file.byte_len)
                .sum();
            let batch = CodeIndexBatch {
                repository_id: snapshot.repository_id.clone(),
                source_scope: snapshot.source_scope.clone(),
                batch_index: self.next_batch_index,
                parsed_byte_count,
                files: snapshot.files[incremental.cursor..batch_end].to_vec(),
                symbols: snapshot
                    .symbols
                    .iter()
                    .filter(|record| batch_paths.contains(record.path.as_str()))
                    .cloned()
                    .collect(),
                references: snapshot
                    .references
                    .iter()
                    .filter(|record| batch_paths.contains(record.path.as_str()))
                    .cloned()
                    .collect(),
                imports: snapshot
                    .imports
                    .iter()
                    .filter(|record| batch_paths.contains(record.path.as_str()))
                    .cloned()
                    .collect(),
                dependencies: snapshot
                    .dependencies
                    .iter()
                    .filter(|record| batch_paths.contains(record.path.as_str()))
                    .cloned()
                    .collect(),
                feature_flags: snapshot
                    .feature_flags
                    .iter()
                    .filter(|record| batch_paths.contains(record.path.as_str()))
                    .cloned()
                    .collect(),
                routes: snapshot
                    .routes
                    .iter()
                    .filter(|record| batch_paths.contains(record.path.as_str()))
                    .cloned()
                    .collect(),
                chunks: snapshot
                    .chunks
                    .iter()
                    .filter(|record| batch_paths.contains(record.path.as_str()))
                    .cloned()
                    .collect(),
                diagnostics: snapshot
                    .diagnostics
                    .iter()
                    .filter(|record| batch_paths.contains(record.path.as_str()))
                    .cloned()
                    .collect(),
            };
            incremental.cursor = batch_end;
            self.next_batch_index += 1;
            return Ok((self, Some(batch)));
        }

        if self.cursor >= self.paths.len() {
            return Ok((self, None));
        }

        let mut build = SnapshotBuild::new_with_scope_filters(
            &self.registration,
            self.commit.clone(),
            self.tree_hash.clone(),
            SnapshotScopeFilters {
                path_filters: self.path_filters.clone(),
                language_filters: self.language_filters.clone(),
            },
            true,
            self.paths.len(),
            0,
        );
        let mut parsed_bytes = 0usize;
        while self.cursor < self.paths.len() {
            let fetch_end = next_fetch_end(&self, build.files.len(), parsed_bytes);
            if fetch_end == self.cursor {
                break;
            }
            let fetched_paths = self.paths[self.cursor..fetch_end]
                .iter()
                .map(|entry| entry.path.clone())
                .collect::<Vec<_>>();
            ensure_filesystem_paths_match_content_hashes(
                &self.root,
                &self.commit,
                &fetched_paths,
                &self.filesystem_path_hashes,
            )?;
            let blobs = source_snapshot_batch_bytes(
                &self.root,
                self.source_kind,
                &self.commit,
                &fetched_paths,
            )?;
            ensure_filesystem_blobs_match_content_hashes(
                &self.commit,
                &fetched_paths,
                &blobs,
                &self.filesystem_path_hashes,
            )?;
            let parsed_files = parse_fetched_files(&self, &fetched_paths, &blobs)?;
            for (bytes, parsed_file) in blobs.iter().zip(parsed_files) {
                parsed_bytes = parsed_bytes.saturating_add(bytes.len());
                build.append_file_records(parsed_file);
                self.cursor += 1;

                if !build.files.is_empty()
                    && (build.files.len() >= self.resource_budget.max_files_per_batch
                        || parsed_bytes >= self.resource_budget.max_bytes_per_batch
                        || batch_row_count(&build) >= self.resource_budget.max_rows_per_batch)
                {
                    break;
                }
            }
            if !build.files.is_empty()
                && (build.files.len() >= self.resource_budget.max_files_per_batch
                    || parsed_bytes >= self.resource_budget.max_bytes_per_batch
                    || batch_row_count(&build) >= self.resource_budget.max_rows_per_batch)
            {
                break;
            }
        }
        identity::enrich_symbol_identities(&build.repository_id, &mut build.symbols);

        let batch = CodeIndexBatch {
            repository_id: build.repository_id,
            source_scope: build.source_scope,
            batch_index: self.next_batch_index,
            parsed_byte_count: parsed_bytes,
            files: build.files,
            symbols: build.symbols,
            references: build.references,
            imports: build.imports,
            dependencies: build.dependencies,
            feature_flags: build.feature_flags,
            routes: build.routes,
            chunks: build.chunks,
            diagnostics: build.diagnostics,
        };
        self.next_batch_index += 1;

        Ok((self, Some(batch)))
    }
}

fn parse_fetched_files(
    plan: &CodeIndexPlan,
    paths: &[String],
    blobs: &[Vec<u8>],
) -> Result<Vec<SnapshotBuild>, CodeIndexError> {
    let worker_count = worker_count(paths.len(), total_blob_bytes(blobs));
    if paths.len() <= 1 || worker_count <= 1 {
        return paths
            .iter()
            .zip(blobs.iter())
            .map(|(path, bytes)| parse_one_file(plan, path, bytes))
            .collect();
    }

    let next_index = AtomicUsize::new(0);
    let mut parsed = thread::scope(|scope| {
        let handles = (0..worker_count)
            .map(|_| {
                let next_index = &next_index;
                scope
                    .spawn(move || parse_worker_queue(plan, paths, blobs, next_index, worker_count))
            })
            .collect::<Vec<_>>();
        let mut parsed = Vec::with_capacity(paths.len());
        for handle in handles {
            let worker_output = handle.join().map_err(|_| {
                CodeIndexError::InvalidInput("code parser worker panicked".to_owned())
            })??;
            parsed.extend(worker_output);
        }

        Ok::<_, CodeIndexError>(parsed)
    })?;
    parsed.sort_by_key(|(index, _)| *index);

    Ok(parsed.into_iter().map(|(_, build)| build).collect())
}

fn parse_one_file(
    plan: &CodeIndexPlan,
    path: &str,
    bytes: &[u8],
) -> Result<SnapshotBuild, CodeIndexError> {
    let mut build = SnapshotBuild::new_with_scope_filters(
        &plan.registration,
        plan.commit.clone(),
        plan.tree_hash.clone(),
        SnapshotScopeFilters {
            path_filters: plan.path_filters.clone(),
            language_filters: plan.language_filters.clone(),
        },
        true,
        plan.paths.len(),
        0,
    );
    parse_indexed_file(&mut build, path, bytes)?;

    Ok(build)
}

fn parse_worker_queue(
    plan: &CodeIndexPlan,
    paths: &[String],
    blobs: &[Vec<u8>],
    next_index: &AtomicUsize,
    worker_count: usize,
) -> Result<Vec<(usize, SnapshotBuild)>, CodeIndexError> {
    let mut parsed = Vec::with_capacity(paths.len().div_ceil(worker_count));
    loop {
        let index = next_index.fetch_add(1, Ordering::Relaxed);
        if index >= paths.len() {
            break;
        }
        parsed.push((index, parse_one_file(plan, &paths[index], &blobs[index])?));
    }

    Ok(parsed)
}

fn total_blob_bytes(blobs: &[Vec<u8>]) -> usize {
    blobs
        .iter()
        .fold(0usize, |total, blob| total.saturating_add(blob.len()))
}

fn worker_count(item_count: usize, total_bytes: usize) -> usize {
    if item_count == 0 {
        return 0;
    }
    if item_count < MIN_PARALLEL_PARSE_FILES && total_bytes < MIN_PARALLEL_PARSE_BYTES {
        return 1;
    }
    let desired_workers = item_count
        .div_ceil(TARGET_PARSE_FILES_PER_WORKER)
        .max(total_bytes.div_ceil(TARGET_PARSE_BYTES_PER_WORKER))
        .max(1);

    thread::available_parallelism()
        .map(usize::from)
        .unwrap_or(1)
        .min(item_count)
        .min(desired_workers)
}

/// Prepares a full repository index as a bounded, checkpointable batch plan.
pub fn prepare_full_index_plan(
    registration: CodeRepositoryRegistration,
    selector: CodeRepositorySelector,
    resource_budget: CodeIndexResourceBudget,
) -> Result<CodeIndexPlan, CodeIndexError> {
    prepare_full_index_plan_with_workspace_detection(
        registration,
        selector,
        resource_budget,
        &CodeWorkspaceDetectionConfig::default(),
    )
}

/// Prepares a full repository index plan with caller-controlled workspace
/// detection metadata for finalization.
pub fn prepare_full_index_plan_with_workspace_detection(
    registration: CodeRepositoryRegistration,
    selector: CodeRepositorySelector,
    resource_budget: CodeIndexResourceBudget,
    workspace_detection: &CodeWorkspaceDetectionConfig,
) -> Result<CodeIndexPlan, CodeIndexError> {
    let root = PathBuf::from(&registration.root_path);
    let snapshot = scoped_source_snapshot(&registration, &selector, &root, &selector.ref_selector)?;
    let filesystem_path_hashes = filesystem_plan_path_hashes(&snapshot)?;
    let source_scope = code_snapshot_scope_id(
        &registration.repository_id,
        &snapshot.tree_hash,
        &snapshot.path_filters,
        &snapshot.language_filters,
    );
    let workspaces = detect_workspaces_for_source_snapshot(
        &snapshot.root,
        snapshot.kind,
        &snapshot.resolved_commit_sha,
        &snapshot.entries,
        &snapshot.path_filters,
        workspace_detection,
    );

    Ok(CodeIndexPlan {
        registration,
        root: snapshot.root,
        commit: snapshot.resolved_commit_sha,
        tree_hash: snapshot.tree_hash,
        source_scope,
        path_filters: snapshot.path_filters,
        language_filters: snapshot.language_filters,
        source_kind: snapshot.kind,
        filesystem_path_hashes,
        paths: snapshot.entries,
        workspaces,
        cursor: 0,
        next_batch_index: 1,
        resource_budget,
        incremental: None,
    })
}

/// Prepares an incremental index plan from a pre-parsed diff snapshot.
///
/// The plan yields bounded batches from the snapshot's already-parsed records
/// instead of fetching and parsing git blobs. The session carries
/// `full_replace: false` and `changed_paths` so `begin_session_once` can
/// clone the historical scope while excluding changed paths.
pub fn prepare_incremental_index_plan_with_workspace_detection(
    registration: CodeRepositoryRegistration,
    selector: CodeRepositorySelector,
    mode: CodeIndexMode,
    previous_hashes: Vec<CodeFileFingerprint>,
    base_resolved_commit_sha: Option<String>,
    workspace_detection: &CodeWorkspaceDetectionConfig,
    resource_budget: CodeIndexResourceBudget,
) -> Result<CodeIndexPlan, CodeIndexError> {
    let snapshot = super::build_index_snapshot_with_workspace_detection(
        &registration,
        &selector,
        mode,
        previous_hashes,
        base_resolved_commit_sha,
        workspace_detection,
    )?;
    let root = PathBuf::from(&registration.root_path);
    let workspaces = snapshot.workspaces.clone();
    Ok(CodeIndexPlan {
        registration,
        root,
        commit: snapshot.resolved_commit_sha.clone(),
        tree_hash: snapshot.tree_hash.clone(),
        source_scope: snapshot.source_scope.clone(),
        path_filters: snapshot.path_filters.clone(),
        language_filters: snapshot.language_filters.clone(),
        source_kind: RepositorySourceKind::Git,
        filesystem_path_hashes: BTreeMap::new(),
        paths: Vec::new(),
        workspaces,
        cursor: 0,
        next_batch_index: 1,
        resource_budget,
        incremental: Some(IncrementalPlanData {
            snapshot,
            cursor: 0,
        }),
    })
}

fn filesystem_plan_path_hashes(
    snapshot: &super::scope::ScopedSourceSnapshot,
) -> Result<BTreeMap<String, String>, CodeIndexError> {
    if !snapshot.kind.is_filesystem() {
        return Ok(BTreeMap::new());
    }
    let paths = snapshot
        .entries
        .iter()
        .map(|entry| entry.path.clone())
        .collect::<Vec<_>>();
    let path_hashes = filesystem_content_hashes_for_paths(&snapshot.root, &paths)?;
    let tree_hash = filesystem_tree_hash_from_path_hashes(&path_hashes);
    if tree_hash != snapshot.tree_hash {
        return Err(CodeIndexError::InvalidInput(format!(
            "filesystem source snapshot {} no longer matches planned filesystem content {tree_hash}",
            snapshot.tree_hash
        )));
    }

    Ok(path_hashes)
}

fn next_fetch_end(plan: &CodeIndexPlan, batch_file_count: usize, parsed_bytes: usize) -> usize {
    let remaining_files = plan
        .resource_budget
        .max_files_per_batch
        .saturating_sub(batch_file_count)
        .max(1);
    let file_limited_end = plan.paths.len().min(
        plan.cursor
            .saturating_add(GIT_BLOB_FETCH_GROUP.min(remaining_files)),
    );
    let remaining_bytes = plan
        .resource_budget
        .max_bytes_per_batch
        .saturating_sub(parsed_bytes);
    let mut byte_count = 0usize;
    let mut end = plan.cursor;
    while end < file_limited_end {
        let entry_bytes = plan.paths[end].byte_count;
        if end > plan.cursor && byte_count.saturating_add(entry_bytes) > remaining_bytes {
            break;
        }
        byte_count = byte_count.saturating_add(entry_bytes);
        end += 1;
    }

    if end == plan.cursor && batch_file_count == 0 {
        return (plan.cursor + 1).min(plan.paths.len());
    }

    end
}

fn batch_row_count(build: &SnapshotBuild) -> usize {
    build
        .files
        .len()
        .saturating_add(build.symbols.len())
        .saturating_add(build.references.len())
        .saturating_add(build.imports.len())
        .saturating_add(build.dependencies.len())
        .saturating_add(build.feature_flags.len())
        .saturating_add(build.routes.len())
        .saturating_add(build.chunks.len())
        .saturating_add(build.diagnostics.len())
}

#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;
