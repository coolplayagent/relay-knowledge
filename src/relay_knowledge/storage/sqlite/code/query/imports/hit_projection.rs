//! Import-row enrichment, grouped excerpts, ranking, and hit projection.

use std::collections::{BTreeMap, BTreeSet};

use rusqlite::{Connection, params_from_iter, types::Value};

use crate::{
    domain::{
        CodeQueryKind, CodeRepositoryStatus, CodeRetrievalHit, CodeRetrievalLayer,
        CodeRetrievalRequest, RepositoryCodeRange,
    },
    storage::StorageError,
};

use super::super::{
    HitParts, hit_from_parts, prepare_code_search_statement,
    relevance::{ScoreQuery, scoped_identity_query_bonus, score_exact_path},
    required_scope,
    rows::ImportRow,
    scoring::path_ranking::{import_test_path_penalty, query_mentions_test_or_benchmark},
    selected_row,
};
use super::{
    path_context::{import_path_lookup_token, query_looks_like_import_path},
    scoring::{
        ImportSourceSignificance, ImporterPathContext, hybrid_import_sparse_query_penalty,
        import_binding_context_bonus, import_importer_path_context_bonus, import_line_priority,
        import_public_dependency_surface_bonus, import_reexport_surface_penalty,
        import_same_file_usage_bonus, import_self_implementation_penalty,
        import_single_module_path_tiebreaker_bonus, import_source_path_query_overlap_bonus,
        import_source_significance_bonus, import_statement_shape_bonus, import_surface_bonus,
        import_target_directory_bonus, import_target_symbol_bonus,
    },
    targets::{attach_import_query_usage_context, attach_import_target_symbols},
};

const MAX_IMPORT_GROUP_LOOKUP_KEYS: usize = 128;
const MAX_IMPORT_GROUP_MODULES: usize = 24;

pub(super) fn import_rows_to_hits(
    connection: &Connection,
    status: &CodeRepositoryStatus,
    request: &CodeRetrievalRequest,
    mut rows: Vec<ImportRow>,
) -> Result<Vec<CodeRetrievalHit>, StorageError> {
    if request.code_query_kind == CodeQueryKind::Imports
        && query_looks_like_import_path(&request.query)
    {
        attach_import_target_symbols(connection, status, &mut rows)?;
    }
    attach_import_query_usage_context(connection, status, request, &mut rows)?;
    let group_modules = import_group_modules(connection, status, &rows)?;

    let scoring_query = import_scoring_query(request);
    let query = scoring_query.to_lowercase();
    let score_query = ScoreQuery::new(scoring_query);
    let query_has_test_intent = query_mentions_test_or_benchmark(&request.query);

    Ok(rows
        .into_iter()
        .filter(|row| {
            selected_row(
                &row.path,
                &row.language_id,
                row.is_generated,
                status,
                request,
            )
        })
        .filter_map(|row| {
            let excerpt = import_excerpt(
                &row.language_id,
                &row.module,
                row.target_symbol_names.as_deref(),
                group_modules
                    .get(&ImportGroupKey::from_row(&row))
                    .map(Vec::as_slice)
                    .unwrap_or(&[]),
            );
            let base_score = score_query.score([
                row.module.as_str(),
                row.target_hint.as_deref().unwrap_or_default(),
                row.matched_symbol_name.as_deref().unwrap_or_default(),
            ]) + score_exact_path(&query, &row.path)
                + scoped_identity_query_bonus(
                    scoring_query,
                    [
                        row.target_hint.as_deref().unwrap_or_default(),
                        row.matched_symbol_name.as_deref().unwrap_or_default(),
                    ],
                )
                + import_target_symbol_bonus(scoring_query, row.matched_symbol_name.as_deref());
            let score = base_score
                + import_resolution_confidence_bonus(
                    base_score,
                    &row.resolution_state,
                    row.confidence_basis_points,
                    request.code_query_kind,
                )
                + import_same_file_usage_bonus(
                    base_score,
                    row.same_file_query_usage_count,
                    request.code_query_kind,
                )
                + import_importer_path_context_bonus(
                    base_score,
                    row.same_file_query_usage_count,
                    scoring_query,
                    &ImporterPathContext {
                        path: &row.path,
                        module: &row.module,
                        target_hint: row.target_hint.as_deref(),
                        matched_symbol_name: row.matched_symbol_name.as_deref(),
                        target_symbol_names: row.target_symbol_names.as_deref(),
                    },
                    request.code_query_kind,
                )
                + import_target_directory_bonus(
                    base_score,
                    scoring_query,
                    &row.path,
                    row.target_hint.as_deref(),
                    request.code_query_kind,
                )
                + import_binding_context_bonus(
                    base_score,
                    scoring_query,
                    &row.module,
                    request.code_query_kind,
                )
                + import_statement_shape_bonus(
                    base_score,
                    &request.query,
                    &row.module,
                    request.code_query_kind,
                )
                + import_line_priority(base_score, row.line_range.start, scoring_query)
                + hybrid_import_sparse_query_penalty(
                    base_score,
                    scoring_query,
                    &row.path,
                    &row.module,
                    row.target_hint.as_deref(),
                    row.matched_symbol_name.as_deref(),
                    request.code_query_kind,
                )
                + import_public_dependency_surface_bonus(
                    base_score,
                    scoring_query,
                    &row.path,
                    row.target_hint.as_deref(),
                    request.code_query_kind,
                )
                + import_source_path_query_overlap_bonus(
                    base_score,
                    scoring_query,
                    &row.path,
                    row.target_hint.as_deref(),
                    request.code_query_kind,
                )
                + import_self_implementation_penalty(
                    base_score,
                    scoring_query,
                    &row.path,
                    row.target_hint.as_deref(),
                    request.code_query_kind,
                )
                + import_single_module_path_tiebreaker_bonus(
                    base_score,
                    scoring_query,
                    &row.path,
                    &row.module,
                    row.target_hint.as_deref(),
                    request.code_query_kind,
                )
                + import_source_significance_bonus(
                    base_score,
                    scoring_query,
                    &ImportSourceSignificance {
                        path: &row.path,
                        is_generated: row.is_generated,
                        module: &row.module,
                        target_hint: row.target_hint.as_deref(),
                        source_line_count: row.source_line_count,
                    },
                    request.code_query_kind,
                )
                + import_reexport_surface_penalty(
                    base_score,
                    scoring_query,
                    &row.path,
                    &row.module,
                    row.target_hint.as_deref(),
                    request.code_query_kind,
                )
                + import_test_path_penalty(base_score, &row.path, request, query_has_test_intent)
                + import_surface_bonus(base_score, &row.path, request.code_query_kind);
            (score > 0.0).then(|| {
                hit_from_parts(
                    status,
                    HitParts {
                        path: row.path,
                        language_id: row.language_id,
                        byte_range: RepositoryCodeRange { start: 0, end: 0 },
                        line_range: row.line_range,
                        symbol_snapshot_id: None,
                        canonical_symbol_id: None,
                        file_id: Some(row.file_id),
                        retrieval_layers: vec![CodeRetrievalLayer::ImportGraph],
                        score: score + 1.0,
                        excerpt,
                        is_generated: row.is_generated,
                        degraded_reason: None,
                        edge_kind: Some("import".to_owned()),
                        edge_resolution_state: Some(row.resolution_state),
                        edge_target_hint: row.target_hint,
                        edge_confidence_basis_points: Some(row.confidence_basis_points),
                        edge_confidence_tier: Some(row.confidence_tier),
                    },
                )
            })
        })
        .collect())
}

fn import_scoring_query(request: &CodeRetrievalRequest) -> &str {
    let Some(path_token) = import_path_lookup_token(&request.query) else {
        return &request.query;
    };
    let contextual = request
        .query
        .split_whitespace()
        .any(|token| !token.contains(path_token));
    if contextual {
        &request.query
    } else {
        path_token
    }
}

fn import_resolution_confidence_bonus(
    base_score: f64,
    resolution_state: &str,
    confidence_basis_points: u16,
    kind: CodeQueryKind,
) -> f64 {
    if base_score <= 0.0 || kind != CodeQueryKind::Imports {
        return 0.0;
    }
    match (resolution_state, confidence_basis_points) {
        ("resolved", confidence) if confidence >= 7_500 => 0.3,
        ("unresolved", confidence) if confidence <= 2_500 => -0.2,
        _ => 0.0,
    }
}

#[derive(Debug, Clone, Eq, Ord, PartialEq, PartialOrd)]
struct ImportGroupKey {
    path: String,
    line_start: u32,
    line_end: u32,
}

impl ImportGroupKey {
    fn from_row(row: &ImportRow) -> Self {
        Self {
            path: row.path.clone(),
            line_start: row.line_range.start,
            line_end: row.line_range.end,
        }
    }
}

fn import_group_modules(
    connection: &Connection,
    status: &CodeRepositoryStatus,
    rows: &[ImportRow],
) -> Result<BTreeMap<ImportGroupKey, Vec<String>>, StorageError> {
    let keys = rows
        .iter()
        .map(ImportGroupKey::from_row)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .take(MAX_IMPORT_GROUP_LOOKUP_KEYS)
        .collect::<Vec<_>>();
    if keys.is_empty() {
        return Ok(BTreeMap::new());
    }

    let key_rows = import_group_key_rows(keys.len());
    let sql = format!(
        "
        SELECT path, module, line_start, line_end
        FROM code_repository_imports
        WHERE source_scope = ?
          AND (path, line_start, line_end) IN (VALUES {key_rows})
        ORDER BY path ASC, line_start ASC, module ASC
        "
    );
    let mut values = vec![Value::Text(required_scope(status)?.to_owned())];
    for key in &keys {
        values.push(Value::Text(key.path.clone()));
        values.push(Value::Integer(i64::from(key.line_start)));
        values.push(Value::Integer(i64::from(key.line_end)));
    }

    let mut statement = prepare_code_search_statement(connection, &sql)?;
    let modules = statement.query_map(params_from_iter(values), |row| {
        Ok((
            ImportGroupKey {
                path: row.get(0)?,
                line_start: row.get(2)?,
                line_end: row.get(3)?,
            },
            row.get::<_, String>(1)?,
        ))
    })?;
    let mut groups = BTreeMap::<ImportGroupKey, Vec<String>>::new();
    for module in modules {
        let (key, module) = module.map_err(StorageError::from)?;
        let entry = groups.entry(key).or_default();
        if entry.len() < MAX_IMPORT_GROUP_MODULES && !entry.contains(&module) {
            entry.push(module);
        }
    }

    Ok(groups)
}

fn import_group_key_rows(key_count: usize) -> String {
    std::iter::repeat_n("(?, ?, ?)", key_count)
        .collect::<Vec<_>>()
        .join(", ")
}

fn import_excerpt(
    language_id: &str,
    module: &str,
    target_symbol_names: Option<&str>,
    group_modules: &[String],
) -> String {
    let mut excerpt_modules = Vec::with_capacity(group_modules.len().saturating_add(1));
    excerpt_modules.push(source_like_import_module(language_id, module));
    for group_module in group_modules {
        if group_module != module {
            let rendered = source_like_import_module(language_id, group_module);
            if !excerpt_modules.contains(&rendered) {
                excerpt_modules.push(rendered);
            }
        }
    }

    let mut excerpt = excerpt_modules.join("\n");
    if let Some(target_symbol_names) = target_symbol_names
        .map(str::trim)
        .filter(|target_symbol_names| !target_symbol_names.is_empty())
    {
        excerpt.push_str(" target symbols: ");
        excerpt.push_str(target_symbol_names);
    }

    excerpt
}

fn source_like_import_module(language_id: &str, module: &str) -> String {
    let trimmed = module.trim();
    if trimmed.starts_with("import ")
        || trimmed.starts_with("from ")
        || trimmed.starts_with("#include")
        || trimmed.starts_with("require ")
        || trimmed.starts_with("use ")
        || trimmed.starts_with("using ")
        || trimmed.starts_with(". \"")
        || trimmed.starts_with(". '")
        || trimmed.starts_with(". $")
    {
        return trimmed.to_owned();
    }
    let parts = trimmed.split_whitespace().collect::<Vec<_>>();
    if language_id == "go" && parts.len() == 2 && import_alias_like(parts[0]) {
        return format!("import {} \"{}\"", parts[0], parts[1]);
    }
    if language_id == "go" && parts.len() == 1 && import_path_like(parts[0]) {
        return format!("import \"{trimmed}\"");
    }
    if parts.len() == 1 && import_path_like(parts[0]) {
        return format!("{trimmed} (\"{trimmed}\")");
    }

    trimmed.to_owned()
}

fn import_alias_like(value: &str) -> bool {
    if matches!(value, "." | "_") {
        return true;
    }
    value
        .chars()
        .next()
        .is_some_and(|character| character == '_' || character.is_ascii_alphabetic())
        && value
            .chars()
            .all(|character| character == '_' || character.is_ascii_alphanumeric())
}

fn import_path_like(value: &str) -> bool {
    value
        .chars()
        .all(|character| character.is_ascii_alphanumeric() || matches!(character, '_' | '-' | '/'))
}

#[cfg(test)]
#[path = "hit_projection_tests.rs"]
mod tests;
