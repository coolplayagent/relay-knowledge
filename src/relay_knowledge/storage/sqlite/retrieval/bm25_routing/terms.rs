use std::collections::BTreeMap;

use super::{super::local_model::stable_hash64, Bm25RoutingText};

const MAX_ROUTING_TERMS_PER_DOCUMENT: usize = 256;
const MAX_ROUTING_TERM_BYTES: usize = 128;

pub(super) struct TermInventory {
    pub(super) counts: Vec<(String, u32)>,
}

pub(super) fn topical_inventory(input: &Bm25RoutingText<'_>) -> TermInventory {
    inventory(input.source_path.into_iter().chain([
        input.entity_labels,
        input.entity_aliases,
        input.content,
    ]))
}

pub(super) fn indexed_inventory(input: &Bm25RoutingText<'_>) -> TermInventory {
    inventory(
        [input.source_scope]
            .into_iter()
            .chain(input.source_path)
            .chain([input.entity_labels, input.entity_aliases, input.content]),
    )
}

fn inventory<'a>(fields: impl IntoIterator<Item = &'a str>) -> TermInventory {
    let mut counts = BTreeMap::<String, u32>::new();
    for field in fields {
        for term in ascii_terms(field) {
            if let Some(count) = counts.get_mut(&term) {
                *count = count.saturating_add(1);
            } else if counts.len() < MAX_ROUTING_TERMS_PER_DOCUMENT {
                counts.insert(term, 1);
            }
        }
    }

    TermInventory {
        counts: counts.into_iter().collect(),
    }
}

pub(super) fn query_terms(query: &str) -> Option<Vec<String>> {
    if !query.is_ascii() {
        return None;
    }
    let mut terms = Vec::new();
    for term in query
        .split(|character: char| !character.is_ascii_alphanumeric())
        .filter(|term| !term.is_empty())
    {
        if term.len() > MAX_ROUTING_TERM_BYTES || terms.len() == 32 {
            return None;
        }
        terms.push(term.to_ascii_lowercase());
    }
    terms.sort();
    terms.dedup();
    if terms.is_empty() {
        return None;
    }
    Some(terms)
}

fn ascii_terms(value: &str) -> impl Iterator<Item = String> + '_ {
    value
        .split(|character: char| {
            character.is_whitespace()
                || (character.is_ascii() && !character.is_ascii_alphanumeric())
        })
        .filter(|term| {
            !term.is_empty()
                && term.len() <= MAX_ROUTING_TERM_BYTES
                && term.bytes().all(|byte| byte.is_ascii_alphanumeric())
        })
        .map(str::to_ascii_lowercase)
}

pub(super) fn simhash_prefix(inventory: &TermInventory, prefix_bits: u8) -> u16 {
    let mut weights = [0_i32; 64];
    for (term, frequency) in &inventory.counts {
        let hash = stable_hash64(term.as_bytes());
        let frequency = (*frequency).min(8) as i32;
        for (bit, weight) in weights.iter_mut().enumerate() {
            if hash & (1_u64 << bit) == 0 {
                *weight -= frequency;
            } else {
                *weight += frequency;
            }
        }
    }
    let fingerprint = weights
        .iter()
        .enumerate()
        .fold(0_u64, |value, (bit, weight)| {
            value | (u64::from(*weight >= 0) << bit)
        });
    (fingerprint >> (64 - prefix_bits)) as u16
}

#[cfg(test)]
#[path = "terms_tests.rs"]
mod tests;
