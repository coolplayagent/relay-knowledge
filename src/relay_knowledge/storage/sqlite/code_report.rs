use rusqlite::{Connection, params};

use crate::{
    domain::{
        CodeParseStatus, CodeParseStatusCounts, CodeRepositoryLatencySample, CodeRepositoryReport,
        CodeRepositoryTotals,
    },
    storage::StorageError,
};

use super::code_status;

pub(super) fn repository_totals(
    connection: &mut Connection,
) -> Result<CodeRepositoryTotals, StorageError> {
    Ok(CodeRepositoryTotals {
        repository_count: count_all_rows(connection, "code_repositories")?,
        indexed_file_count: count_all_rows(connection, "code_repository_files")?,
        symbol_count: count_all_rows(connection, "code_repository_symbols")?,
        reference_count: count_all_rows(connection, "code_repository_references")?,
        chunk_count: count_all_rows(connection, "code_repository_chunks")?,
        degraded_file_count: count_all_rows(connection, "code_repository_file_diagnostics")?,
        parse_status_counts: repository_parse_status_counts(connection)?,
    })
}

pub(super) fn repository_report(
    connection: &mut Connection,
    repository: &str,
) -> Result<CodeRepositoryReport, StorageError> {
    let status = code_status::repository_status(connection, repository)?.ok_or_else(|| {
        StorageError::InvalidInput(format!("code repository '{repository}' is not registered"))
    })?;
    let scope = status.last_indexed_scope_id.as_deref().unwrap_or_default();
    let degradation_summary = repository_diagnostics(connection, scope)?;
    let degraded_file_count = repository_degraded_file_count(connection, scope)?;
    let edge_counts = repository_edge_resolution_counts(connection, scope)?;
    let representative_queries = representative_queries(connection, scope)?;
    let freshness_state = if status.stale {
        "stale"
    } else {
        status.state.as_str()
    }
    .to_owned();

    Ok(CodeRepositoryReport {
        repository_id: status.repository_id,
        alias: status.alias,
        root_path: status.root_path,
        path_filters: status.path_filters,
        language_filters: status.language_filters,
        resolved_commit_sha: status.last_indexed_commit,
        tree_hash: status.tree_hash,
        indexed_file_count: status.indexed_file_count,
        symbol_count: status.symbol_count,
        reference_count: status.reference_count,
        chunk_count: status.chunk_count,
        degraded_file_count,
        resolved_edge_count: edge_counts.resolved,
        ambiguous_edge_count: edge_counts.ambiguous,
        unresolved_edge_count: edge_counts.unresolved,
        degradation_summary,
        representative_queries,
        latency_samples: Vec::<CodeRepositoryLatencySample>::new(),
        freshness_state,
    })
}

fn repository_parse_status_counts(
    connection: &Connection,
) -> Result<CodeParseStatusCounts, StorageError> {
    let mut statement = connection.prepare(
        "
        SELECT parse_status, COUNT(*)
        FROM code_repository_files
        GROUP BY parse_status
        ",
    )?;
    let rows = statement.query_map([], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, usize>(1)?))
    })?;
    let mut counts = CodeParseStatusCounts::default();
    for row in rows {
        let (status, count) = row?;
        match status.as_str() {
            value if value == CodeParseStatus::Parsed.as_str() => counts.parsed = count,
            value if value == CodeParseStatus::Partial.as_str() => counts.partial = count,
            value if value == CodeParseStatus::TextOnly.as_str() => counts.text_only = count,
            value if value == CodeParseStatus::Failed.as_str() => counts.failed = count,
            other => {
                return Err(StorageError::InvalidInput(format!(
                    "unknown code repository parse status '{other}'"
                )));
            }
        }
    }

    Ok(counts)
}

fn repository_degraded_file_count(
    connection: &Connection,
    source_scope: &str,
) -> Result<usize, StorageError> {
    connection
        .query_row(
            "
            SELECT COUNT(*)
            FROM code_repository_file_diagnostics
            WHERE source_scope = ?1
            ",
            params![source_scope],
            |row| row.get::<_, usize>(0),
        )
        .map_err(StorageError::from)
}

#[derive(Debug, Default)]
struct EdgeResolutionCounts {
    resolved: usize,
    ambiguous: usize,
    unresolved: usize,
}

fn repository_edge_resolution_counts(
    connection: &Connection,
    source_scope: &str,
) -> Result<EdgeResolutionCounts, StorageError> {
    let mut counts = EdgeResolutionCounts::default();
    for (table, column) in [
        ("code_repository_references", "resolution_state"),
        ("code_repository_imports", "resolution_state"),
    ] {
        let mut statement = connection.prepare(&format!(
            "
            SELECT {column}, COUNT(*)
            FROM {table}
            WHERE source_scope = ?1
            GROUP BY {column}
            "
        ))?;
        let rows = statement.query_map(params![source_scope], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, usize>(1)?))
        })?;
        for row in rows {
            let (state, count) = row?;
            match state.as_str() {
                "resolved" => counts.resolved += count,
                "ambiguous" => counts.ambiguous += count,
                _ => counts.unresolved += count,
            }
        }
    }

    Ok(counts)
}

fn repository_diagnostics(
    connection: &Connection,
    source_scope: &str,
) -> Result<Vec<String>, StorageError> {
    let mut statement = connection.prepare(
        "
        SELECT path, message
        FROM code_repository_file_diagnostics
        WHERE source_scope = ?1
        ORDER BY path ASC, message ASC
        LIMIT 20
        ",
    )?;
    let rows = statement.query_map(params![source_scope], |row| {
        Ok(format!(
            "{}: {}",
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?
        ))
    })?;

    rows.collect::<Result<Vec<_>, _>>()
        .map_err(StorageError::from)
}

fn representative_queries(
    connection: &Connection,
    source_scope: &str,
) -> Result<Vec<String>, StorageError> {
    let mut queries = Vec::new();
    let mut statement = connection.prepare(
        "
        SELECT name
        FROM code_repository_symbols
        WHERE source_scope = ?1
        ORDER BY path ASC, line_start ASC
        LIMIT 3
        ",
    )?;
    let rows = statement.query_map(params![source_scope], |row| row.get::<_, String>(0))?;
    queries.extend(
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(StorageError::from)?,
    );
    if queries.is_empty() {
        queries.push("hybrid".to_owned());
    }
    queries.sort();
    queries.dedup();

    Ok(queries)
}

fn count_all_rows(connection: &Connection, table: &'static str) -> Result<usize, StorageError> {
    connection
        .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
            row.get(0)
        })
        .map_err(StorageError::from)
}
