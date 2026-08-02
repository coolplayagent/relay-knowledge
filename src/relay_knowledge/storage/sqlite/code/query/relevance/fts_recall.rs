//! Hybrid-chunk recall selection, member-access recovery, and path rejection.

use super::fts_terms::{
    MAX_HYBRID_CHUNK_RECALL_ANCHORS, MIN_HIGH_SIGNAL_TERM_PRIORITY,
    append_type_surface_companion_terms, hybrid_chunk_term_priority, identifier_term_has_structure,
    push_case_insensitive_unique_term,
};

pub(super) const MAX_HYBRID_CHUNK_RECALL_TERMS: usize = 6;
const MIN_API_DENSE_HIGH_SIGNAL_TERMS: usize = 3;
const MAX_API_DENSE_UNSTRUCTURED_TERMS: usize = 1;
const STRICT_HYBRID_CHUNK_MIN_STRUCTURED_TERMS: usize = 2;
const STRICT_HYBRID_CHUNK_MAX_TERMS: usize = 3;

pub(super) fn hybrid_chunk_recall_terms(terms: &[String]) -> Vec<String> {
    if api_dense_hybrid_query(terms) {
        let mut recall_terms = high_signal_hybrid_chunk_recall_terms(terms);
        append_type_surface_companion_terms(terms, &mut recall_terms);
        return recall_terms;
    }

    let mut recall_terms = leading_hybrid_chunk_recall_anchors(terms);
    let mut ranked = terms
        .iter()
        .enumerate()
        .map(|(position, term)| (hybrid_chunk_term_priority(term), position, term))
        .collect::<Vec<_>>();
    ranked.sort_by(|left, right| {
        right
            .0
            .cmp(&left.0)
            .then_with(|| left.1.cmp(&right.1))
            .then_with(|| left.2.cmp(right.2))
    });
    for (priority, _, term) in ranked {
        if recall_terms.len() >= MAX_HYBRID_CHUNK_RECALL_TERMS {
            break;
        }
        if priority < 2 {
            continue;
        }
        push_case_insensitive_unique_term(&mut recall_terms, term);
    }
    append_type_surface_companion_terms(terms, &mut recall_terms);

    recall_terms
}

pub(super) fn api_dense_hybrid_query(terms: &[String]) -> bool {
    let mut high_signal_terms = 0usize;
    let mut has_structured_term = false;
    for term in terms {
        let structured = identifier_term_has_structure(term);
        has_structured_term |= structured;
        if hybrid_chunk_term_priority(term) >= MIN_HIGH_SIGNAL_TERM_PRIORITY {
            high_signal_terms += 1;
        }
    }

    has_structured_term && high_signal_terms >= MIN_API_DENSE_HIGH_SIGNAL_TERMS
}

fn high_signal_hybrid_chunk_recall_terms(terms: &[String]) -> Vec<String> {
    let mut ranked = terms
        .iter()
        .enumerate()
        .map(|(position, term)| {
            (
                identifier_term_has_structure(term),
                hybrid_chunk_term_priority(term),
                position,
                term,
            )
        })
        .collect::<Vec<_>>();
    ranked.sort_by(|left, right| {
        right
            .0
            .cmp(&left.0)
            .then_with(|| right.1.cmp(&left.1))
            .then_with(|| left.2.cmp(&right.2))
            .then_with(|| left.3.cmp(right.3))
    });

    let mut recall_terms = Vec::new();
    let mut unstructured_terms = 0usize;
    for (structured, priority, _, term) in ranked {
        if recall_terms.len() >= MAX_HYBRID_CHUNK_RECALL_TERMS {
            break;
        }
        if priority < MIN_HIGH_SIGNAL_TERM_PRIORITY {
            continue;
        }
        if !structured {
            if unstructured_terms >= MAX_API_DENSE_UNSTRUCTURED_TERMS {
                continue;
            }
            unstructured_terms += 1;
        }
        push_case_insensitive_unique_term(&mut recall_terms, term);
    }

    recall_terms
}

pub(super) fn strict_hybrid_chunk_recall_terms(query: &str, terms: &[String]) -> Vec<String> {
    let mut ranked = terms
        .iter()
        .enumerate()
        .filter(|(_, term)| identifier_term_has_structure(term))
        .filter(|(_, term)| hybrid_chunk_term_priority(term) >= MIN_HIGH_SIGNAL_TERM_PRIORITY)
        .map(|(position, term)| (hybrid_chunk_term_priority(term), position, term))
        .collect::<Vec<_>>();
    ranked.sort_by(|left, right| {
        right
            .0
            .cmp(&left.0)
            .then_with(|| left.1.cmp(&right.1))
            .then_with(|| left.2.cmp(right.2))
    });

    let mut recall_terms = Vec::new();
    for (_, _, term) in ranked {
        if recall_terms.len() >= STRICT_HYBRID_CHUNK_MAX_TERMS {
            break;
        }
        push_case_insensitive_unique_term(&mut recall_terms, term);
    }
    if recall_terms.len() < STRICT_HYBRID_CHUNK_MIN_STRUCTURED_TERMS {
        for term in member_access_leaf_terms(query) {
            if recall_terms.len() >= STRICT_HYBRID_CHUNK_MIN_STRUCTURED_TERMS {
                break;
            }
            push_case_insensitive_unique_term(&mut recall_terms, &term);
        }
    }

    recall_terms
}

pub(super) fn strict_member_access_recall_allowed(query: &str, recall_terms: &[String]) -> bool {
    let member_leaves = member_access_leaf_terms(query);
    !member_leaves.is_empty()
        && recall_terms.iter().any(|term| {
            identifier_term_has_structure(term)
                && hybrid_chunk_term_priority(term) >= MIN_HIGH_SIGNAL_TERM_PRIORITY
        })
        && member_leaves.iter().any(|leaf| {
            recall_terms
                .iter()
                .any(|term| term.eq_ignore_ascii_case(leaf))
        })
}

fn member_access_leaf_terms(query: &str) -> Vec<String> {
    let mut terms: Vec<String> = Vec::new();
    for raw_token in query.split_whitespace().map(str::trim) {
        let token = raw_token.trim_matches(|character: char| {
            !(character.is_ascii_alphanumeric() || matches!(character, '_' | '.' | ':'))
        });
        if token.is_empty()
            || token.contains('/')
            || token.contains('\\')
            || token_has_path_like_extension(token)
            || !(token.contains('.') || token.contains("::"))
        {
            continue;
        }
        let Some(leaf) = token
            .rsplit(|character: char| !(character.is_ascii_alphanumeric() || character == '_'))
            .find(|term| !term.is_empty())
        else {
            continue;
        };
        if leaf.len() >= 4
            && leaf
                .chars()
                .all(|character| character.is_ascii_alphanumeric() || character == '_')
            && !terms
                .iter()
                .any(|existing| existing.eq_ignore_ascii_case(leaf))
        {
            terms.push(leaf.to_owned());
        }
    }

    terms
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

fn leading_hybrid_chunk_recall_anchors(terms: &[String]) -> Vec<String> {
    let mut anchors = Vec::new();
    for term in terms {
        if anchors.len() >= MAX_HYBRID_CHUNK_RECALL_ANCHORS {
            break;
        }
        if leading_hybrid_chunk_anchor(term) {
            push_case_insensitive_unique_term(&mut anchors, term);
        }
    }

    anchors
}

fn leading_hybrid_chunk_anchor(term: &str) -> bool {
    let length = term.chars().count();
    (4..=16).contains(&length)
        && term
            .chars()
            .all(|character| character.is_ascii_lowercase() || character.is_ascii_digit())
}

#[cfg(test)]
#[path = "fts_recall_tests.rs"]
mod tests;
