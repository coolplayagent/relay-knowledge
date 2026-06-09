use std::collections::BTreeSet;

use rusqlite::{Connection, params_from_iter, types::Value};

use crate::storage::StorageError;

use super::super::code_query_prepare::retry_code_search_operation;

const MAX_CANDIDATE_PATH_FTS_TERMS: usize = 8;

pub(in crate::storage::sqlite) fn file_candidate_paths_for_scope(
    connection: &mut Connection,
    source_scope: &str,
    path_filters: &[String],
    language_filters: &[String],
    exclude_generated: bool,
    limit: usize,
) -> Result<Vec<String>, StorageError> {
    if limit == 0 {
        return Ok(Vec::new());
    }
    let path_filter = path_filter_sql_for_column("path", path_filters);
    let language_filter = language_filter_sql_for_column("language_id", language_filters);
    let generated_filter = generated_filter_sql(exclude_generated, "is_generated");
    let sql = format!(
        "
        SELECT path
        FROM code_repository_files
        WHERE source_scope = ?
          {path_filter}
          {language_filter}
          {generated_filter}
        ORDER BY path ASC
        LIMIT ?
        "
    );
    let mut values = vec![Value::Text(source_scope.to_owned())];
    push_path_filter_values(&mut values, path_filters);
    push_language_filter_values(&mut values, language_filters);
    values.push(Value::Integer(limit as i64));
    let mut statement = connection.prepare(&sql)?;
    let rows = statement.query_map(params_from_iter(values), |row| row.get(0))?;

    rows.collect::<Result<Vec<_>, _>>()
        .map_err(StorageError::from)
}

pub(in crate::storage::sqlite) fn file_candidate_paths_for_query_scope(
    connection: &mut Connection,
    source_scope: &str,
    query: &str,
    path_filters: &[String],
    language_filters: &[String],
    exclude_generated: bool,
    limit: usize,
) -> Result<Vec<String>, StorageError> {
    match retry_code_search_operation(|| {
        file_candidate_paths_from_search(
            connection,
            source_scope,
            query,
            path_filters,
            language_filters,
            exclude_generated,
            limit,
        )
    }) {
        Ok(paths) if !paths.is_empty() => Ok(paths),
        Ok(_) => file_candidate_paths_for_scope(
            connection,
            source_scope,
            path_filters,
            language_filters,
            exclude_generated,
            limit,
        ),
        Err(error) if candidate_path_search_can_use_scope_fallback(&error) => {
            let paths = file_candidate_paths_from_indexed_content(
                connection,
                source_scope,
                query,
                path_filters,
                language_filters,
                exclude_generated,
                limit,
            )?;
            if paths.is_empty() {
                return Err(error);
            }

            Ok(paths)
        }
        Err(error) => Err(error),
    }
}

fn file_candidate_paths_from_search(
    connection: &mut Connection,
    source_scope: &str,
    query: &str,
    path_filters: &[String],
    language_filters: &[String],
    exclude_generated: bool,
    limit: usize,
) -> Result<Vec<String>, StorageError> {
    if limit == 0 {
        return Ok(Vec::new());
    }
    let Some(fts_query) = candidate_path_fts_query(query) else {
        return Ok(Vec::new());
    };
    let path_filter = path_filter_sql_for_column("path", path_filters);
    let language_filter = language_filter_sql_for_column("language_id", language_filters);
    let generated_filter = search_generated_filter_sql(exclude_generated);
    let sql = format!(
        "
        SELECT path
        FROM code_repository_search
        WHERE code_repository_search MATCH ?
          AND source_scope = ?
          {path_filter}
          {language_filter}
          {generated_filter}
        GROUP BY path
        ORDER BY MIN(rank) ASC, path ASC
        LIMIT ?
        "
    );
    let mut values = vec![Value::Text(fts_query), Value::Text(source_scope.to_owned())];
    push_path_filter_values(&mut values, path_filters);
    push_language_filter_values(&mut values, language_filters);
    values.push(Value::Integer(limit as i64));
    let mut statement = connection.prepare(&sql)?;
    let rows = statement.query_map(params_from_iter(values), |row| row.get(0))?;

    rows.collect::<Result<Vec<_>, _>>()
        .map_err(StorageError::from)
}

fn file_candidate_paths_from_indexed_content(
    connection: &mut Connection,
    source_scope: &str,
    query: &str,
    path_filters: &[String],
    language_filters: &[String],
    exclude_generated: bool,
    limit: usize,
) -> Result<Vec<String>, StorageError> {
    if limit == 0 {
        return Ok(Vec::new());
    }
    let terms = candidate_path_fts_terms(query)
        .into_iter()
        .map(|term| term.to_ascii_lowercase())
        .collect::<Vec<_>>();
    if terms.is_empty() {
        return Ok(Vec::new());
    }

    let path_filter = path_filter_sql_for_column("f.path", path_filters);
    let language_filter = language_filter_sql_for_column("f.language_id", language_filters);
    let generated_filter = generated_filter_sql(exclude_generated, "f.is_generated");
    let term_filter = terms
        .iter()
        .map(|_| "(instr(lower(f.path), ?) > 0 OR instr(lower(COALESCE(c.content, '')), ?) > 0)")
        .collect::<Vec<_>>()
        .join(" OR ");
    let term_score = terms
        .iter()
        .map(|_| {
            "MAX(CASE WHEN instr(lower(f.path), ?) > 0 THEN 4 ELSE 0 END) \
             + SUM(CASE WHEN instr(lower(COALESCE(c.content, '')), ?) > 0 THEN 1 ELSE 0 END)"
        })
        .collect::<Vec<_>>()
        .join(" + ");
    let sql = format!(
        "
        SELECT f.path
        FROM code_repository_files f
        LEFT JOIN code_repository_chunks c
          ON c.source_scope = f.source_scope
         AND c.path = f.path
        WHERE f.source_scope = ?
          {path_filter}
          {language_filter}
          {generated_filter}
          AND ({term_filter})
        GROUP BY f.path
        ORDER BY ({term_score}) DESC, f.path ASC
        LIMIT ?
        "
    );
    let mut values = vec![Value::Text(source_scope.to_owned())];
    push_path_filter_values(&mut values, path_filters);
    push_language_filter_values(&mut values, language_filters);
    push_candidate_path_term_values(&mut values, &terms);
    push_candidate_path_term_values(&mut values, &terms);
    values.push(Value::Integer(limit as i64));
    let mut statement = connection.prepare(&sql)?;
    let rows = statement.query_map(params_from_iter(values), |row| row.get(0))?;

    rows.collect::<Result<Vec<_>, _>>()
        .map_err(StorageError::from)
}

fn generated_filter_sql(exclude_generated: bool, column: &str) -> &'static str {
    if exclude_generated {
        return match column {
            "is_generated" => "AND is_generated = 0",
            "f.is_generated" => "AND f.is_generated = 0",
            _ => "",
        };
    }

    ""
}

fn search_generated_filter_sql(exclude_generated: bool) -> &'static str {
    if exclude_generated {
        return "
          AND NOT EXISTS (
              SELECT 1
              FROM code_repository_files generated_file
              WHERE generated_file.source_scope = code_repository_search.source_scope
                AND generated_file.path = code_repository_search.path
                AND generated_file.is_generated != 0
          )
        ";
    }

    ""
}

fn push_candidate_path_term_values(values: &mut Vec<Value>, terms: &[String]) {
    for term in terms {
        values.push(Value::Text(term.clone()));
        values.push(Value::Text(term.clone()));
    }
}

fn candidate_path_search_can_use_scope_fallback(error: &StorageError) -> bool {
    let StorageError::Sqlite(error) = error else {
        return false;
    };
    let message = error.to_string();
    message.contains("vtable constructor failed: code_repository_search")
        || message.contains("no such table: code_repository_search")
        || message.contains("no such module: fts5")
}

pub(in crate::storage::sqlite) fn candidate_path_fts_query(query: &str) -> Option<String> {
    let terms = candidate_path_fts_terms(query)
        .into_iter()
        .map(|term| format!("\"{}\"", term.replace('"', "\"\"")))
        .collect::<Vec<_>>();
    (!terms.is_empty()).then(|| terms.join(" OR "))
}

fn candidate_path_fts_terms(query: &str) -> Vec<String> {
    let terms = query
        .split(|character: char| !(character.is_ascii_alphanumeric() || character == '_'))
        .map(str::trim)
        .filter(|term| !term.is_empty())
        .fold(Vec::<String>::new(), |mut terms, term| {
            if !terms
                .iter()
                .any(|existing| existing.eq_ignore_ascii_case(term))
            {
                terms.push(term.to_owned());
            }
            terms
        });
    if terms.len() <= MAX_CANDIDATE_PATH_FTS_TERMS {
        return terms;
    }

    let mut ranked = terms
        .iter()
        .enumerate()
        .map(|(position, term)| (candidate_path_term_priority(term), position, term))
        .collect::<Vec<_>>();
    ranked.sort_by(|left, right| {
        right
            .0
            .cmp(&left.0)
            .then_with(|| right.1.cmp(&left.1))
            .then_with(|| left.2.cmp(right.2))
    });
    let selected = ranked
        .into_iter()
        .take(MAX_CANDIDATE_PATH_FTS_TERMS)
        .map(|(_, position, _)| position)
        .collect::<BTreeSet<_>>();

    terms
        .into_iter()
        .enumerate()
        .filter_map(|(position, term)| selected.contains(&position).then_some(term))
        .collect()
}

fn candidate_path_term_priority(term: &str) -> usize {
    let length_score = term.chars().count().min(64);
    let structure_bonus = if term
        .chars()
        .any(|character| character == '_' || character.is_ascii_uppercase())
    {
        16
    } else {
        0
    };
    let digit_bonus = if term.chars().any(|character| character.is_ascii_digit()) {
        4
    } else {
        0
    };

    length_score + structure_bonus + digit_bonus
}

fn path_filter_sql_for_column(column: &str, filters: &[String]) -> String {
    let clauses = filters
        .iter()
        .filter_map(|filter| normalized_sql_path_filter(filter))
        .map(|_| format!("({column} = ? OR {column} LIKE ? ESCAPE '\\')"))
        .collect::<Vec<_>>();
    if clauses.is_empty() {
        String::new()
    } else {
        format!("AND ({})", clauses.join(" OR "))
    }
}

fn language_filter_sql_for_column(column: &str, filters: &[String]) -> String {
    let clauses = filters
        .iter()
        .map(|_| format!("{column} = ?"))
        .collect::<Vec<_>>();
    if clauses.is_empty() {
        String::new()
    } else {
        format!("AND ({})", clauses.join(" OR "))
    }
}

fn push_path_filter_values(values: &mut Vec<Value>, filters: &[String]) {
    for filter in filters
        .iter()
        .filter_map(|filter| normalized_sql_path_filter(filter))
    {
        values.push(Value::Text(filter.clone()));
        values.push(Value::Text(format!("{}/%", escape_sql_like(&filter))));
    }
}

fn push_language_filter_values(values: &mut Vec<Value>, filters: &[String]) {
    values.extend(filters.iter().cloned().map(Value::Text));
}

fn normalized_sql_path_filter(filter: &str) -> Option<String> {
    let mut filter = filter.trim_end_matches(['/', '\\']);
    while let Some(stripped) = filter.strip_prefix("./") {
        filter = stripped;
    }
    (!filter.is_empty() && filter != ".").then(|| filter.to_owned())
}

fn escape_sql_like(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_")
}
