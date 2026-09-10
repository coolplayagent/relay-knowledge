//! Load complete scoped evidence for keys discovered through symbolic bindings.
use super::*;
pub(super) fn complete_groups(
    connection: &Connection,
    scope: &str,
    status: &CodeRepositoryStatus,
    request: &CodeFeatureFlagRequest,
    rows: &mut Vec<FeatureFlagRow>,
    seen: &mut BTreeSet<String>,
    queried: &mut BTreeSet<(String, String)>,
) -> Result<(), StorageError> {
    let providers = resolution::providers(rows);
    let mut resolver = resolution::Resolver {
        rows,
        providers: &providers,
        targets: HashMap::new(),
        evidence: HashMap::new(),
    };
    let keys = rows
        .iter()
        .filter_map(|row| resolver.resolve(row, 0))
        .filter(|key| key.0 != "config_symbol" && !queried.contains(key))
        .collect::<BTreeSet<_>>();
    for chunk in keys.iter().collect::<Vec<_>>().chunks(200) {
        let filter = feature_flag_sql_filter(scope, status, request, &[]);
        let mut params = filter.params;
        for (kind, key) in chunk {
            params.extend([Value::Text(kind.clone()), Value::Text(key.clone())]);
        }
        let predicates =
            vec!["(flag.source_kind=? AND flag.source_key=?)"; chunk.len()].join(" OR ");
        let sql = format!(
            "SELECT {COLUMNS} FROM code_repository_feature_flags flag WHERE ({}) AND ({predicates}) LIMIT {}",
            filter.where_clause,
            MAX_ROWS + 1
        );
        for row in load(connection, &sql, &params)? {
            if seen.insert(row.usage_id.clone()) {
                rows.push(row);
            }
        }
        check_size(rows)?;
    }
    queried.extend(keys);
    Ok(())
}
