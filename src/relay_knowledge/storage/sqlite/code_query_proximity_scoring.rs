use std::collections::BTreeSet;

use crate::domain::{CodeQueryKind, CodeRetrievalRequest};

const QUERY_PROXIMITY_WINDOW_LINES: usize = 6;
const QUERY_PROXIMITY_MAX_NONBLANK_LINES: usize = 80;
const QUERY_PROXIMITY_MIN_BASE_SCORE: f64 = 6.0;
const QUERY_PROXIMITY_MIN_MATCHED_TERMS: usize = 5;

pub(super) fn query_proximity_chunk_bonus(
    base_score: f64,
    query: &str,
    content: &str,
    path: &str,
    request: &CodeRetrievalRequest,
) -> f64 {
    if base_score < QUERY_PROXIMITY_MIN_BASE_SCORE
        || request.code_query_kind != CodeQueryKind::Hybrid
        || (path_looks_like_test_or_benchmark(path) && !query_mentions_test_or_benchmark(query))
    {
        return 0.0;
    }

    let query_terms = meaningful_terms(query);
    if query_terms.len() < QUERY_PROXIMITY_MIN_MATCHED_TERMS {
        return 0.0;
    }
    if query_api_identity_count(query) >= 3 {
        return 0.0;
    }
    if nonblank_line_count(content) > QUERY_PROXIMITY_MAX_NONBLANK_LINES {
        return 0.0;
    }
    let required_matches =
        QUERY_PROXIMITY_MIN_MATCHED_TERMS.max(query_terms.len().saturating_mul(3).div_ceil(4));
    let best_match_count = proximity_windows(content)
        .iter()
        .map(|window| matched_query_terms(&query_terms, window).len())
        .max()
        .unwrap_or(0);
    if best_match_count < required_matches {
        return 0.0;
    }

    let coverage = best_match_count as f64 / query_terms.len() as f64;
    (0.45 + coverage * 1.45).min(1.9)
}

fn nonblank_line_count(content: &str) -> usize {
    content
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .count()
}

fn proximity_windows(content: &str) -> Vec<Vec<String>> {
    let lines = content
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(identifier_terms)
        .filter(|terms| !terms.is_empty())
        .collect::<Vec<_>>();
    let mut windows = Vec::new();
    for start in 0..lines.len() {
        let mut terms = BTreeSet::new();
        for line_terms in lines.iter().skip(start).take(QUERY_PROXIMITY_WINDOW_LINES) {
            terms.extend(line_terms.iter().cloned());
        }
        windows.push(terms.into_iter().collect());
    }

    windows
}

fn matched_query_terms(query_terms: &[String], content_terms: &[String]) -> Vec<String> {
    let mut matched = Vec::new();
    for query_term in query_terms {
        if matched.contains(query_term) {
            continue;
        }
        if content_terms
            .iter()
            .any(|content_term| related_identifier_terms(content_term, query_term))
        {
            matched.push(query_term.clone());
        }
    }
    matched
}

fn meaningful_terms(value: &str) -> Vec<String> {
    let mut terms = identifier_terms(value)
        .into_iter()
        .filter(|term| term.len() >= 3)
        .filter(|term| !matches!(term.as_str(), "the" | "and" | "for" | "with" | "from"))
        .collect::<Vec<_>>();
    terms.sort();
    terms.dedup();
    terms
}

fn identifier_terms(value: &str) -> Vec<String> {
    let mut terms = Vec::new();
    for raw in value
        .split(|character: char| !(character.is_ascii_alphanumeric() || character == '_'))
        .filter(|term| !term.is_empty())
    {
        terms.push(raw.to_ascii_lowercase());
        terms.extend(
            raw.split('_')
                .filter(|part| !part.is_empty())
                .map(str::to_ascii_lowercase),
        );
        push_camel_terms(raw, &mut terms);
    }
    terms.sort();
    terms.dedup();
    terms
}

fn push_camel_terms(token: &str, terms: &mut Vec<String>) {
    let chars = token.char_indices().collect::<Vec<_>>();
    if chars.is_empty() {
        return;
    }

    let mut start = 0;
    for index in 1..chars.len() {
        let (byte_index, character) = chars[index];
        let previous = chars[index - 1].1;
        let next = chars.get(index + 1).map(|(_, next)| *next);
        let starts_upper_word = character.is_ascii_uppercase()
            && (previous.is_ascii_lowercase()
                || previous.is_ascii_digit()
                || next.is_some_and(|next| next.is_ascii_lowercase()));
        if starts_upper_word {
            terms.push(token[start..byte_index].to_ascii_lowercase());
            start = byte_index;
        }
    }
    if start < token.len() {
        terms.push(token[start..].to_ascii_lowercase());
    }
}

fn related_identifier_terms(left: &str, right: &str) -> bool {
    left == right
        || (left.len() >= 4
            && right.len() >= 4
            && (left.starts_with(right) || right.starts_with(left)))
}

fn path_looks_like_test_or_benchmark(path: &str) -> bool {
    path.to_ascii_lowercase().split('/').any(|segment| {
        matches!(
            segment,
            "test" | "tests" | "__tests__" | "testing" | "bench" | "benchmark" | "benchmarks"
        ) || segment.ends_with("_test")
            || segment.ends_with(".test.ts")
            || segment.ends_with(".test.tsx")
            || segment.ends_with(".spec.ts")
            || segment.ends_with(".spec.tsx")
    })
}

fn query_mentions_test_or_benchmark(query: &str) -> bool {
    meaningful_terms(query).iter().any(|term| {
        matches!(
            term.as_str(),
            "test" | "tests" | "testing" | "bench" | "benchmark" | "benchmarks"
        )
    })
}

fn query_api_identity_count(query: &str) -> usize {
    query
        .split_whitespace()
        .filter(|token| token_looks_like_api_identity(token))
        .count()
}

fn token_looks_like_api_identity(token: &str) -> bool {
    let token = token.trim_matches(|character: char| {
        !(character.is_ascii_alphanumeric() || matches!(character, '_' | '.' | ':'))
    });
    if token.contains('.') || token.contains("::") {
        return true;
    }

    let mut previous = None;
    token.chars().any(|character| {
        let boundary = character.is_ascii_uppercase()
            && previous.is_some_and(|prev: char| prev.is_ascii_lowercase());
        previous = Some(character);
        boundary
    })
}
