use std::path::Path;

use rusqlite::{Connection, OptionalExtension, params};

use crate::{
    domain::{code_snapshot_scope_is_fact_versioned, code_snapshot_scope_matches_identity},
    storage::StorageError,
};

use super::{
    import_compat,
    scope_tables::{
        CODE_SCOPE_TABLES, IMPORTED_DERIVED_SCOPE_TABLES, REFERENCE_SEARCH_SCOPE_TABLES,
    },
    snapshot_import::{IMPORT_SCHEMA, attached_code_table_exists, copy_attached_code_table},
};

pub(in crate::storage::sqlite::code) fn import_repository_from_database(
    connection: &mut Connection,
    source_path: &Path,
    repository_id: &str,
    source_scope: Option<&str>,
) -> Result<(), StorageError> {
    connection.execute(
        &format!("ATTACH DATABASE ?1 AS {IMPORT_SCHEMA}"),
        params![source_path.display().to_string()],
    )?;
    let result = import_attached_repository(connection, repository_id, source_scope);
    let detach = connection.execute(&format!("DETACH DATABASE {IMPORT_SCHEMA}"), []);
    match (result, detach) {
        (Ok(()), Ok(_)) => Ok(()),
        (Err(error), _) => Err(error),
        (Ok(()), Err(error)) => Err(StorageError::from(error)),
    }
}

fn import_attached_repository(
    connection: &mut Connection,
    repository_id: &str,
    source_scope: Option<&str>,
) -> Result<(), StorageError> {
    let transaction = connection.transaction()?;
    if source_scope.is_some() {
        super::super::schema::prepare_query_indexes_for_empty_owners(&transaction)?;
        super::super::schema::require_code_query_indexes_for_fact_publication(&transaction)?;
    }
    import_repository_metadata(&transaction, repository_id)?;
    if let Some(source_scope) = source_scope {
        import_code_scope(&transaction, repository_id, source_scope)?;
    }
    transaction.commit()?;

    Ok(())
}

fn import_repository_metadata(
    transaction: &rusqlite::Transaction<'_>,
    repository_id: &str,
) -> Result<(), StorageError> {
    let main_has_repository = transaction
        .query_row(
            "SELECT 1 FROM code_repositories WHERE repository_id = ?1",
            params![repository_id],
            |_| Ok(()),
        )
        .optional()?
        .is_some();
    let copied = transaction.execute(
        &format!(
            "
            INSERT OR IGNORE INTO code_repositories (
                repository_id, alias, root_path, path_filters_json, language_filters_json,
                last_indexed_scope_id, last_indexed_commit, tree_hash, state,
                indexed_file_count, symbol_count, reference_count, chunk_count,
                stale, degraded_reason
            )
            SELECT repository_id, alias, root_path, path_filters_json, language_filters_json,
                   last_indexed_scope_id, last_indexed_commit, tree_hash, state,
                   indexed_file_count, symbol_count, reference_count, chunk_count,
                   stale, degraded_reason
            FROM {IMPORT_SCHEMA}.code_repositories
            WHERE repository_id = ?1
            "
        ),
        params![repository_id],
    )?;
    if !main_has_repository && copied == 0 {
        return Err(StorageError::InvalidInput(format!(
            "code repository '{repository_id}' is missing from the import database"
        )));
    }
    transaction.execute(
        &format!(
            "
            INSERT OR IGNORE INTO code_repository_aliases (alias, repository_id)
            SELECT alias, repository_id
            FROM {IMPORT_SCHEMA}.code_repositories
            WHERE repository_id = ?1
            LIMIT 1
            "
        ),
        params![repository_id],
    )?;

    Ok(())
}

fn import_code_scope(
    transaction: &rusqlite::Transaction<'_>,
    repository_id: &str,
    source_scope: &str,
) -> Result<(), StorageError> {
    if transaction
        .query_row(
            "SELECT 1 FROM code_repository_scopes WHERE source_scope = ?1",
            params![source_scope],
            |_| Ok(()),
        )
        .optional()?
        .is_some()
    {
        import_commit_scope_aliases(transaction, repository_id, source_scope)?;
        return Ok(());
    }
    let imported_generated_detection_is_current =
        import_compat::attached_generated_detection_is_current(
            transaction,
            super::super::schema::GENERATED_DETECTION_REINDEX_MIGRATION,
        )?;
    let imported_search_owner_is_current = import_compat::attached_search_owner_is_current(
        transaction,
        crate::storage::sqlite::schema::marker::SEARCH_OWNER_V2_MIGRATION,
        crate::storage::sqlite::schema::marker::REFERENCE_SEARCH_GROUP_V2_MIGRATION,
    )?
        && import_compat::attached_grouped_reference_scope_is_current(transaction, source_scope)?;

    super::super::cleanup::delete_scope_index(transaction, source_scope)?;
    let copied = transaction.execute(
        &format!(
            "
            INSERT INTO code_repository_scopes (
                source_scope, repository_id, resolved_commit_sha, tree_hash,
                path_filters_json, language_filters_json, indexed_file_count,
                symbol_count, reference_count, chunk_count, stale, degraded_reason
            )
            SELECT source_scope, repository_id, resolved_commit_sha, tree_hash,
                   path_filters_json, language_filters_json, indexed_file_count,
                   symbol_count, reference_count, chunk_count, stale, degraded_reason
            FROM {IMPORT_SCHEMA}.code_repository_scopes
            WHERE source_scope = ?1 AND repository_id = ?2
            "
        ),
        params![source_scope, repository_id],
    )?;
    if copied == 0 {
        return Err(StorageError::InvalidInput(format!(
            "code repository '{repository_id}' has no importable source scope '{source_scope}'"
        )));
    }
    let imported_fact_version_is_current =
        imported_scope_matches_current_fact_version(transaction, repository_id, source_scope)?;
    import_commit_scope_aliases(transaction, repository_id, source_scope)?;
    for table in CODE_SCOPE_TABLES {
        copy_attached_code_table(transaction, table, source_scope)?;
    }
    if imported_search_owner_is_current {
        for table in REFERENCE_SEARCH_SCOPE_TABLES {
            copy_attached_code_table(transaction, table, source_scope)?;
        }
        super::search_copy::import_exact_search_documents(transaction, source_scope)?;
    }
    for table in IMPORTED_DERIVED_SCOPE_TABLES {
        copy_attached_code_table(transaction, table, source_scope)?;
    }
    if imported_search_owner_is_current {
        require_grouped_reference_projection(transaction, source_scope)?;
    }
    super::super::generated::backfill_scope_path_generated_flags(transaction, source_scope)?;
    if !imported_generated_detection_is_current {
        super::super::generated::mark_scope_generated_detection_stale(transaction, source_scope)?;
    }
    if !imported_search_owner_is_current {
        mark_imported_search_owner_scope_stale(transaction, source_scope)?;
    }
    if !imported_fact_version_is_current {
        mark_imported_fact_version_scope_stale(transaction, source_scope)?;
    }

    Ok(())
}

pub(super) fn require_grouped_reference_projection(
    transaction: &rusqlite::Connection,
    source_scope: &str,
) -> Result<(), StorageError> {
    let valid = transaction.query_row(
        "SELECT EXISTS (
             SELECT 1
             FROM code_repository_reference_search_manifests manifest
             WHERE manifest.source_scope = ?1
               AND manifest.projection_version = 2
               AND manifest.reference_count = (
                   SELECT COUNT(*) FROM code_repository_references
                   WHERE source_scope = ?1
               )
               AND manifest.reference_count = (
                   SELECT reference_count FROM code_repository_scopes
                   WHERE source_scope = ?1
               )
               AND manifest.group_count = (
                   SELECT COUNT(*) FROM code_repository_reference_search_groups
                   WHERE source_scope = ?1
               )
               AND manifest.reference_count = (
                   SELECT coalesce(sum(occurrence_count), 0)
                   FROM code_repository_reference_search_groups
                   WHERE source_scope = ?1
               )
               AND manifest.group_count = (
                   SELECT COUNT(*) FROM code_repository_search_metadata
                   WHERE source_scope = ?1 AND document_kind = 'reference'
               )
               AND manifest.group_count = (
                   SELECT COUNT(*)
                   FROM code_repository_search_metadata metadata
                   CROSS JOIN code_repository_search search_row
                     ON search_row.rowid = metadata.search_rowid
                    AND search_row.source_scope = metadata.source_scope
                    AND search_row.document_kind = metadata.document_kind
                    AND search_row.record_id = metadata.record_id
                    AND search_row.path = metadata.path
                   JOIN code_repository_reference_search_groups search_group
                     ON search_group.source_scope = metadata.source_scope
                    AND search_group.group_id = metadata.record_id
                    AND search_group.path = metadata.path
                    AND search_group.language_id = search_row.language_id
                   WHERE metadata.source_scope = ?1
                     AND metadata.document_kind = 'reference'
               )
               AND NOT EXISTS (
                   SELECT 1
                   FROM code_repository_reference_search_groups search_group
                   WHERE search_group.source_scope = ?1
                     AND (
                         search_group.occurrence_count != (
                             SELECT COUNT(*)
                             FROM code_repository_references reference
                             WHERE reference.source_scope = search_group.source_scope
                               AND reference.name = search_group.name
                               AND reference.kind = search_group.kind
                               AND reference.path = search_group.path
                               AND coalesce(reference.target_hint, '') = search_group.target_hint
                         )
                         OR search_group.group_id != (
                             SELECT MIN(reference.reference_id)
                             FROM code_repository_references reference
                             WHERE reference.source_scope = search_group.source_scope
                               AND reference.name = search_group.name
                               AND reference.kind = search_group.kind
                               AND reference.path = search_group.path
                               AND coalesce(reference.target_hint, '') = search_group.target_hint
                         )
                         OR search_group.language_id != coalesce((
                             SELECT file.language_id
                             FROM code_repository_files file
                             WHERE file.source_scope = search_group.source_scope
                               AND file.path = search_group.path
                         ), '')
                     )
               )
         )",
        params![source_scope],
        |row| row.get::<_, bool>(0),
    )?;
    if !valid {
        return Err(StorageError::Invariant(format!(
            "imported scope '{source_scope}' did not preserve its exact grouped reference-search projection"
        )));
    }
    let mut statement = transaction.prepare(
        "SELECT search_group.name, search_group.kind, search_group.target_hint,
                search_group.path, search_row.content
         FROM code_repository_reference_search_groups search_group
         JOIN code_repository_search_metadata metadata
           ON metadata.source_scope = search_group.source_scope
          AND metadata.document_kind = 'reference'
          AND metadata.record_id = search_group.group_id
          AND metadata.path = search_group.path
         CROSS JOIN code_repository_search search_row
           ON search_row.rowid = metadata.search_rowid
          AND search_row.source_scope = metadata.source_scope
          AND search_row.document_kind = metadata.document_kind
          AND search_row.record_id = metadata.record_id
          AND search_row.path = metadata.path
         WHERE search_group.source_scope = ?1",
    )?;
    let rows = statement.query_map(params![source_scope], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, String>(3)?,
            row.get::<_, String>(4)?,
        ))
    })?;
    for row in rows {
        let (name, kind, target_hint, path, content) = row?;
        let expected = super::super::search::search_document_content(
            "reference",
            [
                name.as_str(),
                kind.as_str(),
                target_hint.as_str(),
                path.as_str(),
            ],
        );
        if content != expected {
            return Err(StorageError::Invariant(format!(
                "scope '{source_scope}' has non-canonical grouped reference-search content"
            )));
        }
    }
    Ok(())
}

fn imported_scope_matches_current_fact_version(
    transaction: &rusqlite::Transaction<'_>,
    repository_id: &str,
    source_scope: &str,
) -> Result<bool, StorageError> {
    if !code_snapshot_scope_is_fact_versioned(source_scope) {
        return Ok(true);
    }
    let (tree_hash, path_filters, language_filters) = transaction.query_row(
        "
        SELECT tree_hash, path_filters_json, language_filters_json
        FROM code_repository_scopes
        WHERE repository_id = ?1 AND source_scope = ?2
        ",
        params![repository_id, source_scope],
        |row| {
            Ok((
                row.get::<_, String>(0)?,
                super::super::status::parse_json_list(row.get::<_, String>(1)?)?,
                super::super::status::parse_json_list(row.get::<_, String>(2)?)?,
            ))
        },
    )?;
    let path_filters = super::super::status::canonical_path_filters(&path_filters);
    let language_filters = super::super::status::canonical_filter_values(&language_filters);

    Ok(code_snapshot_scope_matches_identity(
        repository_id,
        &tree_hash,
        &path_filters,
        &language_filters,
        source_scope,
    ))
}

fn mark_imported_search_owner_scope_stale(
    transaction: &rusqlite::Transaction<'_>,
    source_scope: &str,
) -> Result<(), StorageError> {
    const REASON: &str =
        "import source lacks the complete search-owner-v2 capability; full reindex required";
    mark_imported_scope_stale(transaction, source_scope, REASON)?;
    tracing::warn!(
        source_scope,
        reason = REASON,
        "imported code scope is retained as stale"
    );

    Ok(())
}

fn mark_imported_fact_version_scope_stale(
    transaction: &rusqlite::Transaction<'_>,
    source_scope: &str,
) -> Result<(), StorageError> {
    const REASON: &str =
        "import source scope uses an older code fact version; full reindex required";
    mark_imported_scope_stale(transaction, source_scope, REASON)?;
    tracing::warn!(
        source_scope,
        reason = REASON,
        "imported code scope is retained as stale"
    );

    Ok(())
}

fn mark_imported_scope_stale(
    transaction: &rusqlite::Transaction<'_>,
    source_scope: &str,
    reason: &str,
) -> Result<(), StorageError> {
    transaction.execute(
        "
        UPDATE code_repository_scopes
        SET stale = 1,
            degraded_reason = CASE
                WHEN degraded_reason IS NULL OR degraded_reason = '' THEN ?2
                WHEN instr(degraded_reason, ?2) > 0 THEN degraded_reason
                ELSE degraded_reason || '; ' || ?2
            END
        WHERE source_scope = ?1
        ",
        params![source_scope, reason],
    )?;
    transaction.execute(
        "
        UPDATE code_repositories
        SET stale = 1,
            degraded_reason = CASE
                WHEN degraded_reason IS NULL OR degraded_reason = '' THEN ?2
                WHEN instr(degraded_reason, ?2) > 0 THEN degraded_reason
                ELSE degraded_reason || '; ' || ?2
            END
        WHERE last_indexed_scope_id = ?1
        ",
        params![source_scope, reason],
    )?;

    Ok(())
}

fn import_commit_scope_aliases(
    transaction: &rusqlite::Transaction<'_>,
    repository_id: &str,
    source_scope: &str,
) -> Result<(), StorageError> {
    if attached_code_table_exists(transaction, "code_repository_commit_scopes")? {
        transaction.execute(
            &format!(
                "
                INSERT INTO code_repository_commit_scopes (
                    repository_id, resolved_commit_sha, source_scope, published_sequence
                )
                SELECT repository_id, resolved_commit_sha, source_scope, published_sequence
                FROM {IMPORT_SCHEMA}.code_repository_commit_scopes
                WHERE repository_id = ?1 AND source_scope = ?2
                ON CONFLICT(repository_id, resolved_commit_sha, source_scope) DO UPDATE SET
                    published_sequence = max(
                        code_repository_commit_scopes.published_sequence,
                        excluded.published_sequence
                    )
                "
            ),
            params![repository_id, source_scope],
        )?;
    }
    let resolved_commit_sha = transaction.query_row(
        "
        SELECT resolved_commit_sha
        FROM code_repository_scopes
        WHERE repository_id = ?1 AND source_scope = ?2
        ",
        params![repository_id, source_scope],
        |row| row.get::<_, String>(0),
    )?;
    super::super::lifecycle::commit_scope::record(
        transaction,
        repository_id,
        &resolved_commit_sha,
        source_scope,
    )
}

#[cfg(test)]
#[path = "import_tests.rs"]
mod tests;
