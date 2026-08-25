use rusqlite::{Connection, OptionalExtension, params};

use crate::storage::StorageError;

use super::migrations::table_has_columns;

#[derive(Clone, Copy)]
struct SearchQueryIndexDescriptor {
    name: &'static str,
    table: &'static str,
    sql: &'static str,
    columns: &'static [&'static str],
    mode: SearchQueryIndexMode,
    required_table: Option<&'static str>,
    required_table_columns: &'static [&'static str],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SearchQueryIndexMode {
    Required,
    /// Required, plus safe to precreate for a fresh multi-batch Restart while
    /// the shared chunks owner has no rows.
    RequiredForEmptyChunkOwnerRestart,
    /// Keeps its stable ordinal and exact legacy shape, but is never created
    /// or dropped by the current plan.
    Retired,
}

/// Durable progress for deferred query-index construction.
///
/// SQLite commits each index definition atomically. The checkpoint subphase is
/// the durable cursor; persisted schema is validated against it before the
/// next call creates at most one index.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::storage::sqlite::code) enum SearchQueryIndexAdvance {
    Created {
        completed_unit: usize,
        plan_complete: bool,
    },
    Complete,
}

const SEARCH_QUERY_INDEXES: &[SearchQueryIndexDescriptor] = &[
    SearchQueryIndexDescriptor {
        name: "code_repository_search_metadata_scope_path",
        table: "code_repository_search_metadata",
        sql: "CREATE INDEX IF NOT EXISTS code_repository_search_metadata_scope_path ON code_repository_search_metadata(source_scope, path)",
        columns: &["source_scope", "path"],
        mode: SearchQueryIndexMode::Required,
        required_table: None,
        required_table_columns: &[],
    },
    SearchQueryIndexDescriptor {
        name: "code_repository_symbols_lookup",
        table: "code_repository_symbols",
        sql: "CREATE INDEX IF NOT EXISTS code_repository_symbols_lookup ON code_repository_symbols(source_scope, name, qualified_name, path)",
        columns: &["source_scope", "name", "qualified_name", "path"],
        mode: SearchQueryIndexMode::Retired,
        required_table: None,
        required_table_columns: &[],
    },
    SearchQueryIndexDescriptor {
        name: "code_repository_symbols_name_path_lookup",
        table: "code_repository_symbols",
        sql: "CREATE INDEX IF NOT EXISTS code_repository_symbols_name_path_lookup ON code_repository_symbols(source_scope, name, path)",
        columns: &["source_scope", "name", "path"],
        mode: SearchQueryIndexMode::Required,
        required_table: None,
        required_table_columns: &[],
    },
    SearchQueryIndexDescriptor {
        name: "code_repository_symbols_path_line_lookup",
        table: "code_repository_symbols",
        sql: "CREATE INDEX IF NOT EXISTS code_repository_symbols_path_line_lookup ON code_repository_symbols(source_scope, path, line_end, line_start)",
        columns: &["source_scope", "path", "line_end", "line_start"],
        mode: SearchQueryIndexMode::Required,
        required_table: None,
        required_table_columns: &[],
    },
    SearchQueryIndexDescriptor {
        name: "code_repository_references_lookup",
        table: "code_repository_references",
        sql: "CREATE INDEX IF NOT EXISTS code_repository_references_lookup ON code_repository_references(source_scope, name, kind, path)",
        columns: &["source_scope", "name", "kind", "path"],
        mode: SearchQueryIndexMode::Required,
        required_table: None,
        required_table_columns: &[],
    },
    SearchQueryIndexDescriptor {
        name: "code_repository_calls_lookup",
        table: "code_repository_calls",
        sql: "CREATE INDEX IF NOT EXISTS code_repository_calls_lookup ON code_repository_calls(source_scope, callee_name, caller_name, path)",
        columns: &["source_scope", "callee_name", "caller_name", "path"],
        mode: SearchQueryIndexMode::Required,
        required_table: None,
        required_table_columns: &[],
    },
    SearchQueryIndexDescriptor {
        name: "code_repository_feature_flags_lookup",
        table: "code_repository_feature_flags",
        sql: "CREATE INDEX IF NOT EXISTS code_repository_feature_flags_lookup ON code_repository_feature_flags(source_scope, name, source_key, edge_kind, path)",
        columns: &["source_scope", "name", "source_key", "edge_kind", "path"],
        mode: SearchQueryIndexMode::Required,
        required_table: None,
        required_table_columns: &[],
    },
    SearchQueryIndexDescriptor {
        name: "code_repository_routes_lookup",
        table: "code_repository_routes",
        sql: "CREATE INDEX IF NOT EXISTS code_repository_routes_lookup ON code_repository_routes(source_scope, url, http_method, path)",
        columns: &["source_scope", "url", "http_method", "path"],
        mode: SearchQueryIndexMode::Required,
        required_table: None,
        required_table_columns: &[],
    },
    SearchQueryIndexDescriptor {
        name: "code_repository_routes_handler_lookup",
        table: "code_repository_routes",
        sql: "CREATE INDEX IF NOT EXISTS code_repository_routes_handler_lookup ON code_repository_routes(source_scope, handler_symbol_snapshot_id, path)",
        columns: &["source_scope", "handler_symbol_snapshot_id", "path"],
        mode: SearchQueryIndexMode::Required,
        required_table: None,
        required_table_columns: &[],
    },
    SearchQueryIndexDescriptor {
        name: "code_repository_imports_lookup",
        table: "code_repository_imports",
        sql: "CREATE INDEX IF NOT EXISTS code_repository_imports_lookup ON code_repository_imports(source_scope, module, path)",
        columns: &["source_scope", "module", "path"],
        mode: SearchQueryIndexMode::Required,
        required_table: None,
        required_table_columns: &[],
    },
    SearchQueryIndexDescriptor {
        name: "code_repository_imports_target_lookup",
        table: "code_repository_imports",
        sql: "CREATE INDEX IF NOT EXISTS code_repository_imports_target_lookup ON code_repository_imports(source_scope, target_hint, path)",
        columns: &["source_scope", "target_hint", "path"],
        mode: SearchQueryIndexMode::Required,
        required_table: None,
        required_table_columns: &[],
    },
    SearchQueryIndexDescriptor {
        name: "code_repository_dependencies_lookup",
        table: "code_repository_dependencies",
        sql: "CREATE INDEX IF NOT EXISTS code_repository_dependencies_lookup ON code_repository_dependencies(source_scope, ecosystem, package_name, path)",
        columns: &["source_scope", "ecosystem", "package_name", "path"],
        mode: SearchQueryIndexMode::Required,
        required_table: None,
        required_table_columns: &[],
    },
    SearchQueryIndexDescriptor {
        name: "code_repository_dependencies_group_lookup",
        table: "code_repository_dependencies",
        sql: "CREATE INDEX IF NOT EXISTS code_repository_dependencies_group_lookup ON code_repository_dependencies(source_scope, dependency_group, path)",
        columns: &["source_scope", "dependency_group", "path"],
        mode: SearchQueryIndexMode::Required,
        required_table: None,
        required_table_columns: &[],
    },
    SearchQueryIndexDescriptor {
        name: "code_repository_chunks_lookup",
        table: "code_repository_chunks",
        sql: "CREATE INDEX IF NOT EXISTS code_repository_chunks_lookup ON code_repository_chunks(source_scope, path)",
        columns: &["source_scope", "path"],
        mode: SearchQueryIndexMode::RequiredForEmptyChunkOwnerRestart,
        required_table: None,
        required_table_columns: &[],
    },
    SearchQueryIndexDescriptor {
        name: "code_repository_chunks_symbol_lookup",
        table: "code_repository_chunks",
        sql: "CREATE INDEX IF NOT EXISTS code_repository_chunks_symbol_lookup ON code_repository_chunks(source_scope, symbol_snapshot_id)",
        columns: &["source_scope", "symbol_snapshot_id"],
        mode: SearchQueryIndexMode::RequiredForEmptyChunkOwnerRestart,
        required_table: None,
        required_table_columns: &[],
    },
    SearchQueryIndexDescriptor {
        name: "code_repository_calls_caller_lookup",
        table: "code_repository_calls",
        sql: "CREATE INDEX IF NOT EXISTS code_repository_calls_caller_lookup ON code_repository_calls(source_scope, caller_name, path, line_start)",
        columns: &["source_scope", "caller_name", "path", "line_start"],
        mode: SearchQueryIndexMode::Required,
        required_table: Some("code_repository_calls"),
        required_table_columns: &["source_scope", "caller_name", "path", "line_start"],
    },
    SearchQueryIndexDescriptor {
        name: "code_repository_imports_scope_path_line_lookup",
        table: "code_repository_imports",
        sql: "CREATE INDEX IF NOT EXISTS code_repository_imports_scope_path_line_lookup ON code_repository_imports(source_scope, path, line_start, line_end)",
        columns: &["source_scope", "path", "line_start", "line_end"],
        mode: SearchQueryIndexMode::Required,
        required_table: None,
        required_table_columns: &[],
    },
];

const _: [(); crate::domain::CODE_QUERY_INDEX_PLAN_UNIT_COUNT] = [(); SEARCH_QUERY_INDEXES.len()];

#[cfg(test)]
#[path = "search_schema_tests.rs"]
mod tests;

pub(super) fn initialize_search_schema(connection: &Connection) -> Result<(), StorageError> {
    connection.execute_batch(
        "
        CREATE VIRTUAL TABLE IF NOT EXISTS code_repository_search USING fts5(
            source_scope UNINDEXED,
            document_kind UNINDEXED,
            record_id UNINDEXED,
            path UNINDEXED,
            language_id UNINDEXED,
            content
        );

        CREATE TABLE IF NOT EXISTS code_repository_search_metadata (
            source_scope TEXT NOT NULL,
            document_kind TEXT NOT NULL,
            record_id TEXT NOT NULL,
            path TEXT NOT NULL,
            search_rowid INTEGER PRIMARY KEY,
            UNIQUE (source_scope, document_kind, record_id)
        );
        DROP INDEX IF EXISTS code_repository_search_metadata_scope_kind;
        CREATE INDEX IF NOT EXISTS code_repository_scopes_lookup
            ON code_repository_scopes(repository_id, resolved_commit_sha, path_filters_json, language_filters_json);
        ",
    )?;
    Ok(())
}

#[cfg(test)]
pub(super) fn ensure_search_query_indexes(connection: &Connection) -> Result<(), StorageError> {
    while let SearchQueryIndexAdvance::Created { .. } =
        advance_search_query_indexes(connection, None, false)?
    {}
    Ok(())
}

/// Creates at most one missing deferred query index.
///
/// Existing indexes are accepted only when their persisted column order is
/// exact. This prevents a name collision from falsely advancing the durable
/// schema cursor after a crash or an incompatible upgrade. A legacy cursor
/// requires retired descriptors in its completed prefix to remain present;
/// current cursors permit an absent retired descriptor as a stable skip.
pub(in crate::storage::sqlite::code) fn advance_search_query_indexes(
    connection: &Connection,
    completed_unit: Option<usize>,
    require_retired_prefix: bool,
) -> Result<SearchQueryIndexAdvance, StorageError> {
    validate_query_index_cursor(completed_unit)?;
    if let Some(completed_position) = completed_unit {
        for descriptor in &SEARCH_QUERY_INDEXES[..=completed_position] {
            let persisted = persisted_query_index_columns(connection, descriptor)?;
            if descriptor.mode == SearchQueryIndexMode::Retired {
                if require_retired_prefix {
                    require_query_index_columns(descriptor, persisted)?;
                } else if let Some(columns) = persisted {
                    require_query_index_columns(descriptor, Some(columns))?;
                }
                continue;
            }
            if !query_index_descriptor_is_applicable(connection, descriptor)? {
                return Err(StorageError::Invariant(format!(
                    "durable query-index finalization unit '{}' is no longer applicable",
                    descriptor.name
                )));
            }
            require_query_index_columns(descriptor, persisted)?;
        }
    }
    advance_search_query_indexes_after_cursor(connection, completed_unit, require_retired_prefix)
}

/// Resumes a preserve-phase repair whose coarse scan already treated a
/// descriptor as inapplicable. A still-inapplicable prefix item with no
/// same-name index remains a stable skip; a collision remains fail-closed.
pub(in crate::storage::sqlite::code) fn advance_search_query_index_repair(
    connection: &Connection,
    completed_unit: Option<usize>,
    require_retired_prefix: bool,
) -> Result<SearchQueryIndexAdvance, StorageError> {
    validate_query_index_cursor(completed_unit)?;
    if let Some(completed_position) = completed_unit {
        for descriptor in &SEARCH_QUERY_INDEXES[..=completed_position] {
            let persisted = persisted_query_index_columns(connection, descriptor)?;
            if descriptor.mode == SearchQueryIndexMode::Retired {
                if require_retired_prefix {
                    require_query_index_columns(descriptor, persisted)?;
                } else if let Some(columns) = persisted {
                    require_query_index_columns(descriptor, Some(columns))?;
                }
                continue;
            }
            if !query_index_descriptor_is_applicable(connection, descriptor)? {
                if let Some(columns) = persisted {
                    require_query_index_columns(descriptor, Some(columns))?;
                }
                continue;
            }
            require_query_index_columns(descriptor, persisted)?;
        }
    }
    advance_search_query_indexes_after_cursor(connection, completed_unit, require_retired_prefix)
}

fn validate_query_index_cursor(completed_unit: Option<usize>) -> Result<(), StorageError> {
    if completed_unit.is_some_and(|unit| unit >= SEARCH_QUERY_INDEXES.len()) {
        return Err(StorageError::Invariant(format!(
            "durable query-index finalization unit {:?} exceeds plan length {}",
            completed_unit,
            SEARCH_QUERY_INDEXES.len()
        )));
    }
    Ok(())
}

fn advance_search_query_indexes_after_cursor(
    connection: &Connection,
    completed_unit: Option<usize>,
    require_retired_prefix: bool,
) -> Result<SearchQueryIndexAdvance, StorageError> {
    let next_position = completed_unit.map_or(0, |position| position + 1);
    for (position, descriptor) in SEARCH_QUERY_INDEXES.iter().enumerate().skip(next_position) {
        let persisted = persisted_query_index_columns(connection, descriptor)?;
        if descriptor.mode == SearchQueryIndexMode::Retired {
            if require_retired_prefix {
                require_query_index_columns(descriptor, persisted)?;
            } else if let Some(columns) = persisted {
                require_query_index_columns(descriptor, Some(columns))?;
            }
            continue;
        }
        if !query_index_descriptor_is_applicable(connection, descriptor)? {
            if let Some(columns) = persisted {
                require_query_index_columns(descriptor, Some(columns))?;
            }
            continue;
        }
        match persisted {
            None => {
                connection.execute(descriptor.sql, [])?;
                require_persisted_query_index(connection, descriptor)?;
                let plan_complete = remaining_query_indexes_are_complete(
                    connection,
                    &SEARCH_QUERY_INDEXES[position + 1..],
                )?;
                return Ok(SearchQueryIndexAdvance::Created {
                    completed_unit: position,
                    plan_complete,
                });
            }
            Some(columns) if query_index_columns_match(&columns, descriptor.columns) => {}
            Some(columns) => {
                return Err(StorageError::Invariant(format!(
                    "query index '{}' has columns {:?}, expected {:?}",
                    descriptor.name, columns, descriptor.columns
                )));
            }
        }
    }

    Ok(SearchQueryIndexAdvance::Complete)
}

fn query_index_descriptor_is_applicable(
    connection: &Connection,
    descriptor: &SearchQueryIndexDescriptor,
) -> Result<bool, StorageError> {
    let Some(required_table) = descriptor.required_table else {
        return Ok(true);
    };
    table_has_columns(
        connection,
        required_table,
        descriptor.required_table_columns,
    )
}

fn query_index_columns_match(actual: &[String], expected: &[&str]) -> bool {
    actual
        .iter()
        .map(String::as_str)
        .eq(expected.iter().copied())
}

fn require_persisted_query_index(
    connection: &Connection,
    descriptor: &SearchQueryIndexDescriptor,
) -> Result<(), StorageError> {
    let actual = persisted_query_index_columns(connection, descriptor)?;
    if actual
        .as_deref()
        .is_some_and(|columns| query_index_columns_match(columns, descriptor.columns))
    {
        return Ok(());
    }
    Err(StorageError::Invariant(format!(
        "query index '{}' did not persist its expected columns",
        descriptor.name
    )))
}

fn require_query_index_columns(
    descriptor: &SearchQueryIndexDescriptor,
    actual: Option<Vec<String>>,
) -> Result<(), StorageError> {
    if actual
        .as_deref()
        .is_some_and(|columns| query_index_columns_match(columns, descriptor.columns))
    {
        return Ok(());
    }
    Err(StorageError::Invariant(format!(
        "query index '{}' does not match its durable descriptor",
        descriptor.name
    )))
}

fn remaining_query_indexes_are_complete(
    connection: &Connection,
    descriptors: &[SearchQueryIndexDescriptor],
) -> Result<bool, StorageError> {
    for descriptor in descriptors {
        let persisted = persisted_query_index_columns(connection, descriptor)?;
        if descriptor.mode == SearchQueryIndexMode::Retired
            || !query_index_descriptor_is_applicable(connection, descriptor)?
        {
            if let Some(columns) = persisted {
                require_query_index_columns(descriptor, Some(columns))?;
            }
            continue;
        }
        match persisted {
            None => return Ok(false),
            Some(columns) if query_index_columns_match(&columns, descriptor.columns) => {}
            Some(columns) => {
                return Err(StorageError::Invariant(format!(
                    "query index '{}' has columns {:?}, expected {:?}",
                    descriptor.name, columns, descriptor.columns
                )));
            }
        }
    }
    Ok(true)
}

fn persisted_query_index_columns(
    connection: &Connection,
    descriptor: &SearchQueryIndexDescriptor,
) -> Result<Option<Vec<String>>, StorageError> {
    let table = connection
        .query_row(
            "SELECT tbl_name FROM sqlite_schema WHERE type = 'index' AND name = ?1",
            params![descriptor.name],
            |row| row.get::<_, String>(0),
        )
        .optional()?;
    let Some(table) = table else {
        return Ok(None);
    };
    if table != descriptor.table {
        return Err(StorageError::Invariant(format!(
            "query index '{}' belongs to table '{table}', expected '{}'",
            descriptor.name, descriptor.table
        )));
    }
    let mut statement = connection.prepare(&format!("PRAGMA index_list({table})"))?;
    let rows = statement.query_map([], |row| {
        Ok((
            row.get::<_, String>(1)?,
            row.get::<_, bool>(2)?,
            row.get::<_, bool>(4)?,
        ))
    })?;
    let persisted = rows.collect::<Result<Vec<_>, _>>()?;
    if !persisted
        .iter()
        .any(|(name, unique, partial)| name == descriptor.name && !unique && !partial)
    {
        return Err(StorageError::Invariant(format!(
            "query index '{}' is unique, partial, or absent from its table metadata",
            descriptor.name
        )));
    }
    let mut statement = connection.prepare(&format!("PRAGMA index_xinfo({})", descriptor.name))?;
    let rows = statement.query_map([], |row| {
        Ok((
            row.get::<_, Option<String>>(2)?,
            row.get::<_, bool>(3)?,
            row.get::<_, String>(4)?,
            row.get::<_, bool>(5)?,
        ))
    })?;
    let mut columns = Vec::new();
    for row in rows {
        let (column, descending, collation, key) = row?;
        if !key {
            continue;
        }
        let column = column.ok_or_else(|| {
            StorageError::Invariant(format!(
                "query index '{}' contains an expression column",
                descriptor.name
            ))
        })?;
        if descending || collation != "BINARY" {
            return Err(StorageError::Invariant(format!(
                "query index '{}' uses descending order or non-BINARY collation",
                descriptor.name
            )));
        }
        columns.push(column);
    }
    Ok(Some(columns))
}

/// Validates only descriptors already present in SQLite schema metadata.
///
/// Startup uses this read-only preflight so opening a database cannot turn a
/// missing descriptor into uncheckpointed index-construction work.
pub(in crate::storage::sqlite) fn validate_existing_query_indexes(
    connection: &Connection,
) -> Result<(), StorageError> {
    for descriptor in SEARCH_QUERY_INDEXES {
        if let Some(columns) = persisted_query_index_columns(connection, descriptor)? {
            require_query_index_columns(descriptor, Some(columns))?;
        }
    }

    Ok(())
}

/// Creates missing descriptors only when their complete owner table is empty.
///
/// The caller owns the transaction so session restart and direct-publication
/// preflight can roll these DDL changes back with their surrounding policy.
pub(in crate::storage::sqlite::code) fn prepare_query_indexes_for_empty_owners(
    connection: &Connection,
) -> Result<(), StorageError> {
    validate_existing_query_indexes(connection)?;
    for descriptor in SEARCH_QUERY_INDEXES {
        if descriptor.mode == SearchQueryIndexMode::Retired
            || !query_index_descriptor_is_applicable(connection, descriptor)?
            || persisted_query_index_columns(connection, descriptor)?.is_some()
            || !query_index_owner_is_empty(connection, descriptor)?
        {
            continue;
        }
        connection.execute(descriptor.sql, [])?;
        require_persisted_query_index(connection, descriptor)?;
    }

    Ok(())
}

/// Prepares only the two chunk lookups that would otherwise scan content-heavy
/// rows during finalization, and only while their shared owner is still empty.
pub(in crate::storage::sqlite::code) fn prepare_restart_query_indexes(
    connection: &Connection,
) -> Result<(), StorageError> {
    validate_existing_query_indexes(connection)?;
    for descriptor in SEARCH_QUERY_INDEXES {
        if descriptor.mode != SearchQueryIndexMode::RequiredForEmptyChunkOwnerRestart
            || !query_index_descriptor_is_applicable(connection, descriptor)?
            || persisted_query_index_columns(connection, descriptor)?.is_some()
            || !query_index_owner_is_empty(connection, descriptor)?
        {
            continue;
        }
        connection.execute(descriptor.sql, [])?;
        require_persisted_query_index(connection, descriptor)?;
    }

    Ok(())
}

/// Fails closed unless every applicable descriptor already has its exact shape.
pub(super) fn require_query_indexes_for_fact_publication(
    connection: &Connection,
) -> Result<(), StorageError> {
    if query_indexes_ready_for_fact_publication(connection)? {
        return Ok(());
    }
    Err(StorageError::Invariant(
        "one or more query indexes are missing before direct fact publication".to_owned(),
    ))
}

pub(in crate::storage::sqlite::code) fn query_indexes_ready_for_fact_publication(
    connection: &Connection,
) -> Result<bool, StorageError> {
    for descriptor in SEARCH_QUERY_INDEXES {
        let persisted = persisted_query_index_columns(connection, descriptor)?;
        if descriptor.mode == SearchQueryIndexMode::Retired
            || !query_index_descriptor_is_applicable(connection, descriptor)?
        {
            if let Some(columns) = persisted {
                require_query_index_columns(descriptor, Some(columns))?;
            }
            continue;
        }
        match persisted {
            Some(columns) => require_query_index_columns(descriptor, Some(columns))?,
            None => return Ok(false),
        }
    }
    Ok(true)
}

fn query_index_owner_is_empty(
    connection: &Connection,
    descriptor: &SearchQueryIndexDescriptor,
) -> Result<bool, StorageError> {
    connection
        .query_row(
            &format!(
                "SELECT NOT EXISTS (SELECT 1 FROM {} LIMIT 1)",
                descriptor.table
            ),
            [],
            |row| row.get::<_, bool>(0),
        )
        .map_err(StorageError::from)
}
