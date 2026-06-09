use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use crate::domain::{CodeRepositoryRegistration, RepositoryCodeRange};

use super::common::generated_detection;
use super::{
    CodeIndexError,
    languages::language_id,
    safe_git_blob_path,
    scope::{
        scoped_source_snapshot_for_registration, scoped_source_snapshot_for_registration_filters,
    },
    source::{
        source_batch_bytes_after_content_verification, source_blob_sizes_after_policy_verification,
        source_bytes_after_content_verification, source_commit_is_filesystem,
    },
    source_line_defines_identity,
};

mod query;
use query::{find_query_bytes, source_grep_queries};

pub(crate) const SOURCE_GREP_CANDIDATE_FILE_LIMIT: usize = 256;
const MAX_GREP_CANDIDATE_FILES: usize = SOURCE_GREP_CANDIDATE_FILE_LIMIT;
const MAX_GREP_BYTES: usize = 8 * 1024 * 1024;
const MAX_GREP_LINE_BYTES: usize = 4096;
const GENERATED_EXCLUSION_READ_BUDGET_MULTIPLIER: usize = 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SourceGrepKind {
    Definition,
    References,
    Imports,
    Hybrid,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SourceGrepRequest {
    pub(crate) query: String,
    pub(crate) paths: Vec<String>,
    pub(crate) path_filters: Vec<String>,
    pub(crate) language_filters: Vec<String>,
    pub(crate) limit: usize,
    pub(crate) kind: SourceGrepKind,
    pub(crate) exclude_generated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SourceGrepOutcome {
    pub(crate) matches: Vec<SourceGrepMatch>,
    pub(crate) degraded_reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SourceGrepMatch {
    pub(crate) path: String,
    pub(crate) language_id: String,
    pub(crate) excerpt: String,
    pub(crate) byte_range: RepositoryCodeRange,
    pub(crate) line_range: RepositoryCodeRange,
    pub(crate) is_generated: bool,
}

pub(crate) fn source_grep_matches(
    registration: &CodeRepositoryRegistration,
    commit: &str,
    request: SourceGrepRequest,
) -> Result<SourceGrepOutcome, CodeIndexError> {
    if request.limit == 0 || request.query.trim().is_empty() {
        return Ok(SourceGrepOutcome {
            matches: Vec::new(),
            degraded_reason: None,
        });
    }
    let candidates = selected_candidate_paths(&request);
    if candidates.paths.is_empty() {
        return Ok(SourceGrepOutcome {
            matches: Vec::new(),
            degraded_reason: candidates.degraded_reason,
        });
    }
    let mut tree = TempSourceTree::create()?;
    let materialized = materialize_source_blobs(
        registration,
        commit,
        &candidates.paths,
        SourceMaterializationOptions {
            path_filters: &request.path_filters,
            language_filters: &request.language_filters,
            exclude_generated: request.exclude_generated,
            max_bytes: MAX_GREP_BYTES,
        },
        &mut tree,
    )?;
    let degraded_reason = candidates
        .degraded_reason
        .or(materialized.degraded_reason.clone());
    if materialized.file_count == 0 {
        return Ok(SourceGrepOutcome {
            matches: Vec::new(),
            degraded_reason,
        });
    }
    source_grep_matches_from_materialized_tree(
        &tree.root,
        &candidates.paths,
        &request,
        degraded_reason,
    )
}

fn source_grep_matches_from_materialized_tree(
    root: &Path,
    paths: &[String],
    request: &SourceGrepRequest,
    degraded_reason: Option<String>,
) -> Result<SourceGrepOutcome, CodeIndexError> {
    let matches = internal_source_grep_matches(root, paths, request, |matched| {
        source_grep_accepts(request.kind, &request.query, matched)
    })?;

    Ok(SourceGrepOutcome {
        matches,
        degraded_reason,
    })
}

fn source_grep_accepts(kind: SourceGrepKind, query: &str, matched: &SourceGrepMatch) -> bool {
    kind != SourceGrepKind::Definition
        || matched
            .excerpt
            .lines()
            .map(str::trim)
            .any(|line| source_line_defines_identity(line, query))
}

fn selected_candidate_paths(request: &SourceGrepRequest) -> CandidatePaths {
    let mut paths = Vec::new();
    let mut seen = BTreeSet::new();
    let mut exhausted = false;
    for path in &request.paths {
        if paths.len() >= MAX_GREP_CANDIDATE_FILES {
            exhausted = true;
            break;
        }
        if !safe_git_blob_path(path) || !path_filter_allows(path, &request.path_filters) {
            continue;
        }
        if request.exclude_generated && generated_detection::path_has_generated_signal(path) {
            continue;
        }
        let language = language_id(path).unwrap_or("unknown");
        if !language_filter_allows(path, language, &request.language_filters) {
            continue;
        }
        if seen.insert(path.clone()) {
            paths.push(path.clone());
        }
    }
    CandidatePaths {
        paths,
        degraded_reason: exhausted
            .then(|| "source fallback candidate file budget exhausted".to_owned()),
    }
}

#[derive(Clone, Copy)]
struct SourceMaterializationOptions<'a> {
    path_filters: &'a [String],
    language_filters: &'a [String],
    exclude_generated: bool,
    max_bytes: usize,
}

fn materialize_source_blobs(
    registration: &CodeRepositoryRegistration,
    commit: &str,
    paths: &[String],
    options: SourceMaterializationOptions<'_>,
    tree: &mut TempSourceTree,
) -> Result<MaterializedFiles, CodeIndexError> {
    let root = PathBuf::from(&registration.root_path);
    materialize_source_blobs_at_root(registration, &root, commit, paths, options, tree)
}

fn materialize_source_blobs_at_root(
    registration: &CodeRepositoryRegistration,
    root: &Path,
    commit: &str,
    paths: &[String],
    options: SourceMaterializationOptions<'_>,
    tree: &mut TempSourceTree,
) -> Result<MaterializedFiles, CodeIndexError> {
    let verified_hashes = match ensure_source_grep_commit_current(
        registration,
        commit,
        options.path_filters,
        options.language_filters,
    ) {
        Ok(hashes) => hashes,
        Err(error) if source_commit_is_filesystem(commit) => {
            return Ok(MaterializedFiles {
                file_count: 0,
                degraded_reason: Some(error.to_string()),
            });
        }
        Err(error) => return Err(error),
    };
    let materialized = if options.exclude_generated {
        materialize_source_blobs_per_path(
            root,
            commit,
            paths,
            tree,
            options.max_bytes,
            verified_hashes.as_ref(),
            options.exclude_generated,
        )?
    } else if let Some(selection) =
        candidate_source_blob_selection(root, commit, paths, options.max_bytes)
    {
        materialize_selected_source_blobs(
            root,
            commit,
            selection,
            tree,
            verified_hashes.as_ref(),
            options.exclude_generated,
        )?
    } else {
        materialize_source_blobs_per_path(
            root,
            commit,
            paths,
            tree,
            options.max_bytes,
            verified_hashes.as_ref(),
            options.exclude_generated,
        )?
    };
    if let Err(error) = ensure_source_grep_commit_current(
        registration,
        commit,
        options.path_filters,
        options.language_filters,
    ) {
        if source_commit_is_filesystem(commit) {
            return Ok(MaterializedFiles {
                file_count: 0,
                degraded_reason: Some(error.to_string()),
            });
        }
        return Err(error);
    }

    Ok(materialized)
}

fn ensure_source_grep_commit_current(
    registration: &CodeRepositoryRegistration,
    commit: &str,
    path_filters: &[String],
    language_filters: &[String],
) -> Result<Option<BTreeMap<String, String>>, CodeIndexError> {
    if source_commit_is_filesystem(commit) {
        return filesystem_hashes_for_verified_scope(
            registration,
            commit,
            path_filters,
            language_filters,
        )
        .map(Some);
    }

    Ok(None)
}

fn filesystem_hashes_for_verified_scope(
    registration: &CodeRepositoryRegistration,
    commit: &str,
    path_filters: &[String],
    language_filters: &[String],
) -> Result<BTreeMap<String, String>, CodeIndexError> {
    match scoped_source_snapshot_for_registration(registration, commit) {
        Ok(snapshot) => Ok(snapshot.content_hashes),
        Err(stored_scope_error) => {
            match scoped_source_snapshot_for_registration_filters(
                registration,
                commit,
                path_filters,
                language_filters,
            ) {
                Ok(snapshot) => Ok(snapshot.content_hashes),
                Err(_) => Err(stored_scope_error),
            }
        }
    }
}

fn materialize_selected_source_blobs(
    root: &Path,
    commit: &str,
    selection: BlobCandidateSelection,
    tree: &mut TempSourceTree,
    expected_hashes: Option<&BTreeMap<String, String>>,
    exclude_generated: bool,
) -> Result<MaterializedFiles, CodeIndexError> {
    let mut file_count = 0usize;
    for (path, bytes) in selection.paths.iter().zip(candidate_source_blobs(
        root,
        commit,
        &selection.paths,
        expected_hashes,
    )) {
        let Some(bytes) = bytes else {
            continue;
        };
        if exclude_generated && generated_detection::is_generated_file(path, &bytes) {
            continue;
        }
        tree.write(path, bytes.as_slice())?;
        file_count += 1;
    }

    Ok(MaterializedFiles {
        file_count,
        degraded_reason: selection
            .exhausted
            .then(|| "source fallback materialized byte budget exhausted".to_owned()),
    })
}

fn materialize_source_blobs_per_path(
    root: &Path,
    commit: &str,
    paths: &[String],
    tree: &mut TempSourceTree,
    max_bytes: usize,
    expected_hashes: Option<&BTreeMap<String, String>>,
    exclude_generated: bool,
) -> Result<MaterializedFiles, CodeIndexError> {
    let sizes = source_blob_sizes_after_policy_verification(root, commit, paths).ok();
    let mut budget = SourceMaterializationBudget::new(max_bytes, exclude_generated);
    let mut file_count = 0usize;
    for (index, path) in paths.iter().enumerate() {
        if let Some(size) = sizes
            .as_ref()
            .and_then(|sizes| sizes.get(index))
            .copied()
            .flatten()
            && !budget.may_read_known_size(size)
        {
            continue;
        }
        let Ok(bytes) =
            source_bytes_after_content_verification(root, commit, path, expected_hashes)
        else {
            continue;
        };
        budget.record_read(bytes.len());
        if bytes.len() > max_bytes {
            budget.mark_exhausted();
            continue;
        }
        if exclude_generated && generated_detection::is_generated_file(path, &bytes) {
            continue;
        }
        if !budget.try_materialize(bytes.len()) {
            continue;
        }
        tree.write(path, &bytes)?;
        file_count += 1;
    }

    Ok(MaterializedFiles {
        file_count,
        degraded_reason: budget
            .is_exhausted()
            .then(|| "source fallback materialized byte budget exhausted".to_owned()),
    })
}

struct SourceMaterializationBudget {
    materialized_bytes: usize,
    read_bytes: usize,
    materialized_limit: usize,
    read_limit: usize,
    exclude_generated: bool,
    exhausted: bool,
}

impl SourceMaterializationBudget {
    fn new(materialized_limit: usize, exclude_generated: bool) -> Self {
        let read_limit = if exclude_generated {
            materialized_limit.saturating_mul(GENERATED_EXCLUSION_READ_BUDGET_MULTIPLIER)
        } else {
            materialized_limit
        };
        Self {
            materialized_bytes: 0,
            read_bytes: 0,
            materialized_limit,
            read_limit,
            exclude_generated,
            exhausted: false,
        }
    }

    fn may_read_known_size(&mut self, size: usize) -> bool {
        if size > self.materialized_limit
            || self.read_bytes.saturating_add(size) > self.read_limit
            || (!self.exclude_generated
                && self.materialized_bytes.saturating_add(size) > self.materialized_limit)
        {
            self.exhausted = true;
            return false;
        }
        true
    }

    fn record_read(&mut self, size: usize) {
        self.read_bytes = self.read_bytes.saturating_add(size);
        if self.read_bytes > self.read_limit {
            self.exhausted = true;
        }
    }

    fn try_materialize(&mut self, size: usize) -> bool {
        if self.materialized_bytes.saturating_add(size) > self.materialized_limit {
            self.exhausted = true;
            return false;
        }
        self.materialized_bytes += size;
        true
    }

    fn mark_exhausted(&mut self) {
        self.exhausted = true;
    }

    fn is_exhausted(&self) -> bool {
        self.exhausted
    }
}

struct BlobCandidateSelection {
    paths: Vec<String>,
    exhausted: bool,
}

fn candidate_source_blob_selection(
    root: &Path,
    commit: &str,
    paths: &[String],
    max_bytes: usize,
) -> Option<BlobCandidateSelection> {
    let sizes = source_blob_sizes_after_policy_verification(root, commit, paths).ok()?;
    if sizes.len() != paths.len() {
        return None;
    }

    let mut selected_paths = Vec::new();
    let mut byte_count = 0usize;
    let mut exhausted = false;
    for (path, size) in paths.iter().zip(sizes) {
        let Some(size) = size else {
            continue;
        };
        if byte_count.saturating_add(size) > max_bytes {
            exhausted = true;
            continue;
        }
        selected_paths.push(path.clone());
        byte_count += size;
    }

    Some(BlobCandidateSelection {
        paths: selected_paths,
        exhausted,
    })
}

fn candidate_source_blobs(
    root: &Path,
    commit: &str,
    paths: &[String],
    expected_hashes: Option<&BTreeMap<String, String>>,
) -> Vec<Option<Vec<u8>>> {
    match source_batch_bytes_after_content_verification(root, commit, paths, expected_hashes) {
        Ok(blobs) if blobs.len() == paths.len() => blobs.into_iter().map(Some).collect(),
        Err(_) if source_commit_is_filesystem(commit) => paths.iter().map(|_| None).collect(),
        _ => paths
            .iter()
            .map(|path| {
                source_bytes_after_content_verification(root, commit, path, expected_hashes).ok()
            })
            .collect(),
    }
}

fn internal_source_grep_matches(
    root: &Path,
    paths: &[String],
    request: &SourceGrepRequest,
    accepts: impl Fn(&SourceGrepMatch) -> bool,
) -> Result<Vec<SourceGrepMatch>, CodeIndexError> {
    let queries = source_grep_queries(request);
    if queries.is_empty() {
        return Ok(Vec::new());
    }

    let mut handwritten_matches = Vec::new();
    let mut generated_matches = Vec::new();
    for path in paths {
        if handwritten_matches.len() >= request.limit
            && (request.exclude_generated || generated_matches.len() >= request.limit)
        {
            break;
        }
        let Ok(bytes) = fs::read(root.join(path)) else {
            continue;
        };
        if source_bytes_are_binary(&bytes) {
            continue;
        }
        let is_generated = generated_detection::is_generated_file(path, &bytes);
        if request.exclude_generated && is_generated {
            continue;
        }
        let matches = if is_generated {
            &mut generated_matches
        } else {
            &mut handwritten_matches
        };
        if matches.len() >= request.limit {
            continue;
        }
        push_internal_file_matches(
            InternalFileScan {
                path,
                bytes: &bytes,
                is_generated,
            },
            &queries,
            request.kind,
            request.limit,
            &accepts,
            matches,
        )?;
    }

    let mut matches = handwritten_matches;
    if matches.len() < request.limit {
        matches.extend(
            generated_matches
                .into_iter()
                .take(request.limit - matches.len()),
        );
    }
    Ok(matches)
}

struct InternalFileScan<'a> {
    path: &'a str,
    bytes: &'a [u8],
    is_generated: bool,
}

fn push_internal_file_matches(
    input: InternalFileScan<'_>,
    queries: &[Vec<u8>],
    kind: SourceGrepKind,
    limit: usize,
    accepts: &impl Fn(&SourceGrepMatch) -> bool,
    matches: &mut Vec<SourceGrepMatch>,
) -> Result<(), CodeIndexError> {
    let path = input.path;
    let bytes = input.bytes;
    let mut line_start = 0usize;
    let mut line_number = 1usize;
    let mut previous_line = None;
    while line_start < bytes.len() && matches.len() < limit {
        let line_end = bytes[line_start..]
            .iter()
            .position(|byte| *byte == b'\n')
            .map_or(bytes.len(), |offset| line_start + offset);
        let line = &bytes[line_start..line_end];
        let mut carried_line = SourceLineContext {
            byte_start: line_start,
            byte_end: line_end,
            line_start: line_number,
        };
        if let Some((match_start, match_end)) = find_query_bytes(line, queries) {
            if line.len() > MAX_GREP_LINE_BYTES && kind == SourceGrepKind::Definition {
                line_start = if line_end < bytes.len() {
                    line_end + 1
                } else {
                    bytes.len()
                };
                line_number += 1;
                continue;
            }
            let context = source_grep_line_context(bytes, line_start, line_end, previous_line);
            if let Some(context) = context {
                carried_line = context;
            }
            let byte_range = RepositoryCodeRange::new(
                "byte_range",
                context
                    .as_ref()
                    .map_or(line_start + match_start, |context| context.byte_start),
                context
                    .as_ref()
                    .map_or(line_start + match_end, |context| context.byte_end),
            )
            .map_err(|error| CodeIndexError::InvalidInput(error.to_string()))?;
            let line_range = RepositoryCodeRange::new(
                "line_range",
                context
                    .as_ref()
                    .map_or(line_number, |context| context.line_start),
                line_number,
            )
            .map_err(|error| CodeIndexError::InvalidInput(error.to_string()))?;
            let excerpt = context.map_or_else(
                || {
                    String::from_utf8_lossy(source_line_excerpt(line, match_start, match_end))
                        .trim_end_matches('\r')
                        .trim()
                        .to_owned()
                },
                |context| {
                    String::from_utf8_lossy(&bytes[context.byte_start..context.byte_end])
                        .trim_end_matches('\r')
                        .trim()
                        .to_owned()
                },
            );
            let matched = SourceGrepMatch {
                path: path.to_owned(),
                language_id: language_id(path).unwrap_or("unknown").to_owned(),
                excerpt,
                byte_range,
                line_range,
                is_generated: input.is_generated,
            };
            if accepts(&matched) {
                matches.push(matched);
            }
        }
        previous_line = Some(carried_line);
        line_start = if line_end < bytes.len() {
            line_end + 1
        } else {
            bytes.len()
        };
        line_number += 1;
    }

    Ok(())
}

#[derive(Clone, Copy)]
struct SourceLineContext {
    byte_start: usize,
    byte_end: usize,
    line_start: usize,
}

fn source_grep_line_context(
    bytes: &[u8],
    line_start: usize,
    line_end: usize,
    previous_line: Option<SourceLineContext>,
) -> Option<SourceLineContext> {
    let previous = previous_line?;
    let previous_line = std::str::from_utf8(&bytes[previous.byte_start..previous.byte_end])
        .ok()?
        .trim();
    let current_line = std::str::from_utf8(&bytes[line_start..line_end])
        .ok()?
        .trim_start();
    if previous_line.starts_with("template ")
        || (current_line.starts_with('.')
            && (previous_line.ends_with('{')
                || previous_line
                    .lines()
                    .next()
                    .is_some_and(|line| line.trim_end().ends_with('{'))))
    {
        Some(SourceLineContext {
            byte_start: previous.byte_start,
            byte_end: line_end,
            line_start: previous.line_start,
        })
    } else {
        None
    }
}

fn source_bytes_are_binary(bytes: &[u8]) -> bool {
    bytes.contains(&0)
}

fn source_line_excerpt(line: &[u8], match_start: usize, match_end: usize) -> &[u8] {
    if line.len() <= MAX_GREP_LINE_BYTES {
        return line;
    }

    let match_len = match_end.saturating_sub(match_start);
    let budget = MAX_GREP_LINE_BYTES.max(match_len);
    let ideal_start = match_start.saturating_sub((budget.saturating_sub(match_len)) / 2);
    let max_start = line.len().saturating_sub(budget);
    let start = ideal_start.min(max_start);
    let end = start.saturating_add(budget).min(line.len());
    &line[start..end]
}

fn path_filter_allows(path: &str, filters: &[String]) -> bool {
    filters.is_empty()
        || filters.iter().any(|filter| {
            let filter = normalize_filter_path(filter);
            filter == "." || path == filter || path.starts_with(&format!("{filter}/"))
        })
}

fn language_filter_allows(path: &str, language_id: &str, filters: &[String]) -> bool {
    filters.is_empty()
        || filters.iter().any(|filter| {
            filter == language_id
                || cxx_header_filter_allows(path, language_id, filter)
                || unknown_filter_allows_document_path(path, language_id, filter)
        })
}

fn cxx_header_filter_allows(path: &str, language_id: &str, filter: &str) -> bool {
    filter == "cpp" && language_id == "c" && path.to_ascii_lowercase().ends_with(".h")
}

fn unknown_filter_allows_document_path(path: &str, language_id: &str, filter: &str) -> bool {
    filter == "unknown" && document_like_language_path(path, language_id)
}

fn document_like_language_path(path: &str, language_id: &str) -> bool {
    matches!(
        language_id,
        "markdown" | "json" | "yaml" | "toml" | "xml" | "ini" | "properties"
    ) || matches!(
        path.rsplit('.')
            .next()
            .map(str::to_ascii_lowercase)
            .as_deref(),
        Some("md" | "markdown" | "txt" | "rst" | "adoc")
    )
}

fn normalize_filter_path(filter: &str) -> &str {
    let mut filter = filter.trim_end_matches(['/', '\\']);
    while let Some(stripped) = filter.strip_prefix("./") {
        filter = stripped;
    }

    filter
}

struct CandidatePaths {
    paths: Vec<String>,
    degraded_reason: Option<String>,
}

struct MaterializedFiles {
    file_count: usize,
    degraded_reason: Option<String>,
}

struct TempSourceTree {
    root: PathBuf,
}

impl TempSourceTree {
    fn create() -> Result<Self, CodeIndexError> {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or_default();
        let root = std::env::temp_dir().join(format!(
            "relay-knowledge-source-grep-{}-{nanos}",
            std::process::id()
        ));
        fs::create_dir_all(&root)?;

        Ok(Self { root })
    }

    fn write(&mut self, path: &str, bytes: &[u8]) -> Result<(), CodeIndexError> {
        let target = self.root.join(path);
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(target, bytes)?;

        Ok(())
    }
}

impl Drop for TempSourceTree {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[cfg(test)]
mod tests;
