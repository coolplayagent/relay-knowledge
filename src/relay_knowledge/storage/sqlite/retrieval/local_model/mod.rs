//! Deterministic local semantic, vector, and lexical scoring primitives.

use std::collections::BTreeSet;

use crate::retrieval::terms::extend_normalized_terms;

pub(super) fn token_signature(
    content: &str,
    labels: &[String],
    source_path: Option<&str>,
) -> Vec<String> {
    let mut terms = BTreeSet::new();
    collect_terms(content, &mut terms);
    collect_terms(&labels.join(" "), &mut terms);
    collect_terms(source_path.unwrap_or_default(), &mut terms);

    terms.into_iter().collect()
}

fn collect_terms(value: &str, terms: &mut BTreeSet<String>) {
    extend_normalized_terms(value, 2, terms);
}

pub(super) fn hashed_vector(
    content: &str,
    labels: &[String],
    source_path: Option<&str>,
    dimension: usize,
) -> Vec<f64> {
    if dimension == 0 {
        return Vec::new();
    }
    let terms = token_signature(content, labels, source_path);
    let mut vector = vec![0.0; dimension];
    for term in terms {
        let hash = stable_hash64(term.as_bytes());
        let index = (hash as usize) % dimension;
        let sign = if hash & 1 == 0 { 1.0 } else { -1.0 };
        vector[index] += sign;
    }
    normalize_vector(&mut vector);

    vector
}

fn normalize_vector(vector: &mut [f64]) {
    let norm = vector.iter().map(|value| value * value).sum::<f64>().sqrt();
    if norm == 0.0 {
        return;
    }
    for value in vector {
        *value /= norm;
    }
}

pub(super) fn semantic_overlap_score(
    query_terms: &BTreeSet<String>,
    document_terms: &BTreeSet<String>,
) -> f64 {
    if query_terms.is_empty() || document_terms.is_empty() {
        return 0.0;
    }
    let intersection = query_terms.intersection(document_terms).count();
    if intersection == 0 {
        return 0.0;
    }
    let union = query_terms.union(document_terms).count();

    intersection as f64 / query_terms.len() as f64 + intersection as f64 / union as f64
}

pub(super) fn cosine_similarity(left: &[f64], right: &[f64]) -> f64 {
    if left.len() != right.len() || left.is_empty() {
        return 0.0;
    }
    left.iter()
        .zip(right.iter())
        .map(|(left, right)| left * right)
        .sum::<f64>()
        .max(0.0)
}

pub(super) fn overlap_score(
    query: &str,
    content: &str,
    labels: &[String],
    source_path: Option<&str>,
) -> f64 {
    let haystack = format!(
        "{} {} {}",
        content.to_lowercase(),
        labels.join(" ").to_lowercase(),
        source_path.unwrap_or_default().to_lowercase()
    );
    let mut score = 0.0;
    for token in query.to_lowercase().split_whitespace() {
        if haystack.contains(token) {
            score += 1.0;
        }
    }
    if score > 0.0 {
        return score;
    }

    identifier_overlap_score(query, content, labels, source_path)
}

fn identifier_overlap_score(
    query: &str,
    content: &str,
    labels: &[String],
    source_path: Option<&str>,
) -> f64 {
    let query_terms = token_signature(query, &[], None);
    let document_terms = token_signature(content, labels, source_path);
    query_terms
        .iter()
        .filter(|term| {
            let term = term.as_str();
            document_terms
                .iter()
                .any(|candidate| candidate == term || fuzzy_identifier_part_match(term, candidate))
        })
        .count() as f64
}

fn fuzzy_identifier_part_match(query_term: &str, candidate: &str) -> bool {
    query_term.len() >= 3 && candidate.len() >= 3 && candidate.contains(query_term)
}

pub(super) fn stable_hash64(bytes: &[u8]) -> u64 {
    const FNV_OFFSET_BASIS: u64 = 0xcbf29ce484222325;
    const FNV_PRIME: u64 = 0x100000001b3;

    let mut hash = FNV_OFFSET_BASIS;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(FNV_PRIME);
    }

    hash
}

#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;
