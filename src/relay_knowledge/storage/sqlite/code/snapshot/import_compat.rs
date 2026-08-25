use rusqlite::{OptionalExtension, Transaction, params};

use crate::storage::StorageError;

use super::snapshot_import::IMPORT_SCHEMA;

pub(super) fn attached_generated_detection_is_current(
    transaction: &Transaction<'_>,
    migration: &str,
) -> Result<bool, StorageError> {
    Ok(
        attached_table_has_column(transaction, "code_repository_files", "is_generated")?
            && attached_code_schema_migration_applied(transaction, migration)?,
    )
}

pub(super) fn attached_search_owner_is_current(
    transaction: &Transaction<'_>,
    search_owner_migration: &str,
    reference_group_migration: &str,
) -> Result<bool, StorageError> {
    const SEARCH_COLUMNS: &[&str] = &[
        "source_scope",
        "document_kind",
        "record_id",
        "path",
        "language_id",
        "content",
    ];
    const SEARCH_METADATA_COLUMNS: &[&str] = &[
        "source_scope",
        "document_kind",
        "record_id",
        "path",
        "search_rowid",
    ];
    const REFERENCE_GROUP_COLUMNS: &[&str] = &[
        "source_scope",
        "group_id",
        "name",
        "kind",
        "path",
        "target_hint",
        "language_id",
        "occurrence_count",
    ];
    const REFERENCE_MANIFEST_COLUMNS: &[&str] = &[
        "source_scope",
        "projection_version",
        "reference_count",
        "group_count",
    ];
    if !attached_code_schema_migration_applied(transaction, search_owner_migration)?
        || !attached_code_schema_migration_applied(transaction, reference_group_migration)?
        || !attached_table_exists(transaction, "code_repository_search")?
        || !attached_table_exists(transaction, "code_repository_search_metadata")?
    {
        return Ok(false);
    }
    for (table, columns) in [
        ("code_repository_search", SEARCH_COLUMNS),
        ("code_repository_search_metadata", SEARCH_METADATA_COLUMNS),
        (
            "code_repository_reference_search_groups",
            REFERENCE_GROUP_COLUMNS,
        ),
        (
            "code_repository_reference_search_manifests",
            REFERENCE_MANIFEST_COLUMNS,
        ),
    ] {
        for column in columns {
            if !attached_table_has_column(transaction, table, column)? {
                return Ok(false);
            }
        }
    }

    Ok(true)
}

pub(super) fn attached_grouped_reference_scope_is_current(
    transaction: &Transaction<'_>,
    source_scope: &str,
) -> Result<bool, StorageError> {
    let manifest = transaction
        .query_row(
            &format!(
                "SELECT projection_version, reference_count, group_count
                 FROM {IMPORT_SCHEMA}.code_repository_reference_search_manifests
                 WHERE source_scope = ?1"
            ),
            params![source_scope],
            |row| {
                Ok((
                    row.get::<_, usize>(0)?,
                    row.get::<_, usize>(1)?,
                    row.get::<_, usize>(2)?,
                ))
            },
        )
        .optional()?;
    let Some((projection_version, reference_count, group_count)) = manifest else {
        return Ok(false);
    };
    if projection_version != 2 || group_count > reference_count {
        return Ok(false);
    }
    let scope_reference_count = transaction
        .query_row(
            &format!(
                "SELECT reference_count FROM {IMPORT_SCHEMA}.code_repository_scopes
                 WHERE source_scope = ?1"
            ),
            params![source_scope],
            |row| row.get::<_, usize>(0),
        )
        .optional()?;
    if scope_reference_count != Some(reference_count) {
        return Ok(false);
    }
    let actual_reference_count = transaction.query_row(
        &format!(
            "SELECT COUNT(*) FROM {IMPORT_SCHEMA}.code_repository_references
             WHERE source_scope = ?1"
        ),
        params![source_scope],
        |row| row.get::<_, usize>(0),
    )?;
    let (actual_group_count, occurrence_count) = transaction.query_row(
        &format!(
            "SELECT COUNT(*), coalesce(sum(occurrence_count), 0)
             FROM {IMPORT_SCHEMA}.code_repository_reference_search_groups
             WHERE source_scope = ?1"
        ),
        params![source_scope],
        |row| Ok((row.get::<_, usize>(0)?, row.get::<_, usize>(1)?)),
    )?;
    let (reference_metadata_count, exact_search_owner_count) = transaction.query_row(
        &format!(
            "SELECT
                 (SELECT COUNT(*)
                  FROM {IMPORT_SCHEMA}.code_repository_search_metadata
                  WHERE source_scope = ?1 AND document_kind = 'reference'),
                 (SELECT COUNT(*)
                  FROM {IMPORT_SCHEMA}.code_repository_search_metadata metadata
                  CROSS JOIN {IMPORT_SCHEMA}.code_repository_search search_row
                    ON search_row.rowid = metadata.search_rowid
                   AND search_row.source_scope = metadata.source_scope
                   AND search_row.document_kind = metadata.document_kind
                   AND search_row.record_id = metadata.record_id
                   AND search_row.path = metadata.path
                  JOIN {IMPORT_SCHEMA}.code_repository_reference_search_groups search_group
                    ON search_group.source_scope = metadata.source_scope
                   AND search_group.group_id = metadata.record_id
                   AND search_group.path = metadata.path
                   AND search_group.language_id = search_row.language_id
                  WHERE metadata.source_scope = ?1
                    AND metadata.document_kind = 'reference')"
        ),
        params![source_scope],
        |row| Ok((row.get::<_, usize>(0)?, row.get::<_, usize>(1)?)),
    )?;
    let invalid_group_exists = transaction.query_row(
        &format!(
            "SELECT EXISTS (
                     SELECT 1
                     FROM {IMPORT_SCHEMA}.code_repository_reference_search_groups search_group
                     WHERE search_group.source_scope = ?1
                       AND (
                           search_group.occurrence_count != (
                               SELECT COUNT(*)
                               FROM {IMPORT_SCHEMA}.code_repository_references reference
                               WHERE reference.source_scope = search_group.source_scope
                                 AND reference.name = search_group.name
                                 AND reference.kind = search_group.kind
                                 AND reference.path = search_group.path
                                 AND coalesce(reference.target_hint, '') = search_group.target_hint
                           )
                           OR search_group.group_id != (
                               SELECT MIN(reference.reference_id)
                               FROM {IMPORT_SCHEMA}.code_repository_references reference
                               WHERE reference.source_scope = search_group.source_scope
                                 AND reference.name = search_group.name
                                 AND reference.kind = search_group.kind
                                 AND reference.path = search_group.path
                                 AND coalesce(reference.target_hint, '') = search_group.target_hint
                           )
                           OR search_group.language_id != coalesce((
                               SELECT file.language_id
                               FROM {IMPORT_SCHEMA}.code_repository_files file
                               WHERE file.source_scope = search_group.source_scope
                                 AND file.path = search_group.path
                           ), '')
                       )
                     LIMIT 1
                 )"
        ),
        params![source_scope],
        |row| row.get::<_, bool>(0),
    )?;
    if actual_reference_count != reference_count
        || actual_group_count != group_count
        || occurrence_count != reference_count
        || reference_metadata_count != group_count
        || exact_search_owner_count != group_count
        || invalid_group_exists
    {
        return Ok(false);
    }
    let mut statement = transaction.prepare(&format!(
        "SELECT search_group.name, search_group.kind, search_group.target_hint,
                search_group.path, search_row.content
         FROM {IMPORT_SCHEMA}.code_repository_reference_search_groups search_group
         JOIN {IMPORT_SCHEMA}.code_repository_search_metadata metadata
           ON metadata.source_scope = search_group.source_scope
          AND metadata.document_kind = 'reference'
          AND metadata.record_id = search_group.group_id
          AND metadata.path = search_group.path
         CROSS JOIN {IMPORT_SCHEMA}.code_repository_search search_row
           ON search_row.rowid = metadata.search_rowid
          AND search_row.source_scope = metadata.source_scope
          AND search_row.document_kind = metadata.document_kind
          AND search_row.record_id = metadata.record_id
          AND search_row.path = metadata.path
         WHERE search_group.source_scope = ?1"
    ))?;
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
            return Ok(false);
        }
    }
    Ok(true)
}

fn attached_code_schema_migration_applied(
    transaction: &Transaction<'_>,
    migration: &str,
) -> Result<bool, StorageError> {
    if !attached_table_exists(transaction, "code_repository_schema_migrations")? {
        return Ok(false);
    }
    transaction
        .query_row(
            &format!(
                "
                SELECT EXISTS (
                    SELECT 1
                    FROM {IMPORT_SCHEMA}.code_repository_schema_migrations
                    WHERE name = ?1
                )
                "
            ),
            [migration],
            |row| row.get::<_, bool>(0),
        )
        .map_err(StorageError::from)
}

pub(super) fn attached_table_exists(
    transaction: &Transaction<'_>,
    table: &str,
) -> Result<bool, StorageError> {
    transaction
        .query_row(
            &format!(
                "
                SELECT EXISTS (
                    SELECT 1
                    FROM {IMPORT_SCHEMA}.sqlite_master
                    WHERE type = 'table' AND name = ?1
                )
                "
            ),
            [table],
            |row| row.get::<_, bool>(0),
        )
        .map_err(StorageError::from)
}

pub(super) fn attached_table_has_column(
    transaction: &Transaction<'_>,
    table: &str,
    column: &str,
) -> Result<bool, StorageError> {
    let mut statement =
        transaction.prepare(&format!("PRAGMA {IMPORT_SCHEMA}.table_info({table})"))?;
    let rows = statement.query_map([], |row| row.get::<_, String>(1))?;
    for row in rows {
        if row? == column {
            return Ok(true);
        }
    }

    Ok(false)
}
