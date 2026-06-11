use std::collections::BTreeMap;

use rusqlite::Connection;

use crate::{
    domain::{
        CodeRepositorySelector, CodeRepositoryStatus, CodeRetrievalHit, CodeRetrievalLayer,
        CodeRetrievalRequest, RepositoryCodeRange, StalenessHint,
    },
    storage::StorageError,
};

use super::{
    code_query_scope::{
        language_filter_allows_path, path_filter_allows, selector_filters_fit_indexed_scope,
    },
    code_status::{repository_scope_status, repository_status},
};

pub(super) fn required_repository(
    connection: &mut Connection,
    selector: &CodeRepositorySelector,
) -> Result<CodeRepositoryStatus, StorageError> {
    let status = repository_status(connection, &selector.repository)?.ok_or_else(|| {
        StorageError::InvalidInput(format!(
            "code repository '{}' is not registered",
            selector.repository
        ))
    })?;
    let path_filters = merged_filters(&status.path_filters, &selector.path_filters);
    let language_filters = merged_filters(&status.language_filters, &selector.language_filters);
    let scoped_status = match repository_scope_status(
        connection,
        &selector.repository,
        &selector.ref_selector,
        &path_filters,
        &language_filters,
    )? {
        Some(status) => Some(status),
        None if (!selector.path_filters.is_empty() || !selector.language_filters.is_empty())
            && selector_filters_fit_indexed_scope(
                &status.path_filters,
                &status.language_filters,
                &selector.path_filters,
                &selector.language_filters,
            ) =>
        {
            repository_scope_status(
                connection,
                &selector.repository,
                &selector.ref_selector,
                &status.path_filters,
                &status.language_filters,
            )?
        }
        None => None,
    }
    .ok_or_else(|| {
        StorageError::InvalidInput(format!(
            "code repository '{}' has no index for ref {} and requested filters",
            selector.repository, selector.ref_selector
        ))
    })?;

    Ok(scoped_status)
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

pub(super) fn selected_row(
    path: &str,
    language_id: &str,
    is_generated: bool,
    status: &CodeRepositoryStatus,
    request: &CodeRetrievalRequest,
) -> bool {
    if request.exclude_generated && is_generated {
        return false;
    }
    path_filter_allows(path, &status.path_filters)
        && path_filter_allows(path, &request.repository.path_filters)
        && language_filter_allows_path(path, language_id, &status.language_filters)
        && language_filter_allows_path(path, language_id, &request.repository.language_filters)
        && language_filter_allows_path(path, language_id, &request.query_language_filters)
}

pub(super) fn chunk_layers(parse_status: &str) -> Vec<CodeRetrievalLayer> {
    let mut layers = vec![CodeRetrievalLayer::Lexical];
    if parse_status != "parsed" {
        layers.push(CodeRetrievalLayer::TextFallback);
    }

    layers
}

pub(super) struct HitParts {
    pub(super) path: String,
    pub(super) language_id: String,
    pub(super) byte_range: RepositoryCodeRange,
    pub(super) line_range: RepositoryCodeRange,
    pub(super) symbol_snapshot_id: Option<String>,
    pub(super) canonical_symbol_id: Option<String>,
    pub(super) file_id: Option<String>,
    pub(super) retrieval_layers: Vec<CodeRetrievalLayer>,
    pub(super) score: f64,
    pub(super) excerpt: String,
    pub(super) is_generated: bool,
    pub(super) degraded_reason: Option<String>,
    pub(super) edge_kind: Option<String>,
    pub(super) edge_resolution_state: Option<String>,
    pub(super) edge_target_hint: Option<String>,
    pub(super) edge_confidence_basis_points: Option<u16>,
    pub(super) edge_confidence_tier: Option<String>,
}

pub(super) fn hit_from_parts(status: &CodeRepositoryStatus, parts: HitParts) -> CodeRetrievalHit {
    CodeRetrievalHit {
        repository_id: status.repository_id.clone(),
        scope_id: status.last_indexed_scope_id.clone().unwrap_or_default(),
        resolved_commit_sha: status.last_indexed_commit.clone().unwrap_or_default(),
        tree_hash: status.tree_hash.clone().unwrap_or_default(),
        path: parts.path,
        language_id: parts.language_id,
        byte_range: parts.byte_range,
        line_range: parts.line_range,
        symbol_snapshot_id: parts.symbol_snapshot_id,
        canonical_symbol_id: parts.canonical_symbol_id,
        file_id: parts.file_id,
        retrieval_layers: parts.retrieval_layers,
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
        degraded_reason: parts.degraded_reason,
        edge_kind: parts.edge_kind,
        edge_resolution_state: parts.edge_resolution_state,
        edge_target_hint: parts.edge_target_hint,
        edge_confidence_basis_points: parts.edge_confidence_basis_points,
        edge_confidence_tier: parts.edge_confidence_tier,
        score: generated_adjusted_score(parts.score, parts.is_generated),
        excerpt: parts.excerpt,
    }
}

fn generated_adjusted_score(score: f64, is_generated: bool) -> f64 {
    const GENERATED_FILE_SCORE_MULTIPLIER: f64 = 0.35;

    if is_generated {
        score * GENERATED_FILE_SCORE_MULTIPLIER
    } else {
        score
    }
}

pub(super) fn required_scope(status: &CodeRepositoryStatus) -> Result<&str, StorageError> {
    status.last_indexed_scope_id.as_deref().ok_or_else(|| {
        StorageError::InvalidInput(format!(
            "code repository '{}' does not have an indexed source scope",
            status.alias
        ))
    })
}

pub(super) fn dedupe_sort_truncate(hits: &mut Vec<CodeRetrievalHit>, limit: usize) {
    let mut best = BTreeMap::<(String, u32, String), CodeRetrievalHit>::new();
    for hit in hits.drain(..) {
        let key = (hit.path.clone(), hit.line_range.start, hit.excerpt.clone());
        match best.get(&key) {
            Some(existing) if existing.score >= hit.score => {
                let existing = best.get_mut(&key).expect("checked entry should exist");
                merge_hit_provenance(existing, &hit);
            }
            Some(_) => {
                let mut hit = hit;
                if let Some(existing) = best.get(&key) {
                    merge_hit_provenance(&mut hit, existing);
                }
                best.insert(key, hit);
            }
            _ => {
                best.insert(key, hit);
            }
        }
    }
    hits.extend(best.into_values());
    hits.sort_by(|left, right| {
        right
            .score
            .total_cmp(&left.score)
            .then_with(|| left.path.cmp(&right.path))
            .then_with(|| left.line_range.start.cmp(&right.line_range.start))
    });
    hits.truncate(limit);
}

pub(super) fn filter_dedupe_sort_truncate(
    hits: &mut Vec<CodeRetrievalHit>,
    request: &CodeRetrievalRequest,
) {
    hits.retain(|hit| query_field_filters_allow_hit(hit, request));
    dedupe_sort_truncate(hits, request.limit);
}

pub(super) fn filtered_hits_for_gate(
    hits: &[CodeRetrievalHit],
    request: &CodeRetrievalRequest,
) -> Vec<CodeRetrievalHit> {
    let mut filtered = hits.to_vec();
    filter_dedupe_sort_truncate(&mut filtered, request);
    filtered
}

pub(super) fn query_field_filtered_hits_for_gate(
    hits: &[CodeRetrievalHit],
    request: &CodeRetrievalRequest,
) -> Vec<CodeRetrievalHit> {
    let mut filtered = hits.to_vec();
    filtered.retain(|hit| query_field_filters_allow_hit(hit, request));
    dedupe_sort_truncate(&mut filtered, usize::MAX);
    filtered
}

pub(super) fn has_query_field_hit_filters(request: &CodeRetrievalRequest) -> bool {
    !request.query_kind_filters.is_empty()
        || !request.query_path_substrings.is_empty()
        || !request.query_name_substrings.is_empty()
}

fn query_field_filters_allow_hit(hit: &CodeRetrievalHit, request: &CodeRetrievalRequest) -> bool {
    kind_filters_allow_hit(hit, &request.query_kind_filters)
        && path_substrings_allow_hit(&hit.path, &request.query_path_substrings)
        && name_substrings_allow_hit(hit, &request.query_name_substrings)
}

fn kind_filters_allow_hit(hit: &CodeRetrievalHit, filters: &[String]) -> bool {
    filters.is_empty()
        || (hit.retrieval_layers.contains(&CodeRetrievalLayer::Symbol)
            && hit.symbol_snapshot_id.is_some()
            && hit.canonical_symbol_id.is_some()
            && hit.edge_kind.is_none())
}

fn path_substrings_allow_hit(path: &str, filters: &[String]) -> bool {
    filters.is_empty()
        || filters
            .iter()
            .any(|filter| contains_ignore_ascii_case(path, filter))
}

fn name_substrings_allow_hit(hit: &CodeRetrievalHit, filters: &[String]) -> bool {
    filters.is_empty()
        || filters.iter().any(|filter| {
            hit.canonical_symbol_id
                .as_deref()
                .is_some_and(|symbol_id| contains_ignore_ascii_case(symbol_id, filter))
                || sbom_dependency_name(hit)
                    .is_some_and(|package_name| contains_ignore_ascii_case(package_name, filter))
        })
}

fn sbom_dependency_name(hit: &CodeRetrievalHit) -> Option<&str> {
    if hit.retrieval_layers.contains(&CodeRetrievalLayer::Sbom)
        && hit.edge_kind.as_deref() == Some("dependency")
    {
        hit.edge_target_hint.as_deref()
    } else {
        None
    }
}

fn contains_ignore_ascii_case(haystack: &str, needle: &str) -> bool {
    haystack
        .to_ascii_lowercase()
        .contains(&needle.to_ascii_lowercase())
}

pub(super) fn mark_hits_degraded(hits: &mut [CodeRetrievalHit], reason: &str) {
    for hit in hits {
        if hit.degraded_reason.is_none() {
            hit.degraded_reason = Some(reason.to_owned());
        }
    }
}

fn merge_hit_provenance(target: &mut CodeRetrievalHit, source: &CodeRetrievalHit) {
    target.stale |= source.stale
        || source
            .staleness_hint
            .as_ref()
            .is_some_and(StalenessHint::requires_source_verification);
    for layer in &source.retrieval_layers {
        if !target.retrieval_layers.contains(layer) {
            target.retrieval_layers.push(*layer);
        }
    }
    for version in &source.index_versions {
        if !target.index_versions.contains(version) {
            target.index_versions.push(version.clone());
        }
    }
    if target.degraded_reason.is_none() {
        target.degraded_reason = source.degraded_reason.clone();
    }
    if target.symbol_snapshot_id.is_none() {
        target.symbol_snapshot_id = source.symbol_snapshot_id.clone();
    }
    if target.canonical_symbol_id.is_none() {
        target.canonical_symbol_id = source.canonical_symbol_id.clone();
    }
    if target.file_id.is_none() {
        target.file_id = source.file_id.clone();
    }
    if target.edge_kind.is_none() {
        target.edge_kind = source.edge_kind.clone();
        target.edge_resolution_state = source.edge_resolution_state.clone();
        target.edge_target_hint = source.edge_target_hint.clone();
        target.edge_confidence_basis_points = source.edge_confidence_basis_points;
        target.edge_confidence_tier = source.edge_confidence_tier.clone();
    }
    if let Some(source_hint) = &source.staleness_hint {
        if source_hint.should_replace(target.staleness_hint.as_ref()) {
            target.staleness_hint = source.staleness_hint.clone();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{CodeRetrievalLayer, RepositoryCodeRange, StalenessHint};

    fn make_hit(staleness_hint: Option<StalenessHint>) -> CodeRetrievalHit {
        let r = RepositoryCodeRange { start: 0, end: 1 };
        CodeRetrievalHit {
            repository_id: String::new(),
            scope_id: String::new(),
            resolved_commit_sha: String::new(),
            tree_hash: String::new(),
            path: String::new(),
            language_id: String::new(),
            byte_range: r.clone(),
            line_range: r,
            symbol_snapshot_id: None,
            canonical_symbol_id: None,
            file_id: None,
            retrieval_layers: vec![CodeRetrievalLayer::Lexical],
            index_versions: vec![],
            stale: staleness_hint
                .as_ref()
                .is_some_and(StalenessHint::requires_source_verification),
            staleness_hint,
            degraded_reason: None,
            edge_kind: None,
            edge_resolution_state: None,
            edge_target_hint: None,
            edge_confidence_basis_points: None,
            edge_confidence_tier: None,
            score: 1.0,
            excerpt: String::new(),
        }
    }

    #[test]
    fn merge_prefers_stale_over_fresh() {
        let fresh_hit = make_hit(Some(StalenessHint::Fresh));
        let stale_hit = make_hit(Some(StalenessHint::Stale {}));
        let mut target = fresh_hit.clone();
        merge_hit_provenance(&mut target, &stale_hit);
        assert_eq!(target.staleness_hint, Some(StalenessHint::Stale {}));
        assert!(target.stale);
    }

    #[test]
    fn merge_keeps_stale_when_source_is_fresh() {
        let stale_hit = make_hit(Some(StalenessHint::Stale {}));
        let fresh_hit = make_hit(Some(StalenessHint::Fresh));
        let mut target = stale_hit.clone();
        merge_hit_provenance(&mut target, &fresh_hit);
        assert_eq!(target.staleness_hint, Some(StalenessHint::Stale {}));
        assert!(target.stale);
    }

    #[test]
    fn merge_fills_none_from_source() {
        let no_hint = make_hit(None);
        let fresh_hit = make_hit(Some(StalenessHint::Fresh));
        let mut target = no_hint.clone();
        merge_hit_provenance(&mut target, &fresh_hit);
        assert_eq!(target.staleness_hint, Some(StalenessHint::Fresh));
        assert!(!target.stale);
    }

    #[test]
    fn merge_preserves_none_when_both_none() {
        let a = make_hit(None);
        let b = make_hit(None);
        let mut target = a.clone();
        merge_hit_provenance(&mut target, &b);
        assert_eq!(target.staleness_hint, None);
        assert!(!target.stale);
    }

    #[test]
    fn merge_stale_bool_ors() {
        let fresh_hit = make_hit(Some(StalenessHint::Fresh));
        let stale_hit = make_hit(Some(StalenessHint::Stale {}));
        let mut target = fresh_hit.clone();
        assert!(!target.stale);
        merge_hit_provenance(&mut target, &stale_hit);
        assert!(target.stale);
    }

    #[test]
    fn merge_prefers_pending_index_over_stale() {
        let stale_hit = make_hit(Some(StalenessHint::Stale {}));
        let pending_hit = make_hit(Some(StalenessHint::PendingIndex {}));
        let mut target = stale_hit.clone();
        merge_hit_provenance(&mut target, &pending_hit);
        assert_eq!(target.staleness_hint, Some(StalenessHint::PendingIndex {}));
        assert!(target.stale);
    }
}
