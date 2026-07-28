use std::collections::{BTreeMap, BTreeSet};

use crate::{
    code::{SourceDeclarationMatch, SourceGrepKind, SourceGrepMatch, SourceGrepOutcome},
    domain::{
        CodeRepositoryStatus, CodeRetrievalHit, CodeRetrievalLayer, CodeRetrievalRequest,
        StalenessHint,
    },
};

use super::{
    filters::query_field_filters_allow_match,
    imports::local_import_specifier,
    plan::CodeGrepFallbackPlan,
    scoring::{
        ScoreBounds, generated_adjusted_fallback_score, grep_score, source_grep_match_score,
    },
    surface::{hit_allows_source_refresh, hit_source_line_is_better},
};

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
    let base_fallback_score = grep_score(plan.kind, score_bounds);
    let metadata = path_metadata(results);
    for matched in outcome.matches {
        if !query_field_filters_allow_match(request, &matched.path, &matched.excerpt) {
            continue;
        }
        let fallback_score = generated_adjusted_fallback_score(
            source_grep_match_score(request, plan, &matched, score_bounds, base_fallback_score),
            matched.is_generated,
        );
        if let Some(existing) = results.iter_mut().find(|hit| {
            hit.path == matched.path
                && hit.line_range.start == matched.line_range.start
                && (hit.excerpt == matched.excerpt
                    || (plan.kind == SourceGrepKind::Hybrid && hit_allows_source_refresh(hit)))
        }) {
            add_code_grep_layers(existing, plan.kind);
            if plan.kind == SourceGrepKind::Hybrid
                && hit_allows_source_refresh(existing)
                && hit_source_line_is_better(existing, &matched, &plan.query)
            {
                existing.excerpt = matched.excerpt.clone();
            }
            existing.score = existing.score.max(fallback_score);
            continue;
        }
        let mut should_push_nested_match = true;
        if plan.kind == SourceGrepKind::Hybrid
            && let Some(existing) = results.iter_mut().find(|hit| {
                hit.path == matched.path
                    && hit_allows_source_refresh(hit)
                    && matched.line_range.start >= hit.line_range.start
                    && matched.line_range.end <= hit.line_range.end
            })
        {
            add_code_grep_layers(existing, plan.kind);
            if hit_source_line_is_better(existing, &matched, &plan.query) {
                existing.excerpt = matched.excerpt.clone();
                should_push_nested_match = false;
            }
            existing.score = existing.score.max(fallback_score);
            if matched.line_range.start == existing.line_range.start {
                should_push_nested_match = false;
            }
        }
        if !should_push_nested_match {
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
        if !query_field_filters_allow_match(request, &declaration.path, &declaration.excerpt) {
            continue;
        }
        let declaration_score =
            generated_adjusted_fallback_score(best_score + 4.0, declaration.is_generated);
        if let Some(existing) = results.iter_mut().find(|hit| {
            hit.path == declaration.path
                && hit.line_range.start == declaration.line_range.start
                && hit.excerpt == declaration.excerpt
        }) {
            add_retrieval_layer(existing, CodeRetrievalLayer::Definition);
            add_retrieval_layer(existing, CodeRetrievalLayer::Lexical);
            add_retrieval_layer(existing, CodeRetrievalLayer::TextFallback);
            existing.score = existing.score.max(declaration_score);
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
            staleness_hint: Some(if status.stale {
                StalenessHint::Stale {}
            } else {
                StalenessHint::Fresh
            }),
            degraded_reason: status.degraded_reason.clone(),
            edge_kind: None,
            edge_resolution_state: None,
            edge_target_hint: None,
            edge_confidence_basis_points: None,
            edge_confidence_tier: None,
            score: declaration_score,
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
        staleness_hint: Some(if status.stale {
            StalenessHint::Stale {}
        } else {
            StalenessHint::Fresh
        }),
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

fn fallback_diagnostic(
    plan: &CodeGrepFallbackPlan,
    degraded_reason: Option<String>,
) -> Option<String> {
    let external_import_fallback =
        plan.kind == SourceGrepKind::Imports && !local_import_specifier(&plan.query);
    let reason = degraded_reason?;
    if external_import_fallback {
        Some(format!(
            "source fallback for unresolved external import failed: {reason}"
        ))
    } else {
        Some(reason)
    }
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
