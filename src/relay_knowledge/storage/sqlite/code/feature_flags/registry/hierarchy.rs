//! Snapshot-local type hierarchy connects getter evidence across Java files.
use super::*;
pub(super) struct Hierarchy {
    declarations: BTreeSet<String>,
    fields: BTreeMap<String, String>,
    packages: BTreeMap<String, String>,
    parents: BTreeMap<String, BTreeSet<String>>,
    children: BTreeMap<String, BTreeSet<String>>,
}
struct Access {
    package: Option<String>,
    visibility: Option<String>,
    overridable: Option<bool>,
}
impl Hierarchy {
    pub(super) fn load(
        connection: &Connection,
        scope: &str,
        status: &CodeRepositoryStatus,
        request: &CodeFeatureFlagRequest,
        seeds: &[FeatureFlagRow],
    ) -> Result<Self, StorageError> {
        let rows = Self::relevant_rows(connection, scope, status, request, seeds)?;
        let mut parents: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        let mut children: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        let mut declarations = BTreeSet::new();
        let mut packages = BTreeMap::new();
        let mut fields = BTreeMap::new();
        for row in rows {
            if row.edge_kind == "config_type_declaration" {
                for (name, visibility) in row.metadata.java_fields {
                    fields.insert(format!("{}.{name}", row.source_key), visibility);
                }
                if let Some(package) = row.metadata.java_package {
                    packages.insert(row.source_key.clone(), package);
                }
                declarations.insert(row.source_key);
                continue;
            }
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
        Ok(Self {
            parents,
            fields,
            children,
            declarations,
            packages,
        })
    }
    fn relevant_rows(
        connection: &Connection,
        scope: &str,
        status: &CodeRepositoryStatus,
        request: &CodeFeatureFlagRequest,
        seeds: &[FeatureFlagRow],
    ) -> Result<Vec<FeatureFlagRow>, StorageError> {
        let mut owners = BTreeSet::new();
        let mut closure = BTreeSet::new();
        for row in seeds {
            owners.extend(row.metadata.implicit_platform_owner.iter().cloned());
            owners.extend(row.metadata.conversion_platform_owners.iter().cloned());
            for symbol in row
                .metadata
                .bindings
                .iter()
                .chain(row.metadata.reference.iter())
                .chain(row.metadata.same_package_reference.iter())
            {
                if let Some((owner, _)) = symbol.rsplit_once('.') {
                    closure.insert(owner.to_owned());
                }
            }
        }
        owners.extend(closure.iter().cloned());
        let mut queried = BTreeSet::new();
        let mut seen = BTreeSet::new();
        let mut result = Vec::new();
        for _ in 0..64 {
            if owners.len() > 1000 {
                return Err(incomplete("type owner budget exceeded"));
            }
            let pending = owners.difference(&queried).cloned().collect::<Vec<_>>();
            if pending.is_empty() {
                return Ok(result);
            }
            for chunk in pending.chunks(200) {
                let filter = feature_flag_sql_filter(scope, status, request, &[]);
                let list = vec!["?"; chunk.len()].join(",");
                let mut params = filter.params;
                params.extend(chunk.iter().cloned().map(Value::Text));
                let related = chunk
                    .iter()
                    .filter(|owner| closure.contains(*owner))
                    .cloned()
                    .collect::<Vec<_>>();
                let related_list = vec!["?"; related.len()].join(",");
                params.extend(related.iter().cloned().map(Value::Text));
                params.extend(related.iter().cloned().map(Value::Text));
                let sql = format!(
                    "SELECT {COLUMNS} FROM code_repository_feature_flags flag WHERE ({}) AND ((flag.edge_kind='config_type_declaration' AND flag.source_key IN ({list})) OR (flag.edge_kind='config_type_hierarchy' AND (flag.source_key IN ({related_list}) OR EXISTS (SELECT 1 FROM json_each(flag.metadata_json,'$.bindings') binding WHERE binding.value IN ({related_list}))))) LIMIT {}",
                    filter.where_clause,
                    MAX_ROWS + 1
                );
                for row in load(connection, &sql, &params)? {
                    if row.edge_kind == "config_type_hierarchy" {
                        closure.insert(row.source_key.clone());
                        closure.extend(row.metadata.bindings.iter().cloned());
                        owners.extend(closure.iter().cloned());
                    }
                    if seen.insert(row.usage_id.clone()) {
                        result.push(row);
                    }
                }
                check_size(&result)?;
                if result.iter().map(row_size).sum::<usize>() > 4 * 1024 * 1024 {
                    return Err(incomplete("type hierarchy 4 MiB evidence budget exceeded"));
                }
            }
            queried.extend(pending);
        }
        Err(incomplete(
            "type closure budget exceeded (64 loading rounds)",
        ))
    }
    pub(super) fn filter_platform_reads(&self, rows: &mut Vec<FeatureFlagRow>) {
        rows.retain(|row| {
            row.metadata
                .implicit_platform_owner
                .as_ref()
                .is_none_or(|owner| !self.declarations.contains(owner))
        });
        for row in rows {
            if let Some(candidate) = &row.metadata.same_package_reference {
                if candidate
                    .rsplit_once('.')
                    .is_some_and(|(owner, _)| self.declarations.contains(owner))
                {
                    row.metadata.reference = Some(candidate.clone());
                }
            }
            if row
                .metadata
                .conversion_platform_owners
                .iter()
                .any(|owner| self.declarations.contains(owner))
            {
                row.metadata.bindings.clear();
                row.metadata.flow_incomplete = Some("shadowed_platform_conversion".into());
            }
        }
    }
    fn related(&self, symbol: &str, descendants: bool) -> Result<BTreeSet<String>, StorageError> {
        let Some((owner, method)) = symbol.rsplit_once('.') else {
            return Ok(BTreeSet::from([symbol.into()]));
        };
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
        let rows_declared_fields = rows
            .iter()
            .filter(|r| {
                matches!(
                    r.edge_kind.as_str(),
                    "declares_config_key" | "declares_string_constant"
                )
            })
            .filter_map(|r| r.metadata.bindings.first().cloned())
            .collect::<BTreeSet<_>>();
        let declared = rows
            .iter()
            .filter_map(|row| row.metadata.declared_getter.clone())
            .collect::<BTreeSet<_>>();
        let access = rows
            .iter()
            .filter_map(|row| {
                row.metadata.declared_getter.clone().map(|key| {
                    (
                        key,
                        Access {
                            package: row.metadata.java_package.clone(),
                            visibility: row.metadata.getter_visibility.clone(),
                            overridable: row.metadata.getter_overridable,
                        },
                    )
                })
            })
            .collect::<BTreeMap<_, _>>();
        let declared_fields = self
            .fields
            .keys()
            .cloned()
            .chain(rows_declared_fields)
            .collect();
        for row in rows {
            if row.language_id != "java" {
                continue;
            }
            let previous_bytes = row_size(row);
            if matches!(
                row.edge_kind.as_str(),
                "declares_config_key" | "declares_string_constant"
            ) {
                if let Some(own) = row.metadata.bindings.first().cloned() {
                    let visibility = self.fields.get(&own).cloned();
                    let package = own
                        .rsplit_once('.')
                        .and_then(|(owner, _)| self.packages.get(owner))
                        .cloned();
                    let access = Access {
                        package,
                        visibility,
                        overridable: Some(false),
                    };
                    row.metadata.bindings = vec![own.clone()];
                    if access.visibility.as_deref() != Some("private") {
                        row.metadata.bindings.extend(self.inherited(
                            &own,
                            &declared_fields,
                            Some(&access),
                        )?);
                    }
                }
                retained_bytes = retained_bytes - previous_bytes + row_size(row);
                if retained_bytes > MAX_BYTES {
                    return Err(incomplete("16 MiB fact budget exceeded"));
                }
                continue;
            }
            let mut bindings = BTreeSet::new();
            let original = row
                .metadata
                .declared_getter
                .clone()
                .filter(|_| !row.metadata.bindings.is_empty())
                .map_or_else(|| row.metadata.bindings.clone(), |own| vec![own]);
            row.metadata.inherited_getters.clear();
            for symbol in &original {
                if row.metadata.getter_overridable == Some(false) {
                    bindings.insert(symbol.clone());
                } else {
                    bindings.extend(self.overrides(symbol, &access)?);
                }
                if bindings.len() > 1000 {
                    return Err(incomplete("symbol binding budget exceeded"));
                }
            }
            if let Some(own) =
                row.metadata.declared_getter.as_deref().filter(|_| {
                    !original.is_empty() && row.metadata.getter_inheritable != Some(false)
                })
            {
                row.metadata.inherited_getters = self.inherited(own, &declared, access.get(own))?;
                bindings.extend(row.metadata.inherited_getters.iter().cloned());
            }
            row.metadata.bindings = bindings.into_iter().collect();
            retained_bytes = retained_bytes - previous_bytes + row_size(row);
            if retained_bytes > MAX_BYTES {
                return Err(incomplete("16 MiB fact budget exceeded"));
            }
        }
        Ok(())
    }
    fn inherited(
        &self,
        symbol: &str,
        declared: &BTreeSet<String>,
        access: Option<&Access>,
    ) -> Result<Vec<String>, StorageError> {
        let Some((owner, method)) = symbol.rsplit_once('.') else {
            return Ok(Vec::new());
        };
        let mut pending = vec![owner.to_owned()];
        let mut seen = BTreeSet::from([owner.to_owned()]);
        let mut inherited = Vec::new();
        while let Some(owner) = pending.pop() {
            for child in self.children.get(&owner).into_iter().flatten() {
                if !seen.insert(child.clone()) {
                    continue;
                }
                if seen.len() > 64 {
                    return Err(incomplete("inherited getter closure budget exceeded"));
                }
                let key = format!("{child}.{method}");
                if access.is_some_and(|access| {
                    access.visibility.as_deref() == Some("package")
                        && access
                            .package
                            .as_ref()
                            .zip(self.packages.get(child))
                            .is_some_and(|(own, child)| own != child)
                }) {
                    continue;
                }
                if declared.contains(&key) {
                    continue;
                }
                inherited.push(key);
                pending.push(child.clone());
            }
        }
        Ok(inherited)
    }
    fn overrides(
        &self,
        symbol: &str,
        access: &BTreeMap<String, Access>,
    ) -> Result<BTreeSet<String>, StorageError> {
        let Some((owner, method)) = symbol.rsplit_once('.') else {
            return Ok(BTreeSet::from([symbol.into()]));
        };
        if !method.starts_with("get") && !method.starts_with("is") {
            return Ok(BTreeSet::from([symbol.into()]));
        }
        let own = access.get(symbol);
        let own_package = own.and_then(|a| a.package.as_ref());
        let mut pending = vec![(owner.to_owned(), false)];
        let mut seen = BTreeSet::from([(owner.to_owned(), false)]);
        let mut result = BTreeSet::from([symbol.to_owned()]);
        while let Some((owner, crossed)) = pending.pop() {
            for parent in self.parents.get(&owner).into_iter().flatten() {
                let key = format!("{parent}.{method}");
                let target = access.get(&key);
                let package = self
                    .packages
                    .get(parent)
                    .or_else(|| target.and_then(|a| a.package.as_ref()));
                let crossed = crossed || own_package.zip(package).is_some_and(|(a, b)| a != b);
                if own.is_some_and(|a| a.visibility.as_deref() == Some("package")) && crossed {
                    continue;
                }
                if target.is_some_and(|a| {
                    a.overridable == Some(false)
                        || a.visibility.as_deref() == Some("private")
                        || (a.visibility.as_deref() == Some("package") && crossed)
                }) {
                    continue;
                }
                if seen.insert((parent.clone(), crossed)) {
                    if seen.len() > 64 {
                        return Err(incomplete("getter access closure budget exceeded"));
                    }
                    result.insert(key);
                    pending.push((parent.clone(), crossed));
                }
            }
        }
        Ok(result)
    }
}
