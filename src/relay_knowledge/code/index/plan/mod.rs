//! Plans bounded index batches, row budgets, and workspace metadata.

use std::{
    collections::{BTreeMap, VecDeque},
    path::PathBuf,
    sync::atomic::{AtomicUsize, Ordering},
    thread,
};

use crate::domain::{
    CodeIndexBatch, CodeIndexCheckpoint, CodeIndexResourceBudget, CodeIndexSession,
    CodeMonorepoWorkspace, CodeRepositoryRegistration, CodeRepositorySelector,
    CodeWorkspaceDetectionConfig, code_query_index_repair, code_query_index_subphase,
    code_reference_resolution, code_reference_resolution_query_index_repair,
    code_reference_search_query_index_repair, code_reference_search_rebuild,
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

#[derive(Debug, Clone)]
struct PendingParsedFile {
    parsed_byte_count: usize,
    build: SnapshotBuild,
}

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
    parsed_overflow: VecDeque<PendingParsedFile>,
    next_batch_index: usize,
    resource_budget: CodeIndexResourceBudget,
}

#[derive(Debug)]
pub(crate) enum CodeIndexPlanRecovery {
    Resume(CodeIndexPlan),
    ContentEquivalentRestart(CodeIndexPlan),
}

impl CodeIndexPlan {
    /// Returns the durable session metadata that storage checkpoints.
    pub fn session(&self) -> CodeIndexSession {
        CodeIndexSession {
            repository_id: self.registration.repository_id.clone(),
            source_scope: self.source_scope.clone(),
            base_resolved_commit_sha: None,
            resolved_commit_sha: self.commit.clone(),
            tree_hash: self.tree_hash.clone(),
            path_filters: self.path_filters.clone(),
            language_filters: self.language_filters.clone(),
            full_replace: true,
            total_path_count: self.paths.len(),
            changed_path_count: self.paths.len(),
            skipped_unchanged_count: 0,
            deleted_paths: Vec::new(),
            changed_paths: Vec::new(),
            tombstones: Vec::new(),
            workspaces: self.workspaces.clone(),
            resource_budget: self.resource_budget,
        }
    }

    /// Restores the parser cursor from a fully committed durable checkpoint.
    /// Fresh plans still begin at zero; this path is only valid before any
    /// in-memory parse work has started.
    pub fn resume_from_checkpoint(
        self,
        checkpoint: &CodeIndexCheckpoint,
    ) -> Result<Self, CodeIndexError> {
        match self.recover_from_checkpoint(checkpoint)? {
            CodeIndexPlanRecovery::Resume(plan) => Ok(plan),
            CodeIndexPlanRecovery::ContentEquivalentRestart(_) => Err(invalid_checkpoint(
                "a completed content-equivalent checkpoint with a different resolved commit must restart instead of resuming its cursor",
            )),
        }
    }

    pub(crate) fn recover_from_checkpoint(
        mut self,
        checkpoint: &CodeIndexCheckpoint,
    ) -> Result<CodeIndexPlanRecovery, CodeIndexError> {
        self.validate_pristine_resume_target()?;
        self.validate_checkpoint_content_identity(checkpoint)?;
        self.validate_checkpoint_progress(checkpoint)?;

        if checkpoint.resolved_commit_sha != self.commit {
            if checkpoint.state == "completed" {
                return Ok(CodeIndexPlanRecovery::ContentEquivalentRestart(self));
            }
            return Err(invalid_checkpoint(
                "resolved commit does not match the plan and only a completed content-equivalent checkpoint may restart",
            ));
        }

        self.cursor = checkpoint.committed_file_count;
        self.next_batch_index = checkpoint.batch_count.checked_add(1).ok_or_else(|| {
            invalid_checkpoint("batch count cannot advance to the next batch index")
        })?;

        Ok(CodeIndexPlanRecovery::Resume(self))
    }

    pub(crate) fn resume_from_content_equivalent_restart_checkpoint(
        self,
        checkpoint: &CodeIndexCheckpoint,
    ) -> Result<Self, CodeIndexError> {
        if checkpoint.state != "indexing"
            || checkpoint.parsed_file_count != 0
            || checkpoint.committed_file_count != 0
            || checkpoint.committed_symbol_count != 0
            || checkpoint.committed_reference_count != 0
            || checkpoint.committed_chunk_count != 0
            || checkpoint.batch_count != 0
            || checkpoint.last_path.is_some()
        {
            return Err(invalid_checkpoint(
                "a content-equivalent restart must return a zero-progress indexing checkpoint",
            ));
        }
        self.resume_from_checkpoint(checkpoint)
    }

    fn validate_pristine_resume_target(&self) -> Result<(), CodeIndexError> {
        if !self.parsed_overflow.is_empty() {
            return Err(invalid_checkpoint(
                "uncommitted parsed overflow must be empty before durable resume",
            ));
        }
        if self.cursor != 0 || self.next_batch_index != 1 {
            return Err(invalid_checkpoint(
                "resume requires a newly prepared plan before any parse batch",
            ));
        }

        Ok(())
    }

    fn validate_checkpoint_content_identity(
        &self,
        checkpoint: &CodeIndexCheckpoint,
    ) -> Result<(), CodeIndexError> {
        let identity_matches = checkpoint.repository_id == self.registration.repository_id
            && checkpoint.source_scope == self.source_scope
            && checkpoint.tree_hash == self.tree_hash
            && checkpoint.path_filters == self.path_filters
            && checkpoint.language_filters == self.language_filters
            && checkpoint.total_path_count == self.paths.len();
        if !identity_matches {
            return Err(invalid_checkpoint(
                "repository, scope, source, filters, or path count does not match the plan",
            ));
        }
        if checkpoint.resource_budget != self.resource_budget {
            return Err(invalid_checkpoint(
                "resource budget does not match the plan that will resume",
            ));
        }

        Ok(())
    }

    fn validate_checkpoint_progress(
        &self,
        checkpoint: &CodeIndexCheckpoint,
    ) -> Result<(), CodeIndexError> {
        let requires_complete_prefix =
            checkpoint_state_requires_complete_prefix(checkpoint.state.as_str())
                .ok_or_else(|| invalid_checkpoint("state is not resumable"))?;
        if checkpoint.parsed_file_count != checkpoint.committed_file_count {
            return Err(invalid_checkpoint(
                "parsed and committed file counts must be equal",
            ));
        }
        let committed = checkpoint.committed_file_count;
        if committed > self.paths.len() {
            return Err(invalid_checkpoint(
                "committed file count exceeds the planned path count",
            ));
        }
        if requires_complete_prefix && committed != self.paths.len() {
            return Err(invalid_checkpoint(
                "finalizing or completed state requires every planned path to be committed",
            ));
        }
        if committed == 0 {
            if checkpoint.batch_count != 0 || checkpoint.last_path.is_some() {
                return Err(invalid_checkpoint(
                    "an empty committed prefix requires zero batches and no last path",
                ));
            }
            return Ok(());
        }
        if checkpoint.batch_count == 0 || checkpoint.batch_count > committed {
            return Err(invalid_checkpoint(
                "batch count must describe one or more bounded committed batches",
            ));
        }
        let expected_last_path = self.paths[committed - 1].path.as_str();
        if checkpoint.last_path.as_deref() != Some(expected_last_path) {
            return Err(invalid_checkpoint(
                "last path does not identify the committed plan prefix",
            ));
        }

        Ok(())
    }

    /// Parses the next bounded file batch while retaining at most one fetched
    /// group's uncommitted row-budget overflow.
    pub fn parse_next_batch(mut self) -> Result<(Self, Option<CodeIndexBatch>), CodeIndexError> {
        if self.cursor >= self.paths.len() && self.parsed_overflow.is_empty() {
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
        build.bind_verified_source_scope(&self.source_scope)?;
        let mut parsed_bytes = 0usize;
        loop {
            self.append_parsed_overflow(&mut build, &mut parsed_bytes);
            if batch_budget_reached(&build, parsed_bytes, self.resource_budget) {
                break;
            }
            if !self.fetch_and_parse_next_group(build.files.len(), parsed_bytes)? {
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

    fn append_parsed_overflow(&mut self, build: &mut SnapshotBuild, parsed_bytes: &mut usize) {
        while let Some(parsed_file) = self.parsed_overflow.pop_front() {
            *parsed_bytes = (*parsed_bytes).saturating_add(parsed_file.parsed_byte_count);
            build.append_file_records(parsed_file.build);
            if batch_budget_reached(build, *parsed_bytes, self.resource_budget) {
                break;
            }
        }
    }

    fn fetch_and_parse_next_group(
        &mut self,
        batch_file_count: usize,
        parsed_bytes: usize,
    ) -> Result<bool, CodeIndexError> {
        debug_assert!(self.parsed_overflow.is_empty());
        if self.cursor >= self.paths.len() {
            return Ok(false);
        }
        let fetch_end = next_fetch_end(self, batch_file_count, parsed_bytes);
        if fetch_end == self.cursor {
            return Ok(false);
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
        if blobs.len() != fetched_paths.len() {
            return Err(CodeIndexError::InvalidInput(format!(
                "source batch returned {} blobs for {} paths",
                blobs.len(),
                fetched_paths.len()
            )));
        }
        ensure_filesystem_blobs_match_content_hashes(
            &self.commit,
            &fetched_paths,
            &blobs,
            &self.filesystem_path_hashes,
        )?;
        let parsed_files = parse_fetched_files(self, &fetched_paths, &blobs)?;
        if parsed_files.len() != fetched_paths.len() {
            return Err(CodeIndexError::InvalidInput(format!(
                "parser batch returned {} files for {} paths",
                parsed_files.len(),
                fetched_paths.len()
            )));
        }
        self.parsed_overflow
            .extend(
                blobs
                    .iter()
                    .zip(parsed_files)
                    .map(|(bytes, build)| PendingParsedFile {
                        parsed_byte_count: bytes.len(),
                        build,
                    }),
            );
        self.cursor = fetch_end;

        Ok(true)
    }
}

fn checkpoint_state_requires_complete_prefix(state: &str) -> Option<bool> {
    if code_query_index_subphase(state).is_some()
        || code_query_index_repair(state).is_some()
        || code_reference_resolution(state).is_some()
        || code_reference_resolution_query_index_repair(state).is_some()
        || code_reference_search_query_index_repair(state).is_some()
        || code_reference_search_rebuild(state).is_some()
    {
        return Some(true);
    }
    match state {
        "indexing" => Some(false),
        "finalizing:build_query_indexes"
        | "finalizing:resolve_references"
        | "finalizing:resolve_imports"
        | "finalizing:resolve_call_targets"
        | "finalizing:refresh_dependencies"
        | "finalizing:rebuild_reference_search"
        | "finalizing:rebuild_calls"
        | "finalizing:publish_scope"
        | "finalizing:resolve_workspace_imports"
        | "finalizing:software_projection"
        | "finalizing:partitioned_publish"
        | "completed" => Some(true),
        _ => None,
    }
}

fn invalid_checkpoint(message: &str) -> CodeIndexError {
    CodeIndexError::Invariant(format!("invalid code index resume checkpoint: {message}"))
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
    build.bind_verified_source_scope(&plan.source_scope)?;
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
    let source_scope = crate::domain::code_snapshot_scope_id_with_workspace_detection(
        &registration.repository_id,
        &snapshot.tree_hash,
        &snapshot.path_filters,
        &snapshot.language_filters,
        workspace_detection,
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
        parsed_overflow: VecDeque::new(),
        next_batch_index: 1,
        resource_budget,
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

fn batch_budget_reached(
    build: &SnapshotBuild,
    parsed_bytes: usize,
    resource_budget: CodeIndexResourceBudget,
) -> bool {
    !build.files.is_empty()
        && (build.files.len() >= resource_budget.max_files_per_batch
            || parsed_bytes >= resource_budget.max_bytes_per_batch
            || batch_row_count(build) >= resource_budget.max_rows_per_batch)
}

#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;
