use crate::{
    code::{SourceGrepKind, SourceGrepRequest},
    domain::{
        CodeQueryKind, CodeRepositoryStatus, CodeRetrievalHit, CodeRetrievalLayer,
        CodeRetrievalRequest,
    },
};

use super::super::source_surface::hit_has_complete_source_surface;
use super::{
    filters::{merged_filters, query_language_filters},
    identity::{
        definition_identity, definition_source_candidate_paths, exact_file_filter,
        hybrid_results_cover_identity, normalize_filter_path, reference_grep_query,
        results_define_identity, source_grep_identity,
    },
    imports::{import_grep_candidate_paths, import_grep_query, relative_path_import_specifier},
    surface::{hybrid_exact_path_source_fallback, hybrid_source_surface_fallback},
    worktree::source_fallback_commit,
};

pub(super) struct CodeGrepFallbackPlan {
    pub(super) commit: String,
    pub(super) query: String,
    pub(super) paths: Vec<String>,
    pub(super) path_filters: Vec<String>,
    pub(super) language_filters: Vec<String>,
    pub(super) limit: usize,
    pub(super) kind: SourceGrepKind,
    pub(super) identity: Option<String>,
    pub(super) exclude_generated: bool,
    pub(super) read_worktree_overlay: bool,
    pub(super) needs_scope_paths: bool,
}

impl CodeGrepFallbackPlan {
    pub(super) fn needs_scope_paths(&self) -> bool {
        self.needs_scope_paths
    }

    pub(super) fn with_scope_paths(mut self, scope_paths: Vec<String>) -> Self {
        if self.needs_scope_paths {
            self.paths = scope_paths;
            self.needs_scope_paths = false;
        }
        self
    }

    pub(super) fn source_request(&self) -> SourceGrepRequest {
        SourceGrepRequest {
            query: self.query.clone(),
            paths: self.paths.clone(),
            path_filters: self.path_filters.clone(),
            language_filters: self.language_filters.clone(),
            limit: self.limit,
            kind: self.kind,
            exclude_generated: self.exclude_generated,
        }
    }
}

pub(super) fn plan_code_grep_fallback(
    status: &CodeRepositoryStatus,
    request: &CodeRetrievalRequest,
    results: &[CodeRetrievalHit],
) -> Option<CodeGrepFallbackPlan> {
    let commit = source_fallback_commit(status)?;
    if !request.query_kind_filters.is_empty() {
        return None;
    }
    let path_filters = merged_filters(&status.path_filters, &request.repository.path_filters);
    let language_filters = query_language_filters(
        merged_filters(
            &status.language_filters,
            &request.repository.language_filters,
        ),
        &request.query_language_filters,
    );
    match request.code_query_kind {
        CodeQueryKind::Definition => {
            let identity = definition_identity(&request.query)?;
            if results_define_identity(results, &identity)
                && results.iter().any(|hit| {
                    hit.retrieval_layers
                        .contains(&CodeRetrievalLayer::Definition)
                })
                && results
                    .iter()
                    .any(|hit| hit_has_complete_source_surface(hit, &identity))
            {
                return None;
            }
            let paths = definition_source_candidate_paths(request, results, &identity);
            Some(CodeGrepFallbackPlan {
                commit: commit.commit.clone(),
                query: identity.clone(),
                needs_scope_paths: paths.is_empty(),
                paths,
                path_filters,
                language_filters,
                limit: request.limit,
                kind: SourceGrepKind::Definition,
                identity: Some(identity),
                exclude_generated: request.exclude_generated,
                read_worktree_overlay: commit.read_worktree_overlay,
            })
        }
        CodeQueryKind::References => {
            let identity = reference_grep_query(&request.query)?;
            if results.iter().any(|hit| {
                hit.retrieval_layers
                    .contains(&CodeRetrievalLayer::Reference)
            }) {
                return None;
            }
            let paths = path_filters
                .iter()
                .filter(|path| exact_file_filter(path))
                .map(|path| normalize_filter_path(path).to_owned())
                .collect::<Vec<_>>();
            let needs_scope_paths = paths.is_empty();
            Some(CodeGrepFallbackPlan {
                commit: commit.commit.clone(),
                query: identity,
                paths,
                path_filters,
                language_filters,
                limit: request.limit,
                kind: SourceGrepKind::References,
                identity: None,
                exclude_generated: request.exclude_generated,
                read_worktree_overlay: commit.read_worktree_overlay,
                needs_scope_paths,
            })
        }
        CodeQueryKind::Imports => {
            let query = import_grep_query(request, results)?;
            let local_relative_query = relative_path_import_specifier(&query);
            let paths = if local_relative_query {
                Vec::new()
            } else {
                import_grep_candidate_paths(results, &query)
            };
            let needs_scope_paths = local_relative_query || paths.is_empty();
            Some(CodeGrepFallbackPlan {
                commit: commit.commit.clone(),
                query,
                paths,
                path_filters,
                language_filters,
                limit: request.limit,
                kind: SourceGrepKind::Imports,
                identity: None,
                exclude_generated: request.exclude_generated,
                read_worktree_overlay: commit.read_worktree_overlay,
                needs_scope_paths,
            })
        }
        CodeQueryKind::Hybrid => {
            if let Some((query, paths)) = hybrid_exact_path_source_fallback(request, results) {
                return Some(CodeGrepFallbackPlan {
                    commit: commit.commit.clone(),
                    query,
                    paths,
                    path_filters,
                    language_filters,
                    limit: request.limit,
                    kind: SourceGrepKind::Hybrid,
                    identity: None,
                    exclude_generated: request.exclude_generated,
                    read_worktree_overlay: commit.read_worktree_overlay,
                    needs_scope_paths: false,
                });
            }
            if let Some((identity, paths)) = hybrid_source_surface_fallback(request, results) {
                return Some(CodeGrepFallbackPlan {
                    commit: commit.commit.clone(),
                    query: identity,
                    paths,
                    path_filters,
                    language_filters,
                    limit: request.limit,
                    kind: SourceGrepKind::Hybrid,
                    identity: None,
                    exclude_generated: request.exclude_generated,
                    read_worktree_overlay: commit.read_worktree_overlay,
                    needs_scope_paths: false,
                });
            }
            if results.len() >= request.limit {
                return None;
            }
            let identity = source_grep_identity(&request.query)?;
            if hybrid_results_cover_identity(results, &identity) {
                return None;
            }
            let paths = path_filters
                .iter()
                .filter(|path| exact_file_filter(path))
                .map(|path| normalize_filter_path(path).to_owned())
                .collect::<Vec<_>>();
            let needs_scope_paths = paths.is_empty();
            Some(CodeGrepFallbackPlan {
                commit: commit.commit.clone(),
                query: identity,
                paths,
                path_filters,
                language_filters,
                limit: request.limit.saturating_sub(results.len()).max(1),
                kind: SourceGrepKind::Hybrid,
                identity: None,
                exclude_generated: request.exclude_generated,
                read_worktree_overlay: commit.read_worktree_overlay,
                needs_scope_paths,
            })
        }
        _ => None,
    }
}
