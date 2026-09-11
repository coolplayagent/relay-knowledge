//! Snapshot-local type hierarchy connects getter evidence across Java files.
use super::*;
pub(super) struct Hierarchy {
    parents: BTreeMap<String, BTreeSet<String>>,
    children: BTreeMap<String, BTreeSet<String>>,
}
impl Hierarchy {
    pub(super) fn load(
        connection: &Connection,
        scope: &str,
        status: &CodeRepositoryStatus,
        request: &CodeFeatureFlagRequest,
    ) -> Result<Self, StorageError> {
        let filter = feature_flag_sql_filter(scope, status, request, &[]);
        let sql = format!(
            "SELECT {COLUMNS} FROM code_repository_feature_flags flag WHERE ({}) AND flag.edge_kind='config_type_hierarchy' LIMIT {}",
            filter.where_clause,
            MAX_ROWS + 1
        );
        let rows = load(connection, &sql, &filter.params)?;
        check_size(&rows)?;
        if rows.iter().map(row_size).sum::<usize>() > 4 * 1024 * 1024 {
            return Err(incomplete("type hierarchy 4 MiB evidence budget exceeded"));
        }
        let mut parents: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        let mut children: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        for row in rows {
            for parent in row.metadata.bindings {
                parents
                    .entry(row.source_key.clone())
                    .or_default()
                    .insert(parent.clone());
                children
                    .entry(parent)
                    .or_default()
                    .insert(row.source_key.clone());
            }
        }
        Ok(Self { parents, children })
    }
    fn related(&self, symbol: &str, descendants: bool) -> Result<BTreeSet<String>, StorageError> {
        let Some((owner, method)) = symbol.rsplit_once('.') else {
            return Ok(BTreeSet::from([symbol.into()]));
        };
        if !method.starts_with("get") && !method.starts_with("is") {
            return Ok(BTreeSet::from([symbol.into()]));
        }
        let mut seen = BTreeSet::from([owner.to_owned()]);
        let mut pending = vec![owner.to_owned()];
        while let Some(owner) = pending.pop() {
            let ancestors = self.parents.get(&owner).into_iter().flatten();
            let children = self
                .children
                .get(&owner)
                .into_iter()
                .flatten()
                .filter(|_| descendants);
            for next in ancestors.chain(children) {
                if seen.insert(next.clone()) {
                    if seen.len() > 64 {
                        return Err(incomplete("cross-file type closure budget exceeded"));
                    }
                    pending.push(next.clone());
                }
            }
        }
        Ok(seen
            .into_iter()
            .map(|owner| format!("{owner}.{method}"))
            .collect())
    }
    pub(super) fn expand(&self, keys: &BTreeSet<String>) -> Result<BTreeSet<String>, StorageError> {
        let mut expanded = BTreeSet::new();
        for key in keys {
            expanded.extend(self.related(key, true)?);
            if expanded.len() > 1000 {
                return Err(incomplete("symbol binding budget exceeded"));
            }
        }
        Ok(expanded)
    }
    pub(super) fn augment(&self, rows: &mut [FeatureFlagRow]) -> Result<(), StorageError> {
        if self.parents.is_empty() {
            return Ok(());
        }
        let mut retained_bytes = rows.iter().map(row_size).sum::<usize>();
        for row in rows {
            if row.language_id != "java"
                || matches!(
                    row.edge_kind.as_str(),
                    "declares_config_key" | "declares_string_constant"
                )
            {
                continue;
            }
            let previous_bytes = row_size(row);
            let mut bindings = BTreeSet::new();
            for symbol in &row.metadata.bindings {
                bindings.extend(self.related(symbol, false)?);
                if bindings.len() > 1000 {
                    return Err(incomplete("symbol binding budget exceeded"));
                }
            }
            row.metadata.bindings = bindings.into_iter().collect();
            retained_bytes = retained_bytes - previous_bytes + row_size(row);
            if retained_bytes > MAX_BYTES {
                return Err(incomplete("16 MiB fact budget exceeded"));
            }
        }
        Ok(())
    }
}
