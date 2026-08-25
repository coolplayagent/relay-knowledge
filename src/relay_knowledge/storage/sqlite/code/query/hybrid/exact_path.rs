use crate::domain::{CodeQueryKind, CodeRetrievalHit, CodeRetrievalRequest};

use super::super::relevance::query_is_single_symbol_identity;
use super::planning::{hybrid_query_requires_chunk_first_before_symbols, hybrid_sequence_terms};

pub(in super::super) fn hybrid_exact_path_query_can_defer_to_source_fallback(
    request: &CodeRetrievalRequest,
    hits: &[CodeRetrievalHit],
) -> bool {
    query_is_single_symbol_identity(&request.query)
        && hybrid_query_can_skip_graph_expansion(request, hits)
        && exact_path_hits_cover_query_identities(&request.query, hits)
        && !hybrid_query_mentions_type_surface(&request.query)
}

pub(in super::super) fn hybrid_query_can_skip_graph_expansion(
    request: &CodeRetrievalRequest,
    hits: &[CodeRetrievalHit],
) -> bool {
    request.code_query_kind == CodeQueryKind::Hybrid
        && request_has_exact_file_filter(request)
        && !hits.is_empty()
        && !hybrid_query_mentions_graph_expansion(&request.query)
}

pub(in super::super) fn hybrid_exact_path_query_should_skip_chunk_first(
    request: &CodeRetrievalRequest,
) -> bool {
    request.code_query_kind == CodeQueryKind::Hybrid
        && request_has_exact_file_filter(request)
        && query_is_single_symbol_identity(&request.query)
        && !hybrid_query_mentions_graph_expansion(&request.query)
}

pub(in super::super) fn hybrid_query_should_use_layered_chunk_search(
    request: &CodeRetrievalRequest,
) -> bool {
    request.code_query_kind == CodeQueryKind::Hybrid
        && !hybrid_exact_path_query_should_skip_chunk_first(request)
        && (hybrid_query_requires_chunk_first_before_symbols(request)
            || hybrid_query_mentions_graph_expansion(&request.query))
}

pub(in super::super) fn request_has_exact_file_filter(request: &CodeRetrievalRequest) -> bool {
    request
        .repository
        .path_filters
        .iter()
        .any(|path| exact_file_filter(path))
}

fn hybrid_query_mentions_graph_expansion(query: &str) -> bool {
    hybrid_sequence_terms(query).iter().any(|term| {
        matches!(
            term.as_str(),
            "call"
                | "calls"
                | "caller"
                | "callers"
                | "callee"
                | "callees"
                | "dependencies"
                | "dependency"
                | "execution"
                | "inheritance"
                | "inherits"
                | "reference"
                | "references"
                | "referenced"
                | "import"
                | "imports"
                | "importer"
                | "importers"
        )
    })
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

fn exact_path_hits_cover_query_identities(query: &str, hits: &[CodeRetrievalHit]) -> bool {
    let identities = identifier_like_query_terms(query);
    identities.is_empty()
        || identities
            .iter()
            .all(|identity| hits.iter().any(|hit| hit_mentions_identity(hit, identity)))
}

fn hybrid_query_mentions_type_surface(query: &str) -> bool {
    hybrid_sequence_terms(query).iter().any(|term| {
        matches!(
            term.as_str(),
            "class"
                | "struct"
                | "interface"
                | "interfaces"
                | "trait"
                | "traits"
                | "public"
                | "extends"
                | "implements"
                | "override"
                | "overrides"
        )
    })
}

fn identifier_like_query_terms(query: &str) -> Vec<String> {
    let mut identities = Vec::new();
    for raw_token in query.split_whitespace().map(str::trim) {
        if raw_token.contains('/') || raw_token.contains('\\') {
            continue;
        }
        for term in raw_token
            .split(|character: char| !(character.is_ascii_alphanumeric() || character == '_'))
            .filter(|term| term.len() >= 3)
        {
            if identifier_like_query_term(term)
                && !identities
                    .iter()
                    .any(|existing: &String| existing.eq_ignore_ascii_case(term))
            {
                identities.push(term.to_owned());
            }
        }
    }

    identities
}

fn identifier_like_query_term(term: &str) -> bool {
    term.contains('_') || camel_case_boundary_count(term) > 0 || capitalized_identifier(term)
}

fn camel_case_boundary_count(term: &str) -> usize {
    let mut previous: Option<char> = None;
    let chars = term.chars().collect::<Vec<_>>();
    let mut boundaries = 0usize;
    for (index, character) in chars.iter().enumerate() {
        let next = chars.get(index + 1).copied();
        let starts_upper_word = character.is_ascii_uppercase()
            && previous.is_some_and(|previous| {
                previous.is_ascii_lowercase()
                    || previous.is_ascii_digit()
                    || next.is_some_and(|next| next.is_ascii_lowercase())
            });
        if starts_upper_word {
            boundaries += 1;
        }
        previous = Some(*character);
    }

    boundaries
}

fn capitalized_identifier(term: &str) -> bool {
    let mut chars = term.chars();
    chars.next().is_some_and(|first| first.is_ascii_uppercase())
        && chars.any(|character| character.is_ascii_lowercase())
}

fn hit_mentions_identity(hit: &CodeRetrievalHit, identity: &str) -> bool {
    let identity = identity.to_ascii_lowercase();
    text_mentions_identity(&hit.excerpt, &identity)
        || hit
            .canonical_symbol_id
            .as_deref()
            .is_some_and(|symbol| text_mentions_identity(symbol, &identity))
}

fn text_mentions_identity(text: &str, identity: &str) -> bool {
    let text = text.to_ascii_lowercase();
    text.contains(identity)
        || compact_identifier_text(&text).contains(&compact_identifier_text(identity))
}

fn compact_identifier_text(text: &str) -> String {
    text.chars()
        .filter(|character| character.is_ascii_alphanumeric())
        .collect()
}

#[cfg(test)]
#[path = "exact_path_tests.rs"]
mod tests;
