//! Durable commit-to-content-scope aliases and their bounded audit window.

use std::collections::BTreeSet;

use rusqlite::{
    Connection, OptionalExtension, Transaction, params, params_from_iter, types::Value,
};

use crate::storage::StorageError;

pub(in crate::storage::sqlite::code) const RETAIN_COMMIT_SCOPE_ALIAS_ROWS: usize = 256;

#[cfg(test)]
#[path = "commit_scope_tests.rs"]
mod tests;

pub(in crate::storage) fn record(
    connection: &Connection,
    repository_id: &str,
    resolved_commit_sha: &str,
    source_scope: &str,
) -> Result<(), StorageError> {
    connection.execute(
        "
        INSERT INTO code_repository_commit_scopes (
            repository_id, resolved_commit_sha, source_scope, published_sequence
        )
        SELECT ?1, ?2, ?3,
               coalesce((
                   SELECT published_sequence
                   FROM code_repository_commit_scopes
                   WHERE repository_id = ?1
                   ORDER BY published_sequence DESC, resolved_commit_sha DESC, source_scope DESC
                   LIMIT 1
               ), 0) + 1
        ON CONFLICT(repository_id, resolved_commit_sha, source_scope) DO UPDATE SET
            published_sequence = excluded.published_sequence
        ",
        params![repository_id, resolved_commit_sha, source_scope],
    )?;
    Ok(())
}

/// Lazily preserves the legacy commit currently named by a content scope.
///
/// This runs in the publication transaction immediately before a same-scope
/// upsert, avoiding an unbounded all-scope migration during database open.
pub(in crate::storage) fn preserve_existing_scope_commit(
    connection: &Connection,
    repository_id: &str,
    source_scope: &str,
) -> Result<(), StorageError> {
    let existing = connection
        .query_row(
            "SELECT resolved_commit_sha
             FROM code_repository_scopes
             WHERE repository_id = ?1 AND source_scope = ?2",
            params![repository_id, source_scope],
            |row| row.get::<_, String>(0),
        )
        .optional()?;
    if let Some(commit) = existing {
        record(connection, repository_id, &commit, source_scope)?;
    }
    Ok(())
}

pub(in crate::storage::sqlite::code) fn prune_repository_aliases(
    transaction: &Transaction<'_>,
    repository_id: &str,
    protected_commits: &BTreeSet<String>,
) -> Result<(), StorageError> {
    let aliases = {
        let (query, values) = pruning_candidates_query(
            repository_id,
            protected_commits,
            super::super::tasks::retention_gc::GC_ROW_BATCH_SIZE,
        );
        let mut statement = transaction.prepare(&query)?;
        let rows = statement.query_map(params_from_iter(values), |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;
        rows.collect::<Result<Vec<_>, _>>()?
    };
    for (resolved_commit_sha, source_scope) in aliases {
        transaction.execute(
            "
            DELETE FROM code_repository_commit_scopes
            WHERE repository_id = ?1
              AND resolved_commit_sha = ?2
              AND source_scope = ?3
            ",
            params![repository_id, resolved_commit_sha, source_scope],
        )?;
    }
    Ok(())
}

fn pruning_candidates_query(
    repository_id: &str,
    protected_commits: &BTreeSet<String>,
    limit: usize,
) -> (String, Vec<Value>) {
    let mut values = vec![
        Value::Text(repository_id.to_owned()),
        Value::Integer(RETAIN_COMMIT_SCOPE_ALIAS_ROWS as i64),
    ];
    let protected_clause = if protected_commits.is_empty() {
        String::new()
    } else {
        values.extend(protected_commits.iter().cloned().map(Value::Text));
        let placeholders = std::iter::repeat_n("?", protected_commits.len())
            .collect::<Vec<_>>()
            .join(", ");
        format!("WHERE resolved_commit_sha NOT IN ({placeholders})")
    };
    values.push(Value::Integer(limit as i64));
    (
        format!(
            "SELECT resolved_commit_sha, source_scope FROM (
            SELECT resolved_commit_sha, source_scope
            FROM code_repository_commit_scopes
            WHERE repository_id = ?
            ORDER BY published_sequence DESC, resolved_commit_sha DESC, source_scope DESC
            LIMIT -1 OFFSET ?
         ) {protected_clause}
         LIMIT ?"
        ),
        values,
    )
}

pub(in crate::storage::sqlite::code) fn repository_alias_pruning_pending(
    connection: &Connection,
    repository_id: &str,
    protected_commits: &BTreeSet<String>,
) -> Result<bool, StorageError> {
    let (query, values) = pruning_candidates_query(repository_id, protected_commits, 1);
    connection
        .query_row(&query, params_from_iter(values), |_| Ok(()))
        .optional()
        .map(|row| row.is_some())
        .map_err(StorageError::from)
}
