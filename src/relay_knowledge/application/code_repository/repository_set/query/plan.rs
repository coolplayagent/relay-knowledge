use std::collections::BTreeMap;

use crate::domain::{
    CodeQueryKind, CodeRepositorySetMemberStatus, CodeRepositorySetQueryRequest, CodeRetrievalHit,
    CodeRetrievalLayer, StalenessHint,
};

const MIN_DEPENDENCY_SYMBOL_PLAN_IDENTITIES: usize = 2;

pub(super) struct RepositorySetMemberQueryPlan {
    pub(super) query: String,
    pub(super) kind: CodeQueryKind,
}

pub(super) fn repository_set_member_query_plan(
    request: &CodeRepositorySetQueryRequest,
    member: &CodeRepositorySetMemberStatus,
    highest_priority: i32,
) -> RepositorySetMemberQueryPlan {
    let kind = repository_set_member_query_kind(request, member, highest_priority);
    let query = if kind == CodeQueryKind::Symbol {
        dependency_symbol_query(&request.query).unwrap_or_else(|| request.query.clone())
    } else {
        request.query.clone()
    };

    RepositorySetMemberQueryPlan { query, kind }
}

fn repository_set_member_query_kind(
    request: &CodeRepositorySetQueryRequest,
    member: &CodeRepositorySetMemberStatus,
    highest_priority: i32,
) -> CodeQueryKind {
    if request.code_query_kind != CodeQueryKind::Hybrid
        || member.member.priority >= highest_priority
        || api_query_identity_leaves(&request.query).len() < MIN_DEPENDENCY_SYMBOL_PLAN_IDENTITIES
    {
        request.code_query_kind
    } else {
        CodeQueryKind::Symbol
    }
}

fn dependency_symbol_query(query: &str) -> Option<String> {
    let identities = api_query_identities(query);
    if identities.len() < MIN_DEPENDENCY_SYMBOL_PLAN_IDENTITIES {
        return None;
    }

    let mut terms = Vec::new();
    for identity in identities {
        push_unique_query_term(&mut terms, &identity.raw);
        if !identity.raw.eq_ignore_ascii_case(&identity.leaf) {
            push_unique_query_term(&mut terms, &identity.leaf);
        }
    }

    (!terms.is_empty()).then(|| terms.join(" "))
}

fn push_unique_query_term(terms: &mut Vec<String>, term: &str) {
    if !terms
        .iter()
        .any(|existing| existing.eq_ignore_ascii_case(term))
    {
        terms.push(term.to_owned());
    }
}

pub(super) fn dependency_symbol_plan_needs_hybrid_fallback(
    request: &CodeRepositorySetQueryRequest,
    member_query_kind: CodeQueryKind,
    hits: &[CodeRetrievalHit],
) -> bool {
    if request.code_query_kind != CodeQueryKind::Hybrid
        || member_query_kind != CodeQueryKind::Symbol
    {
        return false;
    }

    let identities = api_query_identity_leaves(&request.query);
    identity_symbol_hit_coverage(&identities, hits) < MIN_DEPENDENCY_SYMBOL_PLAN_IDENTITIES
}

pub(super) fn merge_dependency_symbol_fallback_hits(
    symbol_hits: Vec<CodeRetrievalHit>,
    fallback_hits: Vec<CodeRetrievalHit>,
) -> Vec<CodeRetrievalHit> {
    let mut merged =
        BTreeMap::<(String, String, String, u32, u32, String), CodeRetrievalHit>::new();
    for hit in fallback_hits.into_iter().chain(symbol_hits) {
        let key = (
            hit.repository_id.clone(),
            hit.scope_id.clone(),
            hit.path.clone(),
            hit.line_range.start,
            hit.line_range.end,
            hit.excerpt.clone(),
        );
        match merged.get_mut(&key) {
            Some(existing) => merge_duplicate_hit(existing, hit),
            None => {
                merged.insert(key, hit);
            }
        }
    }

    merged.into_values().collect()
}

fn merge_duplicate_hit(existing: &mut CodeRetrievalHit, candidate: CodeRetrievalHit) {
    if candidate.score > existing.score {
        let previous = std::mem::replace(existing, candidate);
        merge_hit_metadata(existing, previous);
    } else {
        merge_hit_metadata(existing, candidate);
    }
}

fn merge_hit_metadata(target: &mut CodeRetrievalHit, mut source: CodeRetrievalHit) {
    target.stale |= source.stale
        || source
            .staleness_hint
            .as_ref()
            .is_some_and(StalenessHint::requires_source_verification);
    for layer in source.retrieval_layers {
        if !target.retrieval_layers.contains(&layer) {
            target.retrieval_layers.push(layer);
        }
    }
    for version in source.index_versions {
        if !target.index_versions.contains(&version) {
            target.index_versions.push(version);
        }
    }
    if target.symbol_snapshot_id.is_none() {
        target.symbol_snapshot_id = source.symbol_snapshot_id.take();
    }
    if target.canonical_symbol_id.is_none() {
        target.canonical_symbol_id = source.canonical_symbol_id.take();
    }
    if target.file_id.is_none() {
        target.file_id = source.file_id.take();
    }
    if target.degraded_reason.is_none() {
        target.degraded_reason = source.degraded_reason.take();
    }
    if target.edge_kind.is_none() {
        target.edge_kind = source.edge_kind.take();
    }
    if target.edge_resolution_state.is_none() {
        target.edge_resolution_state = source.edge_resolution_state.take();
    }
    if target.edge_target_hint.is_none() {
        target.edge_target_hint = source.edge_target_hint.take();
    }
    if target.edge_confidence_basis_points.is_none() {
        target.edge_confidence_basis_points = source.edge_confidence_basis_points;
    }
    if target.edge_confidence_tier.is_none() {
        target.edge_confidence_tier = source.edge_confidence_tier.take();
    }
    if let Some(source_hint) = &source.staleness_hint {
        if source_hint.should_replace(target.staleness_hint.as_ref()) {
            target.staleness_hint = source.staleness_hint.take();
        }
    }
}

fn identity_symbol_hit_coverage(identities: &[String], hits: &[CodeRetrievalHit]) -> usize {
    identities
        .iter()
        .filter(|identity| {
            hits.iter()
                .any(|hit| symbol_hit_matches_identity(hit, identity))
        })
        .count()
}

fn symbol_hit_matches_identity(hit: &CodeRetrievalHit, identity: &str) -> bool {
    hit.retrieval_layers.contains(&CodeRetrievalLayer::Symbol)
        && (hit
            .canonical_symbol_id
            .as_deref()
            .is_some_and(|symbol_id| canonical_symbol_leaf_matches(symbol_id, identity))
            || text_contains_identifier(&hit.excerpt, identity))
}

pub(super) fn api_query_identity_leaves(query: &str) -> Vec<String> {
    let mut identities = Vec::new();
    for identity in api_query_identities(query) {
        if !identities.contains(&identity.leaf) {
            identities.push(identity.leaf);
        }
    }

    identities
}

struct ApiQueryIdentity {
    raw: String,
    leaf: String,
}

fn api_query_identities(query: &str) -> Vec<ApiQueryIdentity> {
    let mut identities = Vec::new();
    for raw_token in query.split_whitespace().map(str::trim) {
        let Some(identity) = api_query_identity(raw_token) else {
            continue;
        };
        if !identities
            .iter()
            .any(|existing: &ApiQueryIdentity| existing.raw.eq_ignore_ascii_case(&identity.raw))
        {
            identities.push(identity);
        }
    }

    identities
}

fn api_query_identity(token: &str) -> Option<ApiQueryIdentity> {
    let token = token.trim_matches(|character: char| {
        !(character.is_ascii_alphanumeric() || matches!(character, '_' | '.' | ':'))
    });
    if token.is_empty()
        || token.contains('/')
        || token.contains('\\')
        || token_has_path_like_extension(token)
    {
        return None;
    }
    if token.contains('.') || token.contains("::") {
        let leaf = identity_terms(token).last().cloned()?;
        return Some(ApiQueryIdentity {
            raw: token.to_owned(),
            leaf,
        });
    }
    simple_api_identity_token(token).then(|| ApiQueryIdentity {
        raw: token.to_owned(),
        leaf: token.to_owned(),
    })
}

fn identity_terms(value: &str) -> Vec<String> {
    value
        .split(|character: char| !(character.is_ascii_alphanumeric() || character == '_'))
        .filter(|term| !term.is_empty())
        .map(str::to_owned)
        .collect()
}

fn simple_api_identity_token(token: &str) -> bool {
    token.len() >= 4
        && token
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || character == '_')
        && token_has_case_boundary(token)
}

fn token_has_case_boundary(token: &str) -> bool {
    let mut previous = None;
    token.chars().any(|character| {
        let boundary = character.is_ascii_uppercase()
            && previous.is_some_and(|previous: char| previous.is_ascii_lowercase());
        previous = Some(character);
        boundary
    })
}

fn token_has_path_like_extension(token: &str) -> bool {
    let Some((stem, extension)) = token.rsplit_once('.') else {
        return false;
    };
    !stem.is_empty() && file_extension_is_path_like(extension)
}

fn file_extension_is_path_like(extension: &str) -> bool {
    matches!(
        extension.to_ascii_lowercase().as_str(),
        "c" | "cc"
            | "cpp"
            | "cs"
            | "go"
            | "gradle"
            | "h"
            | "hh"
            | "hpp"
            | "hxx"
            | "java"
            | "js"
            | "json"
            | "jsx"
            | "kt"
            | "md"
            | "php"
            | "py"
            | "rb"
            | "rs"
            | "scala"
            | "sh"
            | "swift"
            | "ts"
            | "tsx"
            | "txt"
            | "xml"
            | "yaml"
            | "yml"
    )
}

fn canonical_symbol_leaf_matches(canonical_symbol_id: &str, identity: &str) -> bool {
    canonical_symbol_id
        .rsplit(|character: char| !(character.is_ascii_alphanumeric() || character == '_'))
        .find(|term| !term.is_empty())
        .is_some_and(|leaf| leaf == identity)
}

fn text_contains_identifier(text: &str, identity: &str) -> bool {
    text.match_indices(identity).any(|(start, _)| {
        let end = start + identity.len();
        text.get(..start).is_some_and(|prefix| {
            prefix
                .chars()
                .next_back()
                .is_none_or(|character| !is_identifier_char(character))
        }) && text
            .get(end..)
            .is_some_and(|suffix| suffix.chars().next().is_none_or(|c| !is_identifier_char(c)))
    })
}

fn is_identifier_char(character: char) -> bool {
    character.is_ascii_alphanumeric() || character == '_'
}

#[cfg(test)]
#[path = "plan_tests.rs"]
mod tests;
