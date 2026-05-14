use rusqlite::{Connection, OptionalExtension, params};

use crate::{
    domain::{CodeRepositoryRegistration, CodeRepositoryStatus},
    storage::StorageError,
};

pub(super) fn upsert_repository(
    connection: &mut Connection,
    registration: CodeRepositoryRegistration,
) -> Result<CodeRepositoryStatus, StorageError> {
    reject_alias_collision(connection, &registration)?;
    connection.execute(
        "
        INSERT INTO code_repositories (
            repository_id, alias, root_path, path_filters_json, language_filters_json,
            state, indexed_file_count, symbol_count, reference_count, chunk_count,
            stale, degraded_reason
        )
        VALUES (?1, ?2, ?3, ?4, ?5, 'registered', 0, 0, 0, 0, 1, NULL)
        ON CONFLICT(repository_id) DO UPDATE SET
            root_path = excluded.root_path,
            path_filters_json = excluded.path_filters_json,
            language_filters_json = excluded.language_filters_json,
            stale = 1
        ",
        params![
            registration.repository_id,
            registration.alias,
            registration.root_path,
            serde_json::to_string(&registration.path_filters)
                .map_err(|error| StorageError::InvalidInput(error.to_string()))?,
            serde_json::to_string(&registration.language_filters)
                .map_err(|error| StorageError::InvalidInput(error.to_string()))?,
        ],
    )?;
    connection.execute(
        "
        INSERT OR IGNORE INTO code_repository_aliases (alias, repository_id)
        VALUES (?1, ?2)
        ",
        params![registration.alias, registration.repository_id],
    )?;

    repository_status(connection, &registration.alias)?.ok_or_else(|| {
        StorageError::InvalidInput("registered code repository was not persisted".to_owned())
    })
}

fn reject_alias_collision(
    connection: &Connection,
    registration: &CodeRepositoryRegistration,
) -> Result<(), StorageError> {
    let existing = connection
        .query_row(
            "
            SELECT repository_id
            FROM code_repository_aliases
            WHERE alias = ?1
            ",
            params![registration.alias],
            |row| row.get::<_, String>(0),
        )
        .optional()?;
    if existing.is_some_and(|repository_id| repository_id != registration.repository_id) {
        return Err(StorageError::InvalidInput(format!(
            "code repository alias '{}' is already registered for a different repository",
            registration.alias
        )));
    }

    Ok(())
}

pub(super) fn repository_status(
    connection: &mut Connection,
    repository: &str,
) -> Result<Option<CodeRepositoryStatus>, StorageError> {
    if let Some(status) =
        repository_status_by_column(connection, repository, RepositoryLookupColumn::RepositoryId)?
    {
        return Ok(Some(status));
    }
    repository_status_by_column(connection, repository, RepositoryLookupColumn::Alias)
}

pub(super) fn repository_scope_status(
    connection: &mut Connection,
    repository: &str,
    resolved_commit_sha: &str,
    path_filters: &[String],
    language_filters: &[String],
) -> Result<Option<CodeRepositoryStatus>, StorageError> {
    let base = repository_status(connection, repository)?;
    let Some(base) = base else {
        return Ok(None);
    };
    let path_filters_json = serde_json::to_string(path_filters)
        .map_err(|error| StorageError::InvalidInput(error.to_string()))?;
    let language_filters_json = serde_json::to_string(language_filters)
        .map_err(|error| StorageError::InvalidInput(error.to_string()))?;
    let requested_path_filters = canonical_path_filters(path_filters);
    let requested_language_filters = canonical_filter_values(language_filters);
    let mut statement = connection.prepare(
        "
        SELECT source_scope, tree_hash, indexed_file_count, symbol_count,
               reference_count, chunk_count, stale, degraded_reason,
               path_filters_json, language_filters_json
        FROM code_repository_scopes
        WHERE repository_id = ?1
          AND resolved_commit_sha = ?2
        ORDER BY
          CASE WHEN path_filters_json = ?3 AND language_filters_json = ?4 THEN 0 ELSE 1 END,
          source_scope ASC
        ",
    )?;
    let rows = statement.query_map(
        params![
            base.repository_id,
            resolved_commit_sha,
            path_filters_json,
            language_filters_json
        ],
        |row| {
            let stored_path_filters = parse_json_list(row.get::<_, String>(8)?)?;
            let stored_language_filters = parse_json_list(row.get::<_, String>(9)?)?;
            Ok((
                CodeRepositoryStatus {
                    repository_id: base.repository_id.clone(),
                    alias: base.alias.clone(),
                    root_path: base.root_path.clone(),
                    path_filters: stored_path_filters.clone(),
                    language_filters: stored_language_filters.clone(),
                    last_indexed_scope_id: Some(row.get(0)?),
                    last_indexed_commit: Some(resolved_commit_sha.to_owned()),
                    tree_hash: Some(row.get(1)?),
                    state: "fresh".to_owned(),
                    indexed_file_count: row.get(2)?,
                    symbol_count: row.get(3)?,
                    reference_count: row.get(4)?,
                    chunk_count: row.get(5)?,
                    stale: row.get::<_, i64>(6)? != 0,
                    degraded_reason: row.get(7)?,
                },
                stored_path_filters,
                stored_language_filters,
            ))
        },
    )?;
    for row in rows {
        let (status, stored_path_filters, stored_language_filters) = row?;
        if canonical_path_filters(&stored_path_filters) == requested_path_filters
            && canonical_filter_values(&stored_language_filters) == requested_language_filters
        {
            return Ok(Some(status));
        }
    }

    Ok(None)
}

fn repository_status_by_column(
    connection: &mut Connection,
    repository: &str,
    column: RepositoryLookupColumn,
) -> Result<Option<CodeRepositoryStatus>, StorageError> {
    connection
        .query_row(column.query(), params![repository], |row| {
            Ok(CodeRepositoryStatus {
                repository_id: row.get(0)?,
                alias: row.get(1)?,
                root_path: row.get(2)?,
                path_filters: parse_json_list(row.get::<_, String>(3)?)?,
                language_filters: parse_json_list(row.get::<_, String>(4)?)?,
                last_indexed_scope_id: row.get(5)?,
                last_indexed_commit: row.get(6)?,
                tree_hash: row.get(7)?,
                state: row.get(8)?,
                indexed_file_count: row.get(9)?,
                symbol_count: row.get(10)?,
                reference_count: row.get(11)?,
                chunk_count: row.get(12)?,
                stale: row.get::<_, i64>(13)? != 0,
                degraded_reason: row.get(14)?,
            })
        })
        .optional()
        .map_err(StorageError::from)
}

enum RepositoryLookupColumn {
    RepositoryId,
    Alias,
}

impl RepositoryLookupColumn {
    fn query(&self) -> &'static str {
        match self {
            Self::RepositoryId => {
                "
                SELECT repository_id, alias, root_path, path_filters_json, language_filters_json,
                       last_indexed_scope_id, last_indexed_commit, tree_hash,
                       state, indexed_file_count, symbol_count, reference_count, chunk_count,
                       stale, degraded_reason
                FROM code_repositories
                WHERE repository_id = ?1
                "
            }
            Self::Alias => {
                "
                SELECT r.repository_id, a.alias, r.root_path, r.path_filters_json, r.language_filters_json,
                       r.last_indexed_scope_id, r.last_indexed_commit, r.tree_hash,
                       r.state, r.indexed_file_count, r.symbol_count, r.reference_count, r.chunk_count,
                       r.stale, r.degraded_reason
                FROM code_repository_aliases a
                JOIN code_repositories r ON r.repository_id = a.repository_id
                WHERE a.alias = ?1
                "
            }
        }
    }
}

pub(super) fn parse_json_list(value: String) -> rusqlite::Result<Vec<String>> {
    serde_json::from_str(&value).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(error))
    })
}

pub(super) fn canonical_path_filters(filters: &[String]) -> Vec<String> {
    let mut normalized = Vec::new();
    for filter in filters {
        let value = normalize_path_filter(filter).to_owned();
        if !normalized.contains(&value) {
            normalized.push(value);
        }
    }

    normalized
}

pub(super) fn canonical_filter_values(filters: &[String]) -> Vec<String> {
    let mut normalized = Vec::new();
    for filter in filters {
        if !normalized.contains(filter) {
            normalized.push(filter.clone());
        }
    }

    normalized
}

fn normalize_path_filter(filter: &str) -> &str {
    let mut filter = filter.trim_end_matches(['/', '\\']);
    while let Some(stripped) = filter.strip_prefix("./") {
        filter = stripped;
    }

    filter
}
