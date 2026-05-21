use std::collections::{BTreeMap, BTreeSet};

use crate::{
    code::{SourceDeclarationMatch, simple_source_identifier, source_line_defines_identity},
    domain::{
        CodeQueryKind, CodeRepositoryStatus, CodeRetrievalHit, CodeRetrievalLayer,
        CodeRetrievalRequest,
    },
};

const MAX_DEFINITION_SOURCE_CANDIDATE_PATHS: usize = 8;

pub(super) struct DefinitionSourceFallbackPlan {
    pub(super) commit: String,
    pub(super) identity: String,
    pub(super) paths: Vec<String>,
}

pub(super) fn plan_definition_source_fallback(
    status: &CodeRepositoryStatus,
    request: &CodeRetrievalRequest,
    results: &[CodeRetrievalHit],
) -> Option<DefinitionSourceFallbackPlan> {
    if request.code_query_kind != CodeQueryKind::Definition {
        return None;
    }
    let identity = definition_identity(&request.query)?;
    if results_define_identity(results, &identity) {
        return None;
    }
    let commit = status.last_indexed_commit.clone()?;
    let paths = definition_source_candidate_paths(request, results, &identity);
    (!paths.is_empty()).then_some(DefinitionSourceFallbackPlan {
        commit,
        identity,
        paths,
    })
}

pub(super) fn append_definition_source_fallback(
    status: &CodeRepositoryStatus,
    request: &CodeRetrievalRequest,
    results: &mut Vec<CodeRetrievalHit>,
    declarations: Vec<SourceDeclarationMatch>,
) {
    if declarations.is_empty() {
        return;
    }
    let best_score = results.first().map_or(0.0, |hit| hit.score);
    let metadata = path_metadata(results);
    for declaration in declarations {
        if results.iter().any(|hit| {
            hit.path == declaration.path
                && hit.line_range.start == declaration.line_range.start
                && hit.excerpt == declaration.excerpt
        }) {
            continue;
        }
        let path_metadata = metadata.get(&declaration.path);
        results.push(CodeRetrievalHit {
            repository_id: status.repository_id.clone(),
            scope_id: status.last_indexed_scope_id.clone().unwrap_or_default(),
            resolved_commit_sha: status.last_indexed_commit.clone().unwrap_or_default(),
            tree_hash: status.tree_hash.clone().unwrap_or_default(),
            path: declaration.path,
            language_id: path_metadata
                .map(|metadata| metadata.language_id.clone())
                .unwrap_or_default(),
            byte_range: declaration.byte_range,
            line_range: declaration.line_range,
            symbol_snapshot_id: path_metadata
                .and_then(|metadata| metadata.symbol_snapshot_id.clone()),
            canonical_symbol_id: path_metadata
                .and_then(|metadata| metadata.canonical_symbol_id.clone()),
            file_id: path_metadata.and_then(|metadata| metadata.file_id.clone()),
            retrieval_layers: vec![
                CodeRetrievalLayer::Definition,
                CodeRetrievalLayer::TextFallback,
            ],
            index_versions: vec![format!(
                "code:{}:{}",
                status
                    .last_indexed_scope_id
                    .as_deref()
                    .unwrap_or("unscoped"),
                status.tree_hash.as_deref().unwrap_or("unindexed")
            )],
            stale: status.stale,
            degraded_reason: status.degraded_reason.clone(),
            edge_kind: None,
            edge_resolution_state: None,
            edge_target_hint: None,
            edge_confidence_basis_points: None,
            edge_confidence_tier: None,
            score: best_score + 4.0,
            excerpt: declaration.excerpt,
        });
    }
    dedupe_sort_truncate(results, request.limit);
}

fn definition_source_candidate_paths(
    request: &CodeRetrievalRequest,
    results: &[CodeRetrievalHit],
    identity: &str,
) -> Vec<String> {
    let mut paths = Vec::new();
    for hit in results {
        if hit_mentions_identity(hit, identity) {
            push_candidate_path(&mut paths, &hit.path);
        }
    }
    for path in &request.repository.path_filters {
        if exact_file_filter(path) {
            push_candidate_path(&mut paths, path);
        }
    }
    paths.truncate(MAX_DEFINITION_SOURCE_CANDIDATE_PATHS);

    paths
}

fn hit_mentions_identity(hit: &CodeRetrievalHit, identity: &str) -> bool {
    hit.excerpt.contains(identity)
        || hit
            .canonical_symbol_id
            .as_deref()
            .is_some_and(|symbol_id| symbol_id.contains(identity))
}

fn push_candidate_path(paths: &mut Vec<String>, path: &str) {
    let normalized = normalize_filter_path(path);
    if !normalized.is_empty() && !paths.iter().any(|existing| existing == normalized) {
        paths.push(normalized.to_owned());
    }
}

fn exact_file_filter(path: &str) -> bool {
    let path = normalize_filter_path(path);
    !path.is_empty()
        && path
            .rsplit('/')
            .next()
            .is_some_and(|name| name.contains('.'))
        && !path.ends_with('/')
}

fn normalize_filter_path(path: &str) -> &str {
    let mut path = path.trim_end_matches(['/', '\\']);
    while let Some(stripped) = path.strip_prefix("./") {
        path = stripped;
    }

    path
}

fn results_define_identity(results: &[CodeRetrievalHit], identity: &str) -> bool {
    results.iter().any(|hit| {
        hit.excerpt
            .lines()
            .map(str::trim)
            .any(|line| source_line_defines_identity(line, identity))
    })
}

fn definition_identity(query: &str) -> Option<String> {
    for raw_token in query.split_whitespace().map(str::trim) {
        if raw_token.contains('/') || raw_token.contains('\\') {
            continue;
        }
        let terms = raw_token
            .split(|character: char| !(character.is_ascii_alphanumeric() || character == '_'))
            .filter(|term| !term.is_empty())
            .collect::<Vec<_>>();
        if let Some(term) = terms.last().filter(|term| simple_source_identifier(term)) {
            return Some((*term).to_owned());
        }
    }

    None
}

struct HitPathMetadata {
    language_id: String,
    symbol_snapshot_id: Option<String>,
    canonical_symbol_id: Option<String>,
    file_id: Option<String>,
}

fn path_metadata(results: &[CodeRetrievalHit]) -> BTreeMap<String, HitPathMetadata> {
    let mut metadata = BTreeMap::new();
    for hit in results {
        metadata
            .entry(hit.path.clone())
            .or_insert_with(|| HitPathMetadata {
                language_id: hit.language_id.clone(),
                symbol_snapshot_id: hit.symbol_snapshot_id.clone(),
                canonical_symbol_id: hit.canonical_symbol_id.clone(),
                file_id: hit.file_id.clone(),
            });
    }

    metadata
}

fn dedupe_sort_truncate(results: &mut Vec<CodeRetrievalHit>, limit: usize) {
    let mut seen = BTreeSet::new();
    results
        .retain(|hit| seen.insert((hit.path.clone(), hit.line_range.start, hit.excerpt.clone())));
    results.sort_by(|left, right| {
        right
            .score
            .total_cmp(&left.score)
            .then_with(|| left.path.cmp(&right.path))
            .then_with(|| left.line_range.start.cmp(&right.line_range.start))
    });
    results.truncate(limit);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{RepositoryCodeRange, code_snapshot_scope_id};

    #[test]
    fn fallback_plan_uses_contextual_hits_and_exact_file_filters() {
        let request = request(
            "rk_read_fn",
            CodeQueryKind::Definition,
            vec!["include/driver_ops.h".to_owned()],
        );
        let hit = hit(
            "include/driver_ops.h",
            "struct rk_driver_ops {\n    rk_read_fn read;\n}",
        );

        let plan = plan_definition_source_fallback(&status(), &request, &[hit])
            .expect("contextual hit should plan fallback");

        assert_eq!(plan.identity, "rk_read_fn");
        assert_eq!(plan.paths, ["include/driver_ops.h"]);
    }

    #[test]
    fn fallback_plan_skips_results_with_exact_declaration() {
        let request = request("rk_read_fn", CodeQueryKind::Definition, Vec::new());
        let hit = hit(
            "include/driver_ops.h",
            "typedef int (*rk_read_fn)(struct rk_device *dev);",
        );

        assert!(plan_definition_source_fallback(&status(), &request, &[hit]).is_none());
    }

    fn request(
        query: &str,
        kind: CodeQueryKind,
        path_filters: Vec<String>,
    ) -> CodeRetrievalRequest {
        let selector = crate::domain::CodeRepositorySelector::new(
            "repo",
            "commit",
            path_filters,
            vec!["c".to_owned()],
        )
        .expect("selector should validate");
        CodeRetrievalRequest::new(
            query,
            selector,
            kind,
            10,
            crate::domain::FreshnessPolicy::AllowStale,
        )
        .expect("request should validate")
    }

    fn status() -> CodeRepositoryStatus {
        CodeRepositoryStatus {
            repository_id: "repo".to_owned(),
            alias: "repo".to_owned(),
            root_path: "/tmp/repo".to_owned(),
            path_filters: Vec::new(),
            language_filters: Vec::new(),
            last_indexed_scope_id: Some(code_snapshot_scope_id("repo", "tree", &[], &[])),
            last_indexed_commit: Some("commit".to_owned()),
            tree_hash: Some("tree".to_owned()),
            state: "fresh".to_owned(),
            indexed_file_count: 1,
            symbol_count: 1,
            reference_count: 0,
            chunk_count: 1,
            stale: false,
            degraded_reason: None,
        }
    }

    fn hit(path: &str, excerpt: &str) -> CodeRetrievalHit {
        CodeRetrievalHit {
            repository_id: "repo".to_owned(),
            scope_id: "scope".to_owned(),
            resolved_commit_sha: "commit".to_owned(),
            tree_hash: "tree".to_owned(),
            path: path.to_owned(),
            language_id: "c".to_owned(),
            byte_range: RepositoryCodeRange { start: 0, end: 1 },
            line_range: RepositoryCodeRange { start: 1, end: 1 },
            symbol_snapshot_id: Some("symbol".to_owned()),
            canonical_symbol_id: Some("repo://repo/include::driver_ops::rk_driver_ops".to_owned()),
            file_id: Some("file".to_owned()),
            retrieval_layers: vec![CodeRetrievalLayer::Lexical],
            index_versions: vec!["code:scope:tree".to_owned()],
            stale: false,
            degraded_reason: None,
            edge_kind: None,
            edge_resolution_state: None,
            edge_target_hint: None,
            edge_confidence_basis_points: None,
            edge_confidence_tier: None,
            score: 2.0,
            excerpt: excerpt.to_owned(),
        }
    }
}
