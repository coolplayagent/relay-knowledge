use std::{path::PathBuf, thread};

use crate::domain::{
    CodeIndexBatch, CodeIndexResourceBudget, CodeIndexSession, CodeRepositoryRegistration,
    CodeRepositorySelector, code_snapshot_scope_id,
};

use super::{
    CodeIndexError,
    changes::{GitTreeEntry, tracked_entries},
    git::{git_batch_blobs, resolve_ref, resolve_tree},
    identity, parse_indexed_file,
    scope::{load_ignore_rules_from_commit, selection_exclusion_reason},
    snapshot::SnapshotBuild,
};

const GIT_BLOB_FETCH_GROUP: usize = CodeIndexResourceBudget::DEFAULT_MAX_FILES_PER_BATCH;

/// Blocking plan for a checkpointed full repository index.
#[derive(Debug, Clone)]
pub struct CodeIndexPlan {
    registration: CodeRepositoryRegistration,
    selector: CodeRepositorySelector,
    root: PathBuf,
    commit: String,
    tree_hash: String,
    source_scope: String,
    path_filters: Vec<String>,
    language_filters: Vec<String>,
    paths: Vec<GitTreeEntry>,
    cursor: usize,
    next_batch_index: usize,
    resource_budget: CodeIndexResourceBudget,
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
            tombstones: Vec::new(),
            resource_budget: self.resource_budget,
        }
    }

    /// Parses the next bounded file batch without retaining prior batches.
    pub fn parse_next_batch(mut self) -> Result<(Self, Option<CodeIndexBatch>), CodeIndexError> {
        if self.cursor >= self.paths.len() {
            return Ok((self, None));
        }

        let mut build = SnapshotBuild::new_with_selector(
            &self.registration,
            &self.selector,
            self.commit.clone(),
            self.tree_hash.clone(),
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
            let blobs = git_batch_blobs(&self.root, &self.commit, &fetched_paths)?;
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
    let worker_count = worker_count(paths.len());
    if paths.len() <= 1 || worker_count <= 1 {
        return paths
            .iter()
            .zip(blobs.iter())
            .map(|(path, bytes)| parse_one_file(plan, path, bytes))
            .collect();
    }

    let mut parsed = Vec::with_capacity(paths.len());
    for (path_chunk, blob_chunk) in paths.chunks(worker_count).zip(blobs.chunks(worker_count)) {
        let chunk = thread::scope(|scope| {
            let handles = path_chunk
                .iter()
                .zip(blob_chunk.iter())
                .map(|(path, bytes)| scope.spawn(move || parse_one_file(plan, path, bytes)))
                .collect::<Vec<_>>();
            handles
                .into_iter()
                .map(|handle| {
                    handle.join().map_err(|_| {
                        CodeIndexError::InvalidInput("code parser worker panicked".to_owned())
                    })?
                })
                .collect::<Result<Vec<_>, _>>()
        })?;
        parsed.extend(chunk);
    }

    Ok(parsed)
}

fn parse_one_file(
    plan: &CodeIndexPlan,
    path: &str,
    bytes: &[u8],
) -> Result<SnapshotBuild, CodeIndexError> {
    let mut build = SnapshotBuild::new_with_selector(
        &plan.registration,
        &plan.selector,
        plan.commit.clone(),
        plan.tree_hash.clone(),
        true,
        plan.paths.len(),
        0,
    );
    parse_indexed_file(&mut build, path, bytes)?;

    Ok(build)
}

fn worker_count(item_count: usize) -> usize {
    thread::available_parallelism()
        .map(usize::from)
        .unwrap_or(1)
        .min(item_count)
}

/// Prepares a full repository index as a bounded, checkpointable batch plan.
pub fn prepare_full_index_plan(
    registration: CodeRepositoryRegistration,
    selector: CodeRepositorySelector,
    resource_budget: CodeIndexResourceBudget,
) -> Result<CodeIndexPlan, CodeIndexError> {
    let root = PathBuf::from(&registration.root_path);
    let commit = resolve_ref(&root, &selector.ref_selector)?;
    let tree_hash = resolve_tree(&root, &commit)?;
    let ignore_rules = load_ignore_rules_from_commit(&root, &commit)?;
    let paths = tracked_entries(&root, &commit)?
        .into_iter()
        .filter(|entry| {
            selection_exclusion_reason(&entry.path, &registration, &selector, &ignore_rules)
                .is_none()
        })
        .collect::<Vec<_>>();
    let path_filters = merged_filters(&registration.path_filters, &selector.path_filters);
    let language_filters =
        merged_filters(&registration.language_filters, &selector.language_filters);
    let source_scope = code_snapshot_scope_id(
        &registration.repository_id,
        &tree_hash,
        &path_filters,
        &language_filters,
    );

    Ok(CodeIndexPlan {
        registration,
        selector,
        root,
        commit,
        tree_hash,
        source_scope,
        path_filters,
        language_filters,
        paths,
        cursor: 0,
        next_batch_index: 1,
        resource_budget,
    })
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
        .saturating_add(build.chunks.len())
        .saturating_add(build.diagnostics.len())
}

fn merged_filters(left: &[String], right: &[String]) -> Vec<String> {
    let mut merged = Vec::new();
    for value in left.iter().chain(right.iter()) {
        if !merged.contains(value) {
            merged.push(value.clone());
        }
    }

    merged
}
