use std::collections::BTreeSet;

use crate::{
    code::{SourceGrepMatch, simple_source_identifier, source_line_defines_identity},
    domain::{CodeRetrievalHit, CodeRetrievalLayer, CodeRetrievalRequest},
};

use super::source_surface::hit_has_complete_source_surface;

pub(super) fn hybrid_source_surface_fallback(
    request: &CodeRetrievalRequest,
    results: &[CodeRetrievalHit],
) -> Option<(String, Vec<String>)> {
    let query_terms = identifier_terms(&request.query);
    if query_terms.len() < 2 {
        return None;
    }
    let mut best: Option<(String, usize, usize)> = None;
    for hit in results {
        if !hit_allows_source_refresh(hit) {
            continue;
        }
        let identity = hit_identity(hit)?;
        let identity_terms = identifier_terms(&identity);
        if identity_terms.len() >= 2
            && identity_terms.len() < query_terms.len()
            && identity_terms.iter().all(|term| query_terms.contains(term))
        {
            let term_count = identity_terms.len();
            let identity_len = identity.len();
            if best.as_ref().is_none_or(|(_, best_terms, best_len)| {
                (term_count, identity_len) > (*best_terms, *best_len)
            }) {
                best = Some((identity, term_count, identity_len));
            }
        }
    }

    let (identity, _, _) = best?;
    let paths = incomplete_hybrid_source_surface_paths(results, &identity);
    (!paths.is_empty()).then_some((identity, paths))
}

pub(super) fn hybrid_exact_path_source_fallback(
    request: &CodeRetrievalRequest,
    results: &[CodeRetrievalHit],
) -> Option<(String, Vec<String>)> {
    let query = exact_path_hybrid_source_query(&request.query, results)?;
    let paths = request
        .repository
        .path_filters
        .iter()
        .filter(|path| exact_file_filter(path))
        .map(|path| normalize_filter_path(path).to_owned())
        .collect::<Vec<_>>();

    (!paths.is_empty()).then_some((query, paths))
}

pub(super) fn hit_allows_source_refresh(hit: &CodeRetrievalHit) -> bool {
    hit.retrieval_layers.contains(&CodeRetrievalLayer::Symbol)
        || hit
            .retrieval_layers
            .contains(&CodeRetrievalLayer::Definition)
        || hit
            .retrieval_layers
            .contains(&CodeRetrievalLayer::CallGraph)
}

pub(super) fn hit_source_line_is_better(
    hit: &CodeRetrievalHit,
    matched: &SourceGrepMatch,
    query: &str,
) -> bool {
    if source_type_declaration_line_matches_query(&matched.excerpt, query)
        && !source_type_declaration_line_matches_query(&hit.excerpt, query)
    {
        return true;
    }
    if matched.excerpt.contains(query) && !hit.excerpt.contains(query) {
        return true;
    }
    matched.excerpt.len() > hit.excerpt.len()
        || (matched.excerpt.contains("export ") && !hit.excerpt.contains("export "))
}

pub(super) fn exact_path_hybrid_source_line_score(
    request: &CodeRetrievalRequest,
    paths: &[String],
    matched: &SourceGrepMatch,
    lowest_score: Option<f64>,
) -> Option<f64> {
    if !paths.iter().any(|path| exact_file_filter(path))
        || line_query_term_match_count(&matched.excerpt, &request.query) == 0
    {
        return None;
    }

    let assignment_bonus = if source_line_has_assignment_surface(&matched.excerpt) {
        0.35
    } else {
        0.0
    };
    Some(lowest_score.unwrap_or(1.0) + 0.75 + assignment_bonus)
}

pub(super) fn source_type_declaration_line_matches_query(line: &str, query: &str) -> bool {
    query
        .split(|character: char| !source_identifier_char(character))
        .filter(|term| term.len() >= 3 && simple_source_identifier(term))
        .any(|term| source_type_declaration_line_defines_identity(line, term))
}

fn source_type_declaration_line_defines_identity(line: &str, identity: &str) -> bool {
    let line = line.trim();
    if !source_line_defines_identity(line, identity) {
        return false;
    }

    source_identifier_ranges(line, identity).any(|(start, _)| {
        line.get(..start)
            .is_some_and(declaration_keyword_before_identity)
    })
}

fn declaration_keyword_before_identity(before: &str) -> bool {
    before
        .split(|character: char| !source_identifier_char(character))
        .any(|token| {
            matches!(
                token,
                "class"
                    | "struct"
                    | "enum"
                    | "interface"
                    | "trait"
                    | "type"
                    | "typealias"
                    | "protocol"
            )
        })
}

fn line_query_term_match_count(line: &str, query: &str) -> usize {
    let line = line.to_ascii_lowercase();
    query
        .split(|character: char| !source_identifier_char(character))
        .filter(|term| term.len() >= 3 && simple_source_identifier(term))
        .filter(|term| line.contains(&term.to_ascii_lowercase()))
        .count()
}

fn source_line_has_assignment_surface(line: &str) -> bool {
    line.contains('=') || line.contains("=>") || line.contains(":=")
}

fn exact_path_hybrid_source_query(query: &str, results: &[CodeRetrievalHit]) -> Option<String> {
    let terms = ordered_identifier_terms(query);
    if terms.is_empty() {
        return None;
    }
    let primary = terms
        .iter()
        .enumerate()
        .filter_map(|(index, term)| {
            let score = results
                .iter()
                .map(|hit| {
                    code_surface_term_score(&hit.excerpt, term)
                        + hit
                            .canonical_symbol_id
                            .as_deref()
                            .map_or(0, |symbol_id| code_surface_term_score(symbol_id, term))
                })
                .sum::<usize>();
            (score > 0).then_some((score, index, term))
        })
        .max_by_key(|(score, index, _)| (*score, *index))
        .map(|(_, _, term)| term.clone())
        .unwrap_or_else(|| terms[0].clone());
    let mut query_terms = vec![primary.clone()];
    if let Some(support) = exact_path_identity_support_term(&terms, results, &primary) {
        query_terms.push(support);
    }

    Some(query_terms.join(" "))
}

fn exact_path_identity_support_term(
    terms: &[String],
    results: &[CodeRetrievalHit],
    primary: &str,
) -> Option<String> {
    terms
        .iter()
        .enumerate()
        .filter(|(_, term)| term.as_str() != primary)
        .filter_map(|(index, term)| {
            let score = incomplete_identity_support_score(results, term);
            (score > 0).then_some((score, index, term))
        })
        .max_by_key(|(score, index, _)| (*score, *index))
        .map(|(_, _, term)| term.clone())
}

fn incomplete_identity_support_score(results: &[CodeRetrievalHit], term: &str) -> usize {
    let term = term.to_ascii_lowercase();
    results
        .iter()
        .filter(|hit| hit_allows_source_refresh(hit))
        .filter(|hit| !hit.excerpt.to_ascii_lowercase().contains(&term))
        .filter(|hit| {
            hit.canonical_symbol_id
                .as_deref()
                .is_some_and(|identity| identifier_terms(identity).contains(&term))
        })
        .count()
}

fn ordered_identifier_terms(query: &str) -> Vec<String> {
    let mut terms = Vec::new();
    for term in query
        .split(|character: char| !source_identifier_char(character))
        .filter(|term| term.len() >= 3 && simple_source_identifier(term))
    {
        let term = term.to_owned();
        if !terms.contains(&term) {
            terms.push(term);
        }
    }

    terms
}

fn code_surface_term_score(text: &str, term: &str) -> usize {
    let text = text.to_ascii_lowercase();
    let term = term.to_ascii_lowercase();
    text.match_indices(&term)
        .filter(|(start, _)| identifier_match_has_boundary(&text, &term, *start))
        .map(|(start, _)| 1 + usize::from(term_has_code_separator_before(&text, start)))
        .sum()
}

fn identifier_match_has_boundary(text: &str, term: &str, start: usize) -> bool {
    let end = start + term.len();
    text.get(..start).is_some_and(|prefix| {
        prefix
            .chars()
            .next_back()
            .is_none_or(|character| !source_identifier_char(character))
    }) && text.get(end..).is_some_and(|suffix| {
        suffix
            .chars()
            .next()
            .is_none_or(|character| !source_identifier_char(character))
    })
}

fn term_has_code_separator_before(text: &str, start: usize) -> bool {
    text.get(..start)
        .and_then(|prefix| prefix.chars().next_back())
        .is_some_and(|character| matches!(character, '_' | '.' | ':' | '>' | '-'))
}

fn incomplete_hybrid_source_surface_paths(
    results: &[CodeRetrievalHit],
    identity: &str,
) -> Vec<String> {
    let mut paths = Vec::new();
    for hit in results {
        let own_identity = hit_identity(hit);
        if hit_allows_source_refresh(hit)
            && !hit_has_complete_source_surface(hit, identity)
            && own_identity.as_deref() != Some(identity)
            && !own_identity.as_deref().is_some_and(|own_identity| {
                hit_has_complete_source_surface(hit, own_identity)
                    && !canonical_symbol_has_parent_identity(hit, identity)
            })
        {
            push_candidate_path(&mut paths, &hit.path);
        }
    }
    paths
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

fn canonical_symbol_has_parent_identity(hit: &CodeRetrievalHit, identity: &str) -> bool {
    hit.canonical_symbol_id.as_deref().is_some_and(|symbol_id| {
        let mut parts = symbol_id
            .split(|character: char| !source_identifier_char(character))
            .filter(|part| !part.is_empty());
        parts.any(|part| part == identity) && parts.next().is_some()
    })
}

fn hit_identity(hit: &CodeRetrievalHit) -> Option<String> {
    hit.canonical_symbol_id
        .as_deref()
        .and_then(|symbol_id| {
            symbol_id
                .rsplit(|character: char| !source_identifier_char(character))
                .find(|term| !term.is_empty())
        })
        .or_else(|| {
            hit.excerpt
                .split(|character: char| !source_identifier_char(character))
                .find(|term| simple_source_identifier(term))
        })
        .map(str::to_owned)
}

fn identifier_terms(value: &str) -> BTreeSet<String> {
    let mut terms = BTreeSet::new();
    for token in value.split(|character: char| !source_identifier_char(character)) {
        if token.is_empty() {
            continue;
        }
        for term in split_identifier_token(token) {
            if term.len() > 1 {
                terms.insert(term.to_ascii_lowercase());
            }
        }
    }

    terms
}

fn split_identifier_token(token: &str) -> Vec<&str> {
    let mut terms = Vec::new();
    let mut start = 0usize;
    let mut previous_lowercase = false;
    for (index, character) in token.char_indices() {
        let boundary = index > start
            && (character == '_' || (character.is_ascii_uppercase() && previous_lowercase));
        if boundary {
            terms.push(token[start..index].trim_matches('_'));
            start = index + usize::from(character == '_');
        }
        previous_lowercase = character.is_ascii_lowercase() || character.is_ascii_digit();
    }
    if start < token.len() {
        terms.push(token[start..].trim_matches('_'));
    }

    terms.into_iter().filter(|term| !term.is_empty()).collect()
}

fn push_candidate_path(paths: &mut Vec<String>, path: &str) {
    let normalized = normalize_filter_path(path);
    if !normalized.is_empty() && !paths.iter().any(|existing| existing == normalized) {
        paths.push(normalized.to_owned());
    }
}

fn normalize_filter_path(path: &str) -> &str {
    let mut path = path.trim_end_matches(['/', '\\']);
    while let Some(stripped) = path.strip_prefix("./") {
        path = stripped;
    }

    path
}

fn source_identifier_ranges<'a>(
    line: &'a str,
    identity: &'a str,
) -> impl Iterator<Item = (usize, usize)> + 'a {
    line.match_indices(identity).filter_map(|(start, _)| {
        let end = start + identity.len();
        let has_start_boundary = line.get(..start).is_some_and(|prefix| {
            prefix
                .chars()
                .next_back()
                .is_none_or(|character| !source_identifier_char(character))
        });
        let has_end_boundary = line.get(end..).is_some_and(|suffix| {
            suffix
                .chars()
                .next()
                .is_none_or(|character| !source_identifier_char(character))
        });

        (has_start_boundary && has_end_boundary).then_some((start, end))
    })
}

fn source_identifier_char(character: char) -> bool {
    character.is_ascii_alphanumeric() || character == '_'
}
