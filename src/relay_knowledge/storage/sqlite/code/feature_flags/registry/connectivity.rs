//! Propagate uncertainty only across connected configuration facts.
use super::*;

pub(super) fn incomplete_rows(
    rows: &[FeatureFlagRow],
    resolver: &mut resolution::Resolver<'_>,
) -> Vec<bool> {
    let mut parents = (0..rows.len()).collect::<Vec<_>>();
    let mut symbols = HashMap::<&str, usize>::new();
    let mut keys = HashMap::<(&str, &str), usize>::new();
    for (index, row) in rows.iter().enumerate() {
        for symbol in row
            .metadata
            .bindings
            .iter()
            .chain(row.metadata.reference.iter())
        {
            if let Some(previous) = symbols.insert(symbol, index) {
                let left = root(&mut parents, previous);
                let right = root(&mut parents, index);
                parents[right] = left;
            }
        }
        if row.source_kind != "config_symbol" {
            if let Some(previous) = keys.insert((&row.source_kind, &row.source_key), index) {
                let left = root(&mut parents, previous);
                let right = root(&mut parents, index);
                parents[right] = left;
            }
        }
    }
    let mut bad = BTreeSet::new();
    for (index, row) in rows.iter().enumerate() {
        let unresolved = row.metadata.reference.as_ref().is_some_and(|reference| {
            (row.metadata.target_kind.is_some() || resolver.has_config_evidence(reference, 0))
                && resolver.resolve(row, 0).is_none()
        });
        if unresolved || row.metadata.flow_incomplete.is_some() {
            bad.insert(root(&mut parents, index));
        }
    }
    (0..rows.len())
        .map(|index| bad.contains(&root(&mut parents, index)))
        .collect()
}

fn root(parents: &mut [usize], mut index: usize) -> usize {
    while parents[index] != index {
        parents[index] = parents[parents[index]];
        index = parents[index];
    }
    index
}
