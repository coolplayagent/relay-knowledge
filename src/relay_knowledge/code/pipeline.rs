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
    scope::{
        discover_source_layout, effective_index_path_filters, load_ignore_rules_from_commit,
        selection_exclusion_reason_with_layout,
    },
    snapshot::{SnapshotBuild, SnapshotScopeFilters},
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
            dependencies: build.dependencies,
            feature_flags: build.feature_flags,
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

    let mut parsed = thread::scope(|scope| {
        let handles = (0..worker_count)
            .map(|worker_index| {
                scope.spawn(move || {
                    parse_worker_stride(plan, paths, blobs, worker_index, worker_count)
                })
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

fn parse_worker_stride(
    plan: &CodeIndexPlan,
    paths: &[String],
    blobs: &[Vec<u8>],
    worker_index: usize,
    worker_count: usize,
) -> Result<Vec<(usize, SnapshotBuild)>, CodeIndexError> {
    let mut parsed = Vec::new();
    let mut index = worker_index;
    while index < paths.len() {
        parsed.push((index, parse_one_file(plan, &paths[index], &blobs[index])?));
        index += worker_count;
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
    let root = PathBuf::from(&registration.root_path);
    let commit = resolve_ref(&root, &selector.ref_selector)?;
    let tree_hash = resolve_tree(&root, &commit)?;
    let ignore_rules = load_ignore_rules_from_commit(&root, &commit)?;
    let entries = tracked_entries(&root, &commit)?;
    let source_layout = discover_source_layout(&entries);
    let paths = entries
        .into_iter()
        .filter(|entry| {
            selection_exclusion_reason_with_layout(
                &entry.path,
                &registration,
                &selector,
                &ignore_rules,
                &source_layout,
            )
            .is_none()
        })
        .collect::<Vec<_>>();
    let path_filters = effective_index_path_filters(&registration, &selector, &source_layout);
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
        .saturating_add(build.dependencies.len())
        .saturating_add(build.feature_flags.len())
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parser_worker_count_keeps_tiny_batches_serial() {
        assert_eq!(worker_count(7, 32 * 1024), 1);
    }

    #[test]
    fn parser_worker_count_scales_with_bounded_batch_work() {
        let available = thread::available_parallelism()
            .map(usize::from)
            .unwrap_or(1);
        let workers = worker_count(96, 4 * 1024 * 1024);

        assert_eq!(workers, available.min(8).min(96));
        assert!(workers >= 1);
    }

    #[test]
    fn parser_worker_count_caps_thread_fanout_for_small_byte_batches() {
        let available = thread::available_parallelism()
            .map(usize::from)
            .unwrap_or(1);
        let workers = worker_count(40, 128 * 1024);

        assert_eq!(workers, available.min(3).min(40));
    }

    #[test]
    fn batch_row_count_includes_feature_flags() {
        let registration =
            CodeRepositoryRegistration::new("repo", "fixture", "/tmp/repo", Vec::new(), Vec::new())
                .expect("registration should validate");
        let mut build = SnapshotBuild::new(
            &registration,
            "commit".to_owned(),
            "tree".to_owned(),
            true,
            1,
            0,
        );
        build.feature_flags = crate::code::feature_flags::extract_feature_flags(
            crate::code::feature_flags::FeatureFlagFileInput {
                repository_id: &build.repository_id,
                source_scope: &build.source_scope,
                file_id: "file",
                path: "src/lib.rs",
                language_id: "rust",
                content: "if env::var(\"CHECKOUT_V2\").is_ok() && env::var(\"PAYMENTS_V2\").is_ok() {}",
            },
        )
        .expect("feature flags should extract");

        assert_eq!(batch_row_count(&build), 2);
    }
}
