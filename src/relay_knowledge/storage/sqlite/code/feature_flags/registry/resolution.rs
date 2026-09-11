//! Depth-keyed memoization bounds provider work across repeated getter usages.
use super::*;
pub(super) fn providers(rows: &[FeatureFlagRow]) -> HashMap<String, Vec<usize>> {
    let mut providers = HashMap::<String, Vec<usize>>::new();
    for (index, row) in rows.iter().enumerate() {
        for binding in &row.metadata.bindings {
            providers.entry(binding.clone()).or_default().push(index);
        }
    }
    providers
}
type Target = Option<(String, String)>;
pub(super) struct Resolver<'a> {
    pub rows: &'a [FeatureFlagRow],
    pub providers: &'a HashMap<String, Vec<usize>>,
    pub targets: HashMap<(String, usize, bool), Target>,
    pub evidence: HashMap<(String, usize), bool>,
}
impl Resolver<'_> {
    pub(super) fn resolve(&mut self, row: &FeatureFlagRow, depth: usize) -> Target {
        let Some(reference) = &row.metadata.reference else {
            return Some((row.source_kind.clone(), row.source_key.clone()));
        };
        if depth >= 4 {
            return None;
        }
        let key = (reference.clone(), depth, row.metadata.exact_reference);
        if let Some(target) = self.targets.get(&key) {
            return target
                .clone()
                .map(|(kind, key)| (row.metadata.target_kind.clone().unwrap_or(kind), key));
        }
        let mut targets = BTreeSet::new();
        let mut complete = true;
        if let Some(indices) = self.providers.get(reference) {
            for index in indices {
                if row.metadata.exact_reference
                    && self.rows[*index].metadata.declared_getter.as_ref() != Some(reference)
                {
                    continue;
                }
                if let Some(target) = self.resolve(&self.rows[*index], depth + 1) {
                    targets.insert(target);
                } else {
                    complete = false;
                    break;
                }
            }
        }
        let target = if complete && targets.len() == 1 {
            targets.into_iter().next()
        } else {
            None
        };
        self.targets.insert(key, target.clone());
        target.map(|(kind, key)| (row.metadata.target_kind.clone().unwrap_or(kind), key))
    }

    pub(super) fn has_config_evidence(&mut self, reference: &str, depth: usize) -> bool {
        if depth >= 4 {
            return false;
        }
        let key = (reference.to_owned(), depth);
        if let Some(found) = self.evidence.get(&key) {
            return *found;
        }
        let found = self.providers.get(reference).is_some_and(|indices| {
            indices.iter().any(|index| {
                let row = &self.rows[*index];
                row.source_kind != "config_symbol"
                    || row.metadata.reference.as_ref().is_some_and(|next| {
                        next != reference && self.has_config_evidence(next, depth + 1)
                    })
            })
        });
        self.evidence.insert(key, found);
        found
    }
}
