//! Symbol fact persistence and role-to-search projection ownership.

use std::{borrow::Cow, sync::OnceLock};

use rusqlite::{ToSql, Transaction, limits::Limit, params_from_iter};

use crate::{
    domain::{RepositoryCodeSymbolRecord, SymbolRole},
    storage::StorageError,
};

use super::SearchDocumentInserter;

const SYMBOL_INSERT_BATCH_SIZE: usize = 1_024;
const SYMBOL_INSERT_COLUMN_COUNT: usize = 17;
const SYMBOL_INSERT_BIND_COUNT: usize = SYMBOL_INSERT_BATCH_SIZE * SYMBOL_INSERT_COLUMN_COUNT;
const SYMBOL_INSERT_ROW: &str = "(?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)";
static SYMBOL_INSERT_FULL_SQL: OnceLock<String> = OnceLock::new();
const _: () = assert!(SYMBOL_INSERT_BIND_COUNT == 17_408);

pub(super) fn insert_records(
    transaction: &Transaction<'_>,
    records: &[RepositoryCodeSymbolRecord],
) -> Result<(), StorageError> {
    if !records.is_empty() {
        insert_symbol_facts(transaction, records)?;
    }
    let mut search_documents = SearchDocumentInserter::new(transaction)?;
    for symbol in records {
        let (role_kind, role_url, role_method) = symbol_role_search_fields(&symbol.symbol_role);
        search_documents.insert(
            &symbol.source_scope,
            "symbol",
            &symbol.symbol_snapshot_id,
            &symbol.path,
            &symbol.language_id,
            [
                symbol.name.as_str(),
                symbol.qualified_name.as_str(),
                symbol.kind.as_str(),
                symbol.signature.as_str(),
                symbol.doc_comment.as_deref().unwrap_or_default(),
                symbol.path.as_str(),
                role_kind.as_str(),
                role_url.as_str(),
                role_method.as_str(),
            ],
        )?;
    }
    search_documents.finish()?;

    Ok(())
}

fn insert_symbol_facts(
    transaction: &Transaction<'_>,
    records: &[RepositoryCodeSymbolRecord],
) -> Result<(), StorageError> {
    let variable_limit = usize::try_from(transaction.limit(Limit::SQLITE_LIMIT_VARIABLE_NUMBER))
        .map_err(|_| {
            StorageError::Invariant(
                "SQLite reported a negative variable limit for symbol persistence".to_owned(),
            )
        })?;
    let rows_within_variable_limit = variable_limit / SYMBOL_INSERT_COLUMN_COUNT;
    let rows_per_statement = rows_within_variable_limit.min(SYMBOL_INSERT_BATCH_SIZE);
    if rows_per_statement == 0 {
        return Err(StorageError::Invariant(format!(
            "SQLite variable limit {variable_limit} cannot admit one {}-column symbol row",
            SYMBOL_INSERT_COLUMN_COUNT
        )));
    }

    let mut complete_groups = records.chunks_exact(rows_per_statement);
    if records.len() >= rows_per_statement {
        let full_sql: Cow<'static, str> = if rows_per_statement == SYMBOL_INSERT_BATCH_SIZE {
            Cow::Borrowed(
                SYMBOL_INSERT_FULL_SQL.get_or_init(|| symbol_insert_sql(SYMBOL_INSERT_BATCH_SIZE)),
            )
        } else {
            Cow::Owned(symbol_insert_sql(rows_per_statement))
        };
        let mut statement = transaction.prepare_cached(full_sql.as_ref())?;
        for symbols in complete_groups.by_ref() {
            execute_symbol_insert(&mut statement, symbols)?;
        }
    }
    let tail_rows = complete_groups.remainder();
    if !tail_rows.is_empty() {
        let sql = symbol_insert_sql(tail_rows.len());
        let mut statement = transaction.prepare(&sql)?;
        execute_symbol_insert(&mut statement, tail_rows)?;
    }

    Ok(())
}

fn execute_symbol_insert(
    statement: &mut rusqlite::Statement<'_>,
    symbols: &[RepositoryCodeSymbolRecord],
) -> Result<(), StorageError> {
    let role_json = symbols
        .iter()
        .map(|symbol| symbol_role_json(&symbol.symbol_role))
        .collect::<Result<Vec<_>, _>>()?;
    let mut values: Vec<&dyn ToSql> =
        Vec::with_capacity(symbols.len() * SYMBOL_INSERT_COLUMN_COUNT);
    for (symbol, role_json) in symbols.iter().zip(&role_json) {
        values.push(&symbol.repository_id);
        values.push(&symbol.source_scope);
        values.push(&symbol.symbol_snapshot_id);
        values.push(&symbol.canonical_symbol_id);
        values.push(&symbol.file_id);
        values.push(&symbol.path);
        values.push(&symbol.language_id);
        values.push(&symbol.name);
        values.push(&symbol.qualified_name);
        values.push(&symbol.kind);
        values.push(&symbol.signature);
        values.push(&symbol.doc_comment);
        values.push(&symbol.byte_range.start);
        values.push(&symbol.byte_range.end);
        values.push(&symbol.line_range.start);
        values.push(&symbol.line_range.end);
        values.push(role_json);
    }
    statement.execute(params_from_iter(values))?;

    Ok(())
}

fn symbol_insert_sql(row_count: usize) -> String {
    let placeholders = std::iter::repeat_n(SYMBOL_INSERT_ROW, row_count)
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "
        INSERT INTO code_repository_symbols (
            repository_id, source_scope, symbol_snapshot_id, canonical_symbol_id,
            file_id, path, language_id, name,
            qualified_name, kind, signature, doc_comment, byte_start, byte_end,
            line_start, line_end, symbol_role_json
        )
        VALUES {placeholders}
        "
    )
}

fn symbol_role_json(role: &Option<SymbolRole>) -> Result<Option<String>, StorageError> {
    role.as_ref()
        .map(serde_json::to_string)
        .transpose()
        .map_err(|error| StorageError::InvalidInput(error.to_string()))
}

fn symbol_role_search_fields(role: &Option<SymbolRole>) -> (String, String, String) {
    match role {
        Some(SymbolRole::RouteHandler { url, http_method }) => {
            ("route_handler".to_owned(), url.clone(), http_method.clone())
        }
        Some(SymbolRole::RouteHandlers { routes }) => (
            "route_handler".to_owned(),
            routes
                .iter()
                .map(|route| route.url.as_str())
                .collect::<Vec<_>>()
                .join(" "),
            routes
                .iter()
                .map(|route| route.http_method.as_str())
                .collect::<Vec<_>>()
                .join(" "),
        ),
        None => (String::new(), String::new(), String::new()),
    }
}

#[cfg(test)]
#[path = "mod_tests.rs"]
mod mod_tests;
