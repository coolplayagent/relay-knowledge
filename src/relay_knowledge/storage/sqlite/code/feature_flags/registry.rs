//! Bounded snapshot-local configuration binding resolution and consistency analysis.
use super::*;
mod connectivity;
mod consistency;
mod evidence;
mod resolution;
use std::collections::{BTreeMap, BTreeSet, HashMap};
pub(super) const MAX_ROWS: usize = 10_000;
const MAX_BYTES: usize = 16 * 1024 * 1024;
const COLUMNS: &str = "flag.feature_flag_id,flag.usage_id,flag.file_id,flag.path,flag.language_id,flag.name,flag.source_kind,flag.source_key,flag.edge_kind,flag.confidence_basis_points,flag.confidence_tier,flag.byte_start,flag.byte_end,flag.line_start,flag.line_end,flag.excerpt,flag.metadata_json,(SELECT symbol_snapshot_id FROM code_repository_symbols symbol WHERE symbol.source_scope=flag.source_scope AND symbol.path=flag.path AND symbol.line_start<=flag.line_start AND symbol.line_end>=flag.line_start ORDER BY symbol.line_start DESC,symbol.line_end ASC LIMIT 1),(SELECT name FROM code_repository_symbols symbol WHERE symbol.source_scope=flag.source_scope AND symbol.path=flag.path AND symbol.line_start<=flag.line_start AND symbol.line_end>=flag.line_start ORDER BY symbol.line_start DESC,symbol.line_end ASC LIMIT 1)";
struct QueryBudget<'a>(&'a Connection);
impl Drop for QueryBudget<'_> {
    fn drop(&mut self) {
        self.0.progress_handler(0, None::<fn() -> bool>);
    }
}

pub(super) fn search(
    connection: &Connection,
    status: &CodeRepositoryStatus,
    request: &CodeFeatureFlagRequest,
) -> Result<Vec<CodeFeatureFlagGraph>, StorageError> {
    search_bounded(connection, status, request).map_err(query_error)
}
fn query_error(error: StorageError) -> StorageError {
    if matches!(&error, StorageError::Sqlite(rusqlite::Error::SqliteFailure(code, _))
        if code.code == rusqlite::ErrorCode::OperationInterrupted)
    {
        incomplete("SQLite query time or step budget exceeded")
    } else {
        error
    }
}
fn search_bounded(
    connection: &Connection,
    status: &CodeRepositoryStatus,
    request: &CodeFeatureFlagRequest,
) -> Result<Vec<CodeFeatureFlagGraph>, StorageError> {
    connection.create_scalar_function(
        "config_casefold",
        1,
        rusqlite::functions::FunctionFlags::SQLITE_UTF8
            | rusqlite::functions::FunctionFlags::SQLITE_DETERMINISTIC,
        |context| Ok(context.get::<String>(0)?.to_lowercase()),
    )?;
    let mut normalized = request.clone();
    normalized.filters = normalized
        .filters
        .validate()
        .map_err(|e| StorageError::InvalidInput(e.to_string()))?;
    let request = &normalized;
    let scope = status
        .last_indexed_scope_id
        .as_deref()
        .ok_or_else(|| StorageError::InvalidInput("repository is not indexed".into()))?;
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    let mut steps = 0;
    connection.progress_handler(
        1000,
        Some(move || {
            steps += 1000;
            steps > 2_000_000 || std::time::Instant::now() >= deadline
        }),
    );
    let _budget = QueryBudget(connection);
    let terms = request
        .query
        .as_deref()
        .map(query_terms)
        .unwrap_or_default();
    let query = feature_flag_sql_query(scope, status, request, &terms);
    let mut rows = load(connection, &query.sql, &query.params)?;
    let mut seen = rows
        .iter()
        .map(|row| row.usage_id.clone())
        .collect::<BTreeSet<_>>();
    let mut queried = BTreeSet::new();
    let mut evidence_groups = BTreeSet::new();
    for round in 0..4 {
        let keys = rows
            .iter()
            .flat_map(|row| {
                row.metadata
                    .bindings
                    .iter()
                    .chain(row.metadata.reference.iter())
            })
            .filter(|key| !queried.contains(*key))
            .cloned()
            .collect::<BTreeSet<_>>();
        if keys.is_empty() {
            break;
        }
        if keys.len() + queried.len() > 1000 {
            return Err(incomplete("symbol binding budget exceeded"));
        }
        for chunk in keys.iter().collect::<Vec<_>>().chunks(400) {
            let filter = feature_flag_sql_filter(scope, status, request, &[]);
            let list = vec!["?"; chunk.len()].join(",");
            let mut params = filter.params;
            for _ in 0..2 {
                params.extend(chunk.iter().map(|key| Value::Text((***key).to_owned())));
            }
            let sql = format!(
                "SELECT {COLUMNS} FROM code_repository_feature_flags flag WHERE ({}) AND (json_extract(flag.metadata_json,'$.reference') IN ({list}) OR EXISTS (SELECT 1 FROM json_each(flag.metadata_json,'$.bindings') binding WHERE binding.value IN ({list}))) LIMIT {}",
                filter.where_clause,
                MAX_ROWS + 1
            );
            for row in load(connection, &sql, &params)? {
                if seen.insert(row.usage_id.clone()) {
                    rows.push(row);
                }
            }
            check_size(&rows)?;
        }
        queried.extend(keys);
        evidence::complete_groups(
            connection,
            scope,
            status,
            request,
            &mut rows,
            &mut seen,
            &mut evidence_groups,
        )?;
        if round == 3
            && rows.iter().any(|row| {
                row.metadata
                    .bindings
                    .iter()
                    .chain(row.metadata.reference.iter())
                    .any(|key| !queried.contains(key))
            })
        {
            return Err(incomplete("symbol binding depth exceeded"));
        }
    }
    let providers = resolution::providers(&rows);
    let formats = if request.filters.consistency {
        consistency::formats(connection, scope, status, request)?
    } else {
        BTreeSet::new()
    };
    let mut resolver = resolution::Resolver {
        rows: &rows,
        providers: &providers,
        targets: HashMap::new(),
        evidence: HashMap::new(),
    };
    let incomplete_rows = connectivity::incomplete_rows(&rows, &mut resolver);
    let referenced_bindings = rows
        .iter()
        .filter(|row| row.metadata.target_kind.is_some())
        .filter_map(|row| row.metadata.reference.as_ref())
        .collect::<BTreeSet<_>>();
    let mut binding_kinds = HashMap::<&str, BTreeSet<String>>::new();
    for row in &rows {
        if let (Some(reference), Some(kind)) = (&row.metadata.reference, &row.metadata.target_kind)
        {
            binding_kinds
                .entry(reference)
                .or_default()
                .insert(kind.clone());
        }
    }
    let mut groups = BTreeMap::<(String, String), CodeFeatureFlagGraph>::new();
    for (row_index, row) in rows.iter().enumerate() {
        if row.edge_kind == "declares_string_constant"
            && !row
                .metadata
                .bindings
                .iter()
                .any(|binding| referenced_bindings.contains(binding))
        {
            continue;
        }
        if row.edge_kind == "declares_config_getter" {
            continue;
        }
        if row.source_kind == "config_symbol"
            && row.metadata.target_kind.is_none()
            && !row
                .metadata
                .reference
                .as_ref()
                .is_some_and(|reference| resolver.has_config_evidence(reference, 0))
        {
            continue;
        }
        let mut resolved = row.clone();
        if resolved.edge_kind == "declares_string_constant" {
            resolved.edge_kind = "declares_config_key".into();
        }
        let targets = resolver.resolve(row, 0);
        let complete = if row.metadata.reference.is_none() {
            true
        } else if let Some((kind, key)) = targets {
            resolved.source_kind = row.metadata.target_kind.clone().unwrap_or(kind);
            resolved.source_key = key;
            resolved.name = resolved
                .source_key
                .replace(['.', '-', ':'], "_")
                .to_ascii_lowercase();
            true
        } else {
            false
        };
        let mut kinds = BTreeSet::new();
        if matches!(
            row.edge_kind.as_str(),
            "declares_config_key" | "declares_string_constant"
        ) {
            for binding in &row.metadata.bindings {
                if let Some(namespaces) = binding_kinds.get(binding.as_str()) {
                    kinds.extend(namespaces.iter().cloned());
                }
            }
        }
        if kinds.is_empty() {
            kinds.insert(resolved.source_kind.clone());
        }
        for kind in kinds {
            let mut resolved = resolved.clone();
            resolved.source_kind = kind;
            if row.metadata.reference.is_some() || resolved.source_kind != row.source_kind {
                let mut hasher = crate::identity::StableHasher64::new();
                for part in [
                    &status.repository_id,
                    scope,
                    &resolved.source_kind,
                    &resolved.source_key,
                ] {
                    hasher.update(&(part.len() as u64).to_le_bytes());
                    hasher.update(part.as_bytes());
                }
                resolved.feature_flag_id = format!("feature_flag:{:016x}", hasher.finish());
            }
            let key = (resolved.source_kind.clone(), resolved.source_key.clone());
            let group = groups.entry(key).or_insert_with(|| CodeFeatureFlagGraph {
                feature_flag_id: resolved.feature_flag_id.clone(),
                name: resolved.name.clone(),
                source_kind: resolved.source_kind.clone(),
                source_key: resolved.source_key.clone(),
                score: 0.0,
                usages: Vec::new(),
                consistency_diagnostics: Vec::new(),
                conflicting_default_sources: Vec::new(),
                analysis_complete: !status.stale
                    && status.degraded_reason.is_none()
                    && !incomplete_rows[row_index],
            });
            group.analysis_complete &=
                complete && !incomplete_rows[row_index] && row.metadata.flow_incomplete.is_none();
            if !complete
                && !group
                    .consistency_diagnostics
                    .iter()
                    .any(|d| d == "unresolved_or_ambiguous_config_symbol")
            {
                group
                    .consistency_diagnostics
                    .push("unresolved_or_ambiguous_config_symbol".to_owned());
            }
            group.score = group.score.max(score_row(&resolved, &terms));
            group.usages.push(CodeFeatureFlagUsage {
                usage_id: resolved.usage_id,
                path: resolved.path,
                language_id: resolved.language_id,
                file_id: resolved.file_id,
                byte_range: resolved.byte_range,
                line_range: resolved.line_range,
                edge_kind: resolved.edge_kind,
                related_symbol_snapshot_id: resolved.related_symbol_snapshot_id,
                related_symbol_name: resolved.related_symbol_name,
                confidence_basis_points: resolved.confidence_basis_points,
                confidence_tier: resolved.confidence_tier,
                excerpt: resolved.excerpt,
                metadata: resolved.metadata,
            });
        }
    }
    let mut groups = groups
        .into_values()
        .filter(|group| matches_group(group, request, &terms))
        .collect::<Vec<_>>();
    for group in &mut groups {
        group.usages.sort_by(|a, b| {
            edge_priority(&a.edge_kind)
                .cmp(&edge_priority(&b.edge_kind))
                .then_with(|| a.path.cmp(&b.path))
                .then_with(|| a.byte_range.start.cmp(&b.byte_range.start))
        });
        if request.filters.consistency {
            consistency::check(group, &formats);
        }
    }
    groups.sort_by(|a, b| {
        b.score
            .total_cmp(&a.score)
            .then_with(|| a.source_key.cmp(&b.source_key))
    });
    groups.truncate(request.limit);
    Ok(groups)
}
fn matches_group(
    group: &CodeFeatureFlagGraph,
    request: &CodeFeatureFlagRequest,
    terms: &[String],
) -> bool {
    let haystack = format!(
        "{} {} {} {}",
        group.name,
        group.source_key,
        group.source_kind,
        group
            .usages
            .iter()
            .map(|u| format!("{} {}", u.path, u.excerpt))
            .collect::<Vec<_>>()
            .join(" ")
    )
    .to_lowercase();
    terms.iter().all(|term| haystack.contains(term))
        && request.filters.domain.as_ref().is_none_or(|v| {
            group
                .usages
                .iter()
                .any(|u| u.metadata.domain.as_ref() == Some(v))
        })
        && request
            .filters
            .source
            .as_ref()
            .is_none_or(|v| group.usages.iter().any(|u| &u.metadata.source_format == v))
        && request.filters.hot_reload.is_none_or(|v| {
            group
                .usages
                .iter()
                .any(|u| u.metadata.hot_reload == Some(v))
        })
}
fn incomplete(reason: &str) -> StorageError {
    StorageError::InvalidInput(format!(
        "configuration analysis incomplete: {reason}; narrow the query scope"
    ))
}
fn check_size(rows: &[FeatureFlagRow]) -> Result<(), StorageError> {
    if rows.len() > MAX_ROWS {
        return Err(incomplete("usage budget exceeded"));
    }
    let bytes = rows.iter().map(row_size).sum::<usize>();
    if bytes > MAX_BYTES {
        return Err(incomplete("16 MiB fact budget exceeded"));
    }
    Ok(())
}
fn row_size(row: &FeatureFlagRow) -> usize {
    std::mem::size_of::<FeatureFlagRow>()
        + [
            &row.feature_flag_id,
            &row.usage_id,
            &row.file_id,
            &row.path,
            &row.language_id,
            &row.name,
            &row.source_kind,
            &row.source_key,
            &row.edge_kind,
            &row.confidence_tier,
            &row.excerpt,
        ]
        .iter()
        .map(|value| value.len())
        .sum::<usize>()
        + row
            .related_symbol_snapshot_id
            .as_ref()
            .map_or(0, String::len)
        + row.related_symbol_name.as_ref().map_or(0, String::len)
        + row.metadata.bindings.capacity() * std::mem::size_of::<String>()
        + serde_json::to_vec(&row.metadata).map_or(MAX_BYTES, |value| value.len())
}

fn load(
    connection: &Connection,
    sql: &str,
    params: &[Value],
) -> Result<Vec<FeatureFlagRow>, StorageError> {
    load_rows(connection, sql, params).map_err(query_error)
}
fn load_rows(
    connection: &Connection,
    sql: &str,
    params: &[Value],
) -> Result<Vec<FeatureFlagRow>, StorageError> {
    let mut statement = connection.prepare(sql)?;
    let mapped = statement.query_map(params_from_iter(params.iter()), |row| {
        let json: String = row.get(16)?;
        if json.len() > 65_536 {
            return Err(rusqlite::Error::InvalidQuery);
        }
        let metadata = serde_json::from_str(&json).map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(16, rusqlite::types::Type::Text, Box::new(e))
        })?;
        Ok(FeatureFlagRow {
            feature_flag_id: row.get(0)?,
            usage_id: row.get(1)?,
            file_id: row.get(2)?,
            path: row.get(3)?,
            language_id: row.get(4)?,
            name: row.get(5)?,
            source_kind: row.get(6)?,
            source_key: row.get(7)?,
            edge_kind: row.get(8)?,
            confidence_basis_points: row.get(9)?,
            confidence_tier: row.get(10)?,
            byte_range: RepositoryCodeRange {
                start: row.get(11)?,
                end: row.get(12)?,
            },
            line_range: RepositoryCodeRange {
                start: row.get(13)?,
                end: row.get(14)?,
            },
            excerpt: row.get(15)?,
            metadata,
            related_symbol_snapshot_id: row.get(17)?,
            related_symbol_name: row.get(18)?,
        })
    })?;
    let mut rows = Vec::new();
    let mut bytes = 0usize;
    for row in mapped {
        let row = row?;
        bytes = bytes.saturating_add(row_size(&row));
        if bytes > MAX_BYTES {
            return Err(incomplete("16 MiB fact budget exceeded"));
        }
        if rows.len() >= MAX_ROWS {
            return Err(incomplete("usage budget exceeded"));
        }
        rows.push(row);
    }
    check_size(&rows)?;
    Ok(rows)
}

#[cfg(test)]
#[path = "registry_tests.rs"]
mod tests;
