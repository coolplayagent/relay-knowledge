//! SQL-ranked flag candidates and bounded snapshot-local binding closure.
use super::filters::{append_language_filter_clause, append_path_filter_clause};
use crate::{
    domain::{
        CodeFeatureFlagRecord, CodeFeatureFlagRequest, CodeRepositoryStatus, RepositoryCodeRange,
    },
    storage::StorageError,
};
use rusqlite::{Connection, params_from_iter, types::Value};
use std::collections::BTreeSet;

const MAX_SCOPE_USAGES: usize = 10_000;
const MAX_METADATA_BYTES: usize = 64 * 1024;
const MAX_ANALYSIS_BYTES: usize = 16 * 1024 * 1024;
const MAX_BINDING_IDENTITIES: usize = 1000;
const MAX_CLOSURE_ROUNDS: usize = 4;
const FACT_COLUMNS: &str =
    "repository_id, source_scope, feature_flag_id, usage_id, file_id, path, language_id,
    name, source_kind, source_key, edge_kind, confidence_basis_points, confidence_tier,
    byte_start, byte_end, line_start, line_end, excerpt, metadata_json";

pub(super) fn load(
    connection: &Connection,
    status: &CodeRepositoryStatus,
    request: &CodeFeatureFlagRequest,
) -> Result<Vec<CodeFeatureFlagRecord>, StorageError> {
    let (scope_predicate, scope_params) = authorized_scope(status, request)?;
    let mut records = Vec::new();
    let mut analyzed_bytes = 0usize;
    if request.consistency {
        append_records(
            connection,
            &scope_predicate,
            scope_params,
            &mut records,
            &mut analyzed_bytes,
        )?;
        return Ok(records);
    }
    let keys = ranked_keys(connection, &scope_predicate, &scope_params, request)?;
    if keys.is_empty() {
        return Ok(records);
    }
    let mut values = scope_params.clone();
    let keys_clause = key_clause(&keys, &mut values);
    append_records(
        connection,
        &format!("{scope_predicate} AND {keys_clause}"),
        values,
        &mut records,
        &mut analyzed_bytes,
    )?;
    for _ in 0..MAX_CLOSURE_ROUNDS {
        let keys = records
            .iter()
            .map(|r| (r.source_kind.clone(), r.source_key.clone()))
            .collect::<BTreeSet<_>>();
        let mut symbols = BTreeSet::new();
        for record in &records {
            symbols.extend(record.metadata.bindings.iter().cloned());
            if matches!(
                record.source_kind.as_str(),
                "config_symbol" | "config_getter"
            ) {
                symbols.insert(
                    record
                        .metadata
                        .referenced_symbol
                        .clone()
                        .unwrap_or_else(|| record.source_key.clone()),
                );
            }
        }
        if symbols.is_empty() {
            return Ok(records);
        }
        if symbols.len() + keys.len() > MAX_BINDING_IDENTITIES {
            return Err(incomplete("binding identity budget exhausted"));
        }
        let mut values = scope_params.clone();
        let same_keys = key_clause(&keys, &mut values);
        let references = in_clause("flag.source_key", &symbols, &mut values);
        let bindings = in_clause("binding.value", &symbols, &mut values);
        let seen = records
            .iter()
            .map(|r| r.usage_id.clone())
            .collect::<BTreeSet<_>>();
        let excluded = in_clause("flag.usage_id", &seen, &mut values);
        let predicate = format!("{scope_predicate} AND ({same_keys} OR (flag.source_kind IN ('config_symbol', 'config_getter') AND {references})
            OR EXISTS (SELECT 1 FROM json_each(flag.metadata_json, '$.bindings') binding WHERE {bindings})) AND NOT ({excluded})");
        let before = records.len();
        append_records(
            connection,
            &predicate,
            values,
            &mut records,
            &mut analyzed_bytes,
        )?;
        if before == records.len() {
            return Ok(records);
        }
    }
    Err(incomplete("binding closure exceeded four bounded rounds"))
}

fn authorized_scope(
    status: &CodeRepositoryStatus,
    request: &CodeFeatureFlagRequest,
) -> Result<(String, Vec<Value>), StorageError> {
    let scope = status.last_indexed_scope_id.as_deref().ok_or_else(|| {
        StorageError::InvalidInput("repository has no indexed source scope".to_owned())
    })?;
    let mut clauses = vec!["flag.source_scope = ?".to_owned()];
    let mut values = vec![Value::Text(scope.to_owned())];
    append_path_filter_clause(&mut clauses, &mut values, &status.path_filters);
    append_path_filter_clause(&mut clauses, &mut values, &request.repository.path_filters);
    append_language_filter_clause(&mut clauses, &mut values, &status.language_filters);
    append_language_filter_clause(
        &mut clauses,
        &mut values,
        &request.repository.language_filters,
    );
    Ok((clauses.join(" AND "), values))
}

fn ranked_keys(
    connection: &Connection,
    scope_predicate: &str,
    scope_params: &[Value],
    request: &CodeFeatureFlagRequest,
) -> Result<BTreeSet<(String, String)>, StorageError> {
    let reference_scope = scope_predicate.replace("flag.", "reference.");
    let evidence = format!("(flag.edge_kind <> 'binds_config_symbol' OR EXISTS (
        SELECT 1 FROM json_each(flag.metadata_json, '$.bindings') binding
        CROSS JOIN code_repository_feature_flags reference
        WHERE reference.source_key = binding.value AND {reference_scope} AND reference.source_kind IN ('config_symbol', 'config_getter')
          AND reference.edge_kind <> 'binds_config_symbol'))");
    let getter_scope = scope_predicate.replace("flag.", "owner.");
    let getter_evidence = format!(
        "(flag.source_kind <> 'config_getter' OR flag.source_key IN (
        SELECT binding.value FROM code_repository_feature_flags owner
        CROSS JOIN json_each(owner.metadata_json, '$.bindings') binding
        WHERE {getter_scope} AND owner.source_kind <> 'config_getter'))"
    );
    let mut clauses = vec![scope_predicate.to_owned(), evidence, getter_evidence];
    let mut values = scope_params.to_vec();
    values.extend_from_slice(scope_params);
    values.extend_from_slice(scope_params);
    if let Some(query) = &request.query {
        for term in query
            .split(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
            .filter(|s| !s.is_empty())
        {
            let fields = [
                "name",
                "source_kind",
                "source_key",
                "edge_kind",
                "path",
                "excerpt",
                "metadata_json",
            ];
            clauses.push(format!(
                "({})",
                fields
                    .iter()
                    .map(|field| format!("lower(flag.{field}) LIKE ? ESCAPE '\\'"))
                    .collect::<Vec<_>>()
                    .join(" OR ")
            ));
            let pattern = format!(
                "%{}%",
                term.to_ascii_lowercase()
                    .replace('\\', "\\\\")
                    .replace('%', "\\%")
                    .replace('_', "\\_")
            );
            values.extend(fields.iter().map(|_| Value::Text(pattern.clone())));
        }
    }
    for (field, value) in [
        ("domain", &request.domain),
        ("source_format", &request.source),
    ] {
        if let Some(value) = value {
            clauses.push(format!("json_extract(flag.metadata_json, '$.{field}') = ?"));
            values.push(Value::Text(value.clone()));
        }
    }
    if let Some(value) = request.hot_reload {
        clauses.push("json_extract(flag.metadata_json, '$.hot_reload') = ?".to_owned());
        values.push(Value::Integer(i64::from(value)));
    }
    values.push(Value::Integer(request.limit as i64));
    let sql = format!("SELECT flag.source_kind, flag.source_key FROM code_repository_feature_flags flag WHERE {}
        GROUP BY flag.source_kind, flag.source_key ORDER BY MAX(CASE flag.edge_kind WHEN 'guards_code' THEN 20.0 WHEN 'defines_config' THEN 16.0 ELSE 12.0 END
        + CAST(flag.confidence_basis_points AS REAL) / 1000.0) DESC, MIN(flag.name), MIN(flag.source_key) LIMIT ?", clauses.join(" AND "));
    let mut statement = connection.prepare(&sql)?;
    let rows = statement.query_map(params_from_iter(values), |row| {
        Ok((row.get(0)?, row.get(1)?))
    })?;
    rows.collect::<Result<BTreeSet<_>, _>>()
        .map_err(StorageError::from)
}

fn key_clause(keys: &BTreeSet<(String, String)>, values: &mut Vec<Value>) -> String {
    // A string binding is namespace-neutral. Load evidence for the selected
    // value across namespaces; the read API decides its final grouped identity.
    let source_keys = keys
        .iter()
        .map(|(_, key)| key.clone())
        .collect::<BTreeSet<_>>();
    in_clause("flag.source_key", &source_keys, values)
}

fn in_clause(column: &str, identities: &BTreeSet<String>, values: &mut Vec<Value>) -> String {
    values.extend(identities.iter().cloned().map(Value::Text));
    format!("{column} IN ({})", vec!["?"; identities.len()].join(","))
}

fn append_records(
    connection: &Connection,
    predicate: &str,
    values: Vec<Value>,
    records: &mut Vec<CodeFeatureFlagRecord>,
    analyzed_bytes: &mut usize,
) -> Result<(), StorageError> {
    let sql = format!(
        "SELECT {FACT_COLUMNS} FROM code_repository_feature_flags flag WHERE {predicate} ORDER BY path, usage_id LIMIT {}",
        MAX_SCOPE_USAGES + 1
    );
    let mut statement = connection.prepare(&sql)?;
    let mut rows = statement.query(params_from_iter(values))?;
    while let Some(row) = rows.next()? {
        if records.len() == MAX_SCOPE_USAGES {
            return Err(incomplete("scope exceeds 10000 usages"));
        }
        for index in [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 12, 17, 18] {
            *analyzed_bytes = analyzed_bytes.saturating_add(
                row.get_ref(index)?
                    .as_bytes()
                    .map_err(|e| StorageError::InvalidInput(e.to_string()))?
                    .len(),
            );
        }
        if *analyzed_bytes > MAX_ANALYSIS_BYTES {
            return Err(incomplete("scope exceeds 16 MiB fact budget"));
        }
        let json: String = row.get(18)?;
        if json.len() > MAX_METADATA_BYTES {
            return Err(incomplete("metadata exceeds 64 KiB per usage"));
        }
        let metadata =
            serde_json::from_str(&json).map_err(|e| StorageError::InvalidInput(e.to_string()))?;
        records.push(CodeFeatureFlagRecord {
            repository_id: row.get(0)?,
            source_scope: row.get(1)?,
            feature_flag_id: row.get(2)?,
            usage_id: row.get(3)?,
            file_id: row.get(4)?,
            path: row.get(5)?,
            language_id: row.get(6)?,
            name: row.get(7)?,
            source_kind: row.get(8)?,
            source_key: row.get(9)?,
            edge_kind: row.get(10)?,
            confidence_basis_points: row.get(11)?,
            confidence_tier: row.get(12)?,
            byte_range: RepositoryCodeRange {
                start: row.get(13)?,
                end: row.get(14)?,
            },
            line_range: RepositoryCodeRange {
                start: row.get(15)?,
                end: row.get(16)?,
            },
            excerpt: row.get(17)?,
            metadata,
        });
    }
    Ok(())
}

fn incomplete(reason: &str) -> StorageError {
    StorageError::InvalidInput(format!(
        "configuration analysis incomplete: {reason}; narrow --path/--language; consistency absence is unknown"
    ))
}

#[cfg(test)]
#[path = "candidates_tests.rs"]
mod tests;
