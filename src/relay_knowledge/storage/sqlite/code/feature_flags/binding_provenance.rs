//! Declaration evidence follows the bindings traversed by resolved reads, not equal values.
use std::collections::{BTreeMap, BTreeSet};

use crate::domain::CodeFeatureFlagRecord;

pub(super) fn used_symbols(
    records: &[CodeFeatureFlagRecord],
    states: &[String],
) -> BTreeMap<(String, String), BTreeSet<String>> {
    let mut edges = BTreeMap::<&str, BTreeSet<&str>>::new();
    let mut used = BTreeMap::<(String, String), BTreeSet<String>>::new();
    for (record, state) in records.iter().zip(states) {
        let Some(reference) = record.metadata.referenced_symbol.as_deref() else {
            continue;
        };
        if state != "resolved" {
            continue;
        }
        for binding in &record.metadata.bindings {
            edges.entry(binding).or_default().insert(reference);
        }
        if record.edge_kind != "binds_config_symbol" {
            used.entry((record.source_kind.clone(), record.source_key.clone()))
                .or_default()
                .insert(reference.to_owned());
        }
    }
    // Match the resolver's maximum two binding hops; facts and metadata are
    // already bounded by the query loader. Deduplicate before expanding aliases.
    for symbols in used.values_mut() {
        for _ in 0..2 {
            let next = symbols
                .iter()
                .filter_map(|symbol| edges.get(symbol.as_str()))
                .flatten()
                .map(|symbol| (*symbol).to_owned())
                .collect::<BTreeSet<_>>();
            let previous_len = symbols.len();
            symbols.extend(next);
            if symbols.len() == previous_len {
                break;
            }
        }
    }
    used
}

#[cfg(test)]
mod tests;
