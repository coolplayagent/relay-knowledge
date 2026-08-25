use std::{collections::BTreeMap, path::Path};

use crate::domain::{CodeRepositoryRegistration, RepositoryCodeRange};

use super::{CodeIndexError, source_line_defines_identity};

mod candidate_scope;
mod materialization;
mod query;
mod scanner;
use candidate_scope::selected_candidate_paths;
use materialization::{
    TempSourceTree, materialize_source_blobs, materialize_worktree_overlay_source_blobs,
};
use scanner::internal_source_grep_matches;

pub(crate) use candidate_scope::{
    SOURCE_GREP_CANDIDATE_FILE_LIMIT, bounded_source_grep_candidate_match_limit,
};

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

pub(crate) fn source_fallback_reference_language_is_code(language_id: &str) -> bool {
    !matches!(
        language_id,
        "gomod" | "ini" | "json" | "markdown" | "properties" | "toml" | "xml" | "yaml"
    )
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
        &request.path_filters,
        &request.language_filters,
        request.exclude_generated,
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

pub(crate) fn source_grep_matches_from_worktree_overlay(
    registration: &CodeRepositoryRegistration,
    expected_hashes: BTreeMap<String, String>,
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
    let materialized = materialize_worktree_overlay_source_blobs(
        registration,
        &candidates.paths,
        &mut tree,
        &expected_hashes,
        request.exclude_generated,
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

#[cfg(test)]
#[path = "mod_tests.rs"]
mod mod_tests;
