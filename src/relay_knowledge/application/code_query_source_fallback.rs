use std::collections::{BTreeMap, BTreeSet};

use crate::{
    code::{
        SourceDeclarationMatch, SourceGrepKind, SourceGrepMatch, SourceGrepOutcome,
        SourceGrepRequest, simple_source_identifier, source_line_defines_identity,
    },
    domain::{
        CodeQueryKind, CodeRepositoryStatus, CodeRetrievalHit, CodeRetrievalLayer,
        CodeRetrievalRequest,
    },
};

const MAX_DEFINITION_SOURCE_CANDIDATE_PATHS: usize = 8;
const EXTERNAL_IMPORT_GREP_DIAGNOSTIC: &str = "external dependency import is not indexed in the code graph; searched current repository source with internal grep fallback";

pub(super) struct CodeGrepFallbackPlan {
    pub(super) commit: String,
    pub(super) query: String,
    pub(super) paths: Vec<String>,
    pub(super) path_filters: Vec<String>,
    pub(super) language_filters: Vec<String>,
    pub(super) limit: usize,
    pub(super) kind: SourceGrepKind,
    pub(super) identity: Option<String>,
    needs_scope_paths: bool,
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
        }
    }
}

pub(super) fn plan_code_grep_fallback(
    status: &CodeRepositoryStatus,
    request: &CodeRetrievalRequest,
    results: &[CodeRetrievalHit],
) -> Option<CodeGrepFallbackPlan> {
    let commit = status.last_indexed_commit.clone()?;
    let path_filters = merged_filters(&status.path_filters, &request.repository.path_filters);
    let language_filters = merged_filters(
        &status.language_filters,
        &request.repository.language_filters,
    );
    match request.code_query_kind {
        CodeQueryKind::Definition => {
            let identity = definition_identity(&request.query)?;
            if results_define_identity(results, &identity)
                && results.iter().any(|hit| {
                    hit.retrieval_layers
                        .contains(&CodeRetrievalLayer::Definition)
                })
            {
                return None;
            }
            let paths = definition_source_candidate_paths(request, results, &identity);
            Some(CodeGrepFallbackPlan {
                commit,
                query: identity.clone(),
                needs_scope_paths: paths.is_empty(),
                paths,
                path_filters,
                language_filters,
                limit: request.limit,
                kind: SourceGrepKind::Definition,
                identity: Some(identity),
            })
        }
        CodeQueryKind::References => {
            let identity = source_grep_identity(&request.query)?;
            if results.iter().any(|hit| {
                hit.retrieval_layers
                    .contains(&CodeRetrievalLayer::Reference)
            }) {
                return None;
            }
            Some(CodeGrepFallbackPlan {
                commit,
                query: identity,
                paths: Vec::new(),
                path_filters,
                language_filters,
                limit: request.limit,
                kind: SourceGrepKind::References,
                identity: None,
                needs_scope_paths: true,
            })
        }
        CodeQueryKind::Imports => {
            let query = import_grep_query(&request.query)?;
            if !results.is_empty() && !results_have_unresolved_import(results) {
                return None;
            }
            Some(CodeGrepFallbackPlan {
                commit,
                query,
                paths: Vec::new(),
                path_filters,
                language_filters,
                limit: request.limit,
                kind: SourceGrepKind::Imports,
                identity: None,
                needs_scope_paths: true,
            })
        }
        CodeQueryKind::Hybrid if results.len() < request.limit => {
            let identity = source_grep_identity(&request.query)?;
            Some(CodeGrepFallbackPlan {
                commit,
                query: identity,
                paths: Vec::new(),
                path_filters,
                language_filters,
                limit: request.limit.saturating_sub(results.len()).max(1),
                kind: SourceGrepKind::Hybrid,
                identity: None,
                needs_scope_paths: true,
            })
        }
        _ => None,
    }
}

pub(super) fn append_code_grep_fallback(
    status: &CodeRepositoryStatus,
    request: &CodeRetrievalRequest,
    results: &mut Vec<CodeRetrievalHit>,
    plan: &CodeGrepFallbackPlan,
    outcome: SourceGrepOutcome,
) -> Option<String> {
    if outcome.matches.is_empty() {
        return fallback_diagnostic(plan, outcome.degraded_reason);
    }
    let score_bounds = ScoreBounds::from_results(results);
    let fallback_score = grep_score(plan.kind, score_bounds);
    let metadata = path_metadata(results);
    for matched in outcome.matches {
        if let Some(existing) = results.iter_mut().find(|hit| {
            hit.path == matched.path
                && hit.line_range.start == matched.line_range.start
                && hit.excerpt == matched.excerpt
        }) {
            add_code_grep_layers(existing, plan.kind);
            existing.score = existing.score.max(fallback_score);
            continue;
        }
        let path_metadata = metadata.get(&matched.path);
        results.push(code_grep_hit(
            status,
            &matched,
            path_metadata,
            plan.kind,
            fallback_score,
            outcome.degraded_reason.clone(),
        ));
    }
    dedupe_sort_truncate(results, request.limit);

    fallback_diagnostic(plan, outcome.degraded_reason)
}

fn add_code_grep_layers(hit: &mut CodeRetrievalHit, kind: SourceGrepKind) {
    if kind == SourceGrepKind::Definition {
        add_retrieval_layer(hit, CodeRetrievalLayer::Definition);
    }
    add_retrieval_layer(hit, CodeRetrievalLayer::Lexical);
    add_retrieval_layer(hit, CodeRetrievalLayer::TextFallback);
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
        if let Some(existing) = results.iter_mut().find(|hit| {
            hit.path == declaration.path
                && hit.line_range.start == declaration.line_range.start
                && hit.excerpt == declaration.excerpt
        }) {
            add_retrieval_layer(existing, CodeRetrievalLayer::Definition);
            add_retrieval_layer(existing, CodeRetrievalLayer::Lexical);
            add_retrieval_layer(existing, CodeRetrievalLayer::TextFallback);
            existing.score = existing.score.max(best_score + 4.0);
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
                CodeRetrievalLayer::Lexical,
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

fn add_retrieval_layer(hit: &mut CodeRetrievalHit, layer: CodeRetrievalLayer) {
    if !hit.retrieval_layers.contains(&layer) {
        hit.retrieval_layers.push(layer);
    }
}

fn code_grep_hit(
    status: &CodeRepositoryStatus,
    matched: &SourceGrepMatch,
    path_metadata: Option<&HitPathMetadata>,
    kind: SourceGrepKind,
    score: f64,
    degraded_reason: Option<String>,
) -> CodeRetrievalHit {
    let mut layers = vec![
        CodeRetrievalLayer::Lexical,
        CodeRetrievalLayer::TextFallback,
    ];
    if kind == SourceGrepKind::Definition {
        layers.insert(0, CodeRetrievalLayer::Definition);
    }

    CodeRetrievalHit {
        repository_id: status.repository_id.clone(),
        scope_id: status.last_indexed_scope_id.clone().unwrap_or_default(),
        resolved_commit_sha: status.last_indexed_commit.clone().unwrap_or_default(),
        tree_hash: status.tree_hash.clone().unwrap_or_default(),
        path: matched.path.clone(),
        language_id: path_metadata
            .map(|metadata| metadata.language_id.clone())
            .unwrap_or_else(|| matched.language_id.clone()),
        byte_range: matched.byte_range.clone(),
        line_range: matched.line_range.clone(),
        symbol_snapshot_id: path_metadata.and_then(|metadata| metadata.symbol_snapshot_id.clone()),
        canonical_symbol_id: path_metadata
            .and_then(|metadata| metadata.canonical_symbol_id.clone()),
        file_id: path_metadata.and_then(|metadata| metadata.file_id.clone()),
        retrieval_layers: layers,
        index_versions: vec![format!(
            "code:{}:{}",
            status
                .last_indexed_scope_id
                .as_deref()
                .unwrap_or("unscoped"),
            status.tree_hash.as_deref().unwrap_or("unindexed")
        )],
        stale: status.stale,
        degraded_reason: degraded_reason.or_else(|| status.degraded_reason.clone()),
        edge_kind: None,
        edge_resolution_state: None,
        edge_target_hint: None,
        edge_confidence_basis_points: None,
        edge_confidence_tier: None,
        score,
        excerpt: matched.excerpt.clone(),
    }
}

#[derive(Clone, Copy)]
struct ScoreBounds {
    best: Option<f64>,
    lowest: Option<f64>,
}

impl ScoreBounds {
    fn from_results(results: &[CodeRetrievalHit]) -> Self {
        let mut bounds = Self {
            best: None,
            lowest: None,
        };
        for hit in results {
            bounds.best = Some(bounds.best.map_or(hit.score, |best| best.max(hit.score)));
            bounds.lowest = Some(
                bounds
                    .lowest
                    .map_or(hit.score, |lowest| lowest.min(hit.score)),
            );
        }

        bounds
    }
}

fn grep_score(kind: SourceGrepKind, score_bounds: ScoreBounds) -> f64 {
    match kind {
        SourceGrepKind::Definition => score_bounds.best.unwrap_or(0.0) + 3.5,
        SourceGrepKind::References => score_bounds.best.unwrap_or(0.0) + 2.0,
        SourceGrepKind::Imports => score_bounds.best.unwrap_or(0.0) + 1.0,
        SourceGrepKind::Hybrid => score_bounds.lowest.map(|score| score - 0.25).unwrap_or(1.0),
    }
}

fn fallback_diagnostic(
    plan: &CodeGrepFallbackPlan,
    degraded_reason: Option<String>,
) -> Option<String> {
    let Some(reason) = degraded_reason else {
        return (plan.kind == SourceGrepKind::Imports)
            .then(|| EXTERNAL_IMPORT_GREP_DIAGNOSTIC.to_owned());
    };
    if plan.kind == SourceGrepKind::Imports {
        Some(format!("{EXTERNAL_IMPORT_GREP_DIAGNOSTIC}; {reason}"))
    } else {
        Some(reason)
    }
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

fn source_grep_identity(query: &str) -> Option<String> {
    let identity = definition_identity(query)?;
    (query.split_whitespace().count() == 1).then_some(identity)
}

fn import_grep_query(query: &str) -> Option<String> {
    let trimmed = query.trim();
    if trimmed.is_empty() {
        return None;
    }
    if trimmed.len() <= 128 {
        return Some(trimmed.to_owned());
    }
    definition_identity(trimmed)
}

fn results_have_unresolved_import(results: &[CodeRetrievalHit]) -> bool {
    results.iter().any(|hit| {
        hit.edge_kind.as_deref() == Some("import")
            && hit
                .edge_resolution_state
                .as_deref()
                .is_none_or(|state| state != "resolved")
    })
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

        let plan = plan_code_grep_fallback(&status(), &request, &[hit])
            .expect("contextual hit should plan fallback");

        assert_eq!(plan.identity.as_deref(), Some("rk_read_fn"));
        assert_eq!(plan.query, "rk_read_fn");
        assert_eq!(plan.paths, ["include/driver_ops.h"]);
    }

    #[test]
    fn fallback_plan_skips_results_with_exact_declaration() {
        let request = request("rk_read_fn", CodeQueryKind::Definition, Vec::new());
        let mut hit = hit(
            "include/driver_ops.h",
            "typedef int (*rk_read_fn)(struct rk_device *dev);",
        );
        hit.retrieval_layers = vec![CodeRetrievalLayer::Symbol, CodeRetrievalLayer::Definition];

        assert!(plan_code_grep_fallback(&status(), &request, &[hit]).is_none());
    }

    #[test]
    fn hybrid_grep_fallback_fills_after_structured_hits() {
        let request = request("rk_helper", CodeQueryKind::Hybrid, Vec::new());
        let mut results = vec![hit("src/lib.c", "void structured_hit(void);")];
        let plan = plan_code_grep_fallback(&status(), &request, &results)
            .expect("partial hybrid results should plan fallback");
        let outcome = SourceGrepOutcome {
            matches: vec![SourceGrepMatch {
                path: "src/fallback.c".to_owned(),
                language_id: "c".to_owned(),
                excerpt: "rk_helper();".to_owned(),
                byte_range: RepositoryCodeRange { start: 10, end: 19 },
                line_range: RepositoryCodeRange { start: 4, end: 4 },
            }],
            degraded_reason: None,
        };

        append_code_grep_fallback(&status(), &request, &mut results, &plan, outcome);

        assert_eq!(results[0].path, "src/lib.c");
        let fallback = results
            .iter()
            .find(|hit| hit.path == "src/fallback.c")
            .expect("fallback hit should be appended");
        assert!(fallback.score < results[0].score);
        assert!(
            fallback
                .retrieval_layers
                .contains(&CodeRetrievalLayer::TextFallback)
        );
    }

    #[test]
    fn import_fallback_runs_for_unresolved_external_imports_and_reports_capability() {
        let request = request("react", CodeQueryKind::Imports, Vec::new());
        let mut import_hit = hit("src/component.tsx", "import React from \"react\";");
        import_hit.edge_kind = Some("import".to_owned());
        import_hit.edge_resolution_state = Some("unresolved".to_owned());
        import_hit.retrieval_layers = vec![CodeRetrievalLayer::ImportGraph];
        let mut results = vec![import_hit];
        let plan = plan_code_grep_fallback(&status(), &request, &results)
            .expect("unresolved import should plan source fallback");
        let outcome = SourceGrepOutcome {
            matches: vec![SourceGrepMatch {
                path: "src/component.tsx".to_owned(),
                language_id: "tsx".to_owned(),
                excerpt: "import React from \"react\";".to_owned(),
                byte_range: RepositoryCodeRange { start: 0, end: 26 },
                line_range: RepositoryCodeRange { start: 1, end: 1 },
            }],
            degraded_reason: None,
        };

        let reason = append_code_grep_fallback(&status(), &request, &mut results, &plan, outcome)
            .expect("import fallback should explain external dependency fallback");

        assert!(reason.contains("external dependency import is not indexed"));
        let hit = results
            .iter()
            .find(|hit| hit.path == "src/component.tsx")
            .expect("import hit should remain");
        assert!(
            hit.retrieval_layers
                .contains(&CodeRetrievalLayer::ImportGraph)
        );
        assert!(
            hit.retrieval_layers
                .contains(&CodeRetrievalLayer::TextFallback)
        );
    }

    #[test]
    fn import_fallback_skips_resolved_import_graph_hits() {
        let request = request("crate::local", CodeQueryKind::Imports, Vec::new());
        let mut import_hit = hit("src/lib.rs", "use crate::local;");
        import_hit.edge_kind = Some("import".to_owned());
        import_hit.edge_resolution_state = Some("resolved".to_owned());
        import_hit.retrieval_layers = vec![CodeRetrievalLayer::ImportGraph];

        assert!(plan_code_grep_fallback(&status(), &request, &[import_hit]).is_none());
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
