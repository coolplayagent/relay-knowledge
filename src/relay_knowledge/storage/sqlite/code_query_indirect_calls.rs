use rusqlite::{Connection, params_from_iter, types::Value};

use super::super::{
    code_query_rows::CallRow,
    code_query_support::{
        fts_match_query, fts_path_and_language_filter_sql, language_filter_sql_for_columns,
        path_filter_sql_for_column, push_language_filter_values, push_path_filter_values,
    },
    prepare_code_search_statement, required_scope,
};
use super::{
    CallIdentityDirection, CallIdentityQuery, CallIdentityRows, call_identity_candidate_limit,
    call_rows_sql, identifier_character, row_to_call,
};
use crate::{
    domain::{CodeRepositoryStatus, CodeRetrievalRequest},
    storage::StorageError,
};

struct IndirectCallBinding {
    field_name: String,
    target_name: String,
    binding_path: String,
}

struct IndirectCallBindings {
    bindings: Vec<IndirectCallBinding>,
    saturated: bool,
}

const INDIRECT_CALL_BINDING_LIMIT: usize = 80;
const MAX_INDIRECT_CALL_FIELDS: usize = 24;

pub(super) fn search_indirect_call_identity_rows(
    connection: &Connection,
    status: &CodeRepositoryStatus,
    request: &CodeRetrievalRequest,
    identity: &CallIdentityQuery,
) -> Result<CallIdentityRows, StorageError> {
    if identity.direction != CallIdentityDirection::Callee {
        return Ok(CallIdentityRows {
            rows: Vec::new(),
            saturated: false,
        });
    }
    let bindings =
        search_indirect_call_bindings(connection, status, request, identity.leaf_name())?;
    if bindings.bindings.is_empty() {
        return Ok(CallIdentityRows {
            rows: Vec::new(),
            saturated: bindings.saturated,
        });
    }

    let mut field_names = Vec::new();
    for binding in &bindings.bindings {
        if !field_names.contains(&binding.field_name) {
            field_names.push(binding.field_name.clone());
        }
        if field_names.len() >= MAX_INDIRECT_CALL_FIELDS {
            break;
        }
    }

    let path_filter = path_filter_sql_for_column("c.path", status, request);
    let language_filter =
        language_filter_sql_for_columns("f.language_id", "f.path", status, request);
    let placeholders = placeholders(field_names.len());
    let sql = call_rows_sql(&format!(
        "
          AND c.callee_name IN ({placeholders})
          {path_filter}
          {language_filter}
        "
    ));
    let direct_limit = call_identity_candidate_limit(request);
    let mut values = vec![Value::Text(required_scope(status)?.to_owned())];
    values.extend(field_names.into_iter().map(Value::Text));
    push_path_filter_values(&mut values, &status.path_filters);
    push_path_filter_values(&mut values, &request.repository.path_filters);
    push_language_filter_values(&mut values, &status.language_filters);
    push_language_filter_values(&mut values, &request.repository.language_filters);
    values.push(Value::Integer((direct_limit + 1) as i64));

    let mut statement = prepare_code_search_statement(connection, &sql)?;
    let rows = statement.query_map(params_from_iter(values), row_to_call)?;
    let mut rows = rows
        .collect::<Result<Vec<_>, _>>()
        .map_err(StorageError::from)?;
    let saturated = rows.len() > direct_limit;
    rows.truncate(direct_limit);
    rows.retain_mut(|row| {
        let Some(binding) = best_indirect_call_binding(&bindings.bindings, row) else {
            return false;
        };
        let same_path = row.path == binding.binding_path;
        row.target_hint = Some(binding.target_name.clone());
        row.resolution_state = "inferred".to_owned();
        let confidence_floor = if same_path { 7_500 } else { 5_500 };
        row.confidence_basis_points = row.confidence_basis_points.max(confidence_floor);
        row.confidence_tier = "inferred".to_owned();
        true
    });

    Ok(CallIdentityRows {
        rows,
        saturated: saturated || bindings.saturated,
    })
}

fn search_indirect_call_bindings(
    connection: &Connection,
    status: &CodeRepositoryStatus,
    request: &CodeRetrievalRequest,
    target_name: &str,
) -> Result<IndirectCallBindings, StorageError> {
    let fts_filter = fts_path_and_language_filter_sql(status, request);
    let sql = format!(
        "
        SELECT path, content
        FROM code_repository_search
        WHERE code_repository_search MATCH ?
          AND source_scope = ?
          AND document_kind = 'chunk'
          {fts_filter}
        ORDER BY bm25(code_repository_search) ASC, record_id ASC
        LIMIT ?
        "
    );
    let mut values = vec![
        Value::Text(fts_match_query(target_name)),
        Value::Text(required_scope(status)?.to_owned()),
    ];
    push_path_filter_values(&mut values, &status.path_filters);
    push_path_filter_values(&mut values, &request.repository.path_filters);
    push_language_filter_values(&mut values, &status.language_filters);
    push_language_filter_values(&mut values, &request.repository.language_filters);
    values.push(Value::Integer((INDIRECT_CALL_BINDING_LIMIT + 1) as i64));

    let mut statement = prepare_code_search_statement(connection, &sql)?;
    let rows = statement.query_map(params_from_iter(values), |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
    })?;
    let mut rows = rows
        .collect::<Result<Vec<_>, _>>()
        .map_err(StorageError::from)?;
    let saturated = rows.len() > INDIRECT_CALL_BINDING_LIMIT;
    rows.truncate(INDIRECT_CALL_BINDING_LIMIT);
    let mut bindings: Vec<IndirectCallBinding> = Vec::new();
    for (path, excerpt) in rows {
        for field_name in indirect_call_binding_fields(&excerpt, target_name) {
            let binding = IndirectCallBinding {
                field_name,
                target_name: target_name.to_owned(),
                binding_path: path.clone(),
            };
            if !bindings.iter().any(|existing| {
                existing.field_name == binding.field_name
                    && existing.binding_path == binding.binding_path
            }) {
                bindings.push(binding);
            }
        }
    }

    Ok(IndirectCallBindings {
        bindings,
        saturated,
    })
}

fn best_indirect_call_binding<'a>(
    bindings: &'a [IndirectCallBinding],
    row: &CallRow,
) -> Option<&'a IndirectCallBinding> {
    bindings.iter().find(|binding| {
        binding.field_name == row.callee_name
            && (binding.binding_path == row.path
                || row_has_indirect_target_evidence(row, &binding.target_name))
    })
}

fn row_has_indirect_target_evidence(row: &CallRow, target_name: &str) -> bool {
    matches!(row.resolution_state.as_str(), "resolved" | "inferred")
        && row.confidence_basis_points >= 5_000
        && [
            row.target_hint.as_deref(),
            row.callee_canonical_symbol_id.as_deref(),
            row.callee_signature.as_deref(),
        ]
        .into_iter()
        .flatten()
        .any(|field| line_contains_identifier(field, target_name))
}

fn indirect_call_binding_fields(excerpt: &str, target_name: &str) -> Vec<String> {
    let mut fields = Vec::new();
    for line in excerpt.lines() {
        if !line_contains_identifier(line, target_name) {
            continue;
        }
        if let Some(field_name) = field_name_before_bound_target(line, target_name)
            && !fields.contains(&field_name)
        {
            fields.push(field_name);
        }
    }

    fields
}

fn field_name_before_bound_target(line: &str, target_name: &str) -> Option<String> {
    let target_start = identifier_start(line, target_name)?;
    let before_target = line.get(..target_start)?;
    let assignment_start = before_target
        .rfind('=')
        .or_else(|| before_target.rfind(':'))?;
    let left = before_target.get(..assignment_start)?.trim_end();
    field_name_from_member_surface(left).filter(|field_name| field_name != target_name)
}

fn field_name_from_member_surface(value: &str) -> Option<String> {
    if let Some((_, tail)) = value.rsplit_once("->") {
        return leading_identifier(tail.trim_start());
    }
    if let Some((_, tail)) = value.rsplit_once('.') {
        return leading_identifier(tail.trim_start());
    }

    None
}

fn leading_identifier(value: &str) -> Option<String> {
    let mut end = 0usize;
    for (index, character) in value.char_indices() {
        if index == 0 && !identifier_start_character(character) {
            return None;
        }
        if !identifier_character(character) {
            break;
        }
        end = index + character.len_utf8();
    }
    (end > 0).then(|| value[..end].to_owned())
}

fn line_contains_identifier(line: &str, identifier: &str) -> bool {
    identifier_start(line, identifier).is_some()
}

fn identifier_start(line: &str, identifier: &str) -> Option<usize> {
    if identifier.is_empty() {
        return None;
    }
    line.match_indices(identifier)
        .find(|(start, _)| {
            let end = start + identifier.len();
            line.get(..*start).is_some_and(|prefix| {
                prefix
                    .chars()
                    .next_back()
                    .is_none_or(|character| !identifier_character(character))
            }) && line.get(end..).is_some_and(|suffix| {
                suffix
                    .chars()
                    .next()
                    .is_none_or(|character| !identifier_character(character))
            })
        })
        .map(|(start, _)| start)
}

fn identifier_start_character(character: char) -> bool {
    character == '_' || character.is_ascii_alphabetic()
}

fn placeholders(count: usize) -> String {
    std::iter::repeat_n("?", count)
        .collect::<Vec<_>>()
        .join(", ")
}
