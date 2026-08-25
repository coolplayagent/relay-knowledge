//! Static first-page and continuation SQL for grouped reference-search finalization.

pub(super) const CLEANUP_SCAN_FIRST: &str = "SELECT metadata.search_rowid,
         length(CAST(metadata.record_id AS BLOB)),
         length(CAST(search_row.source_scope AS BLOB))
         + length(CAST(search_row.document_kind AS BLOB))
         + length(CAST(search_row.record_id AS BLOB))
         + length(CAST(search_row.path AS BLOB))
         + length(CAST(search_row.language_id AS BLOB))
         + length(CAST(search_row.content AS BLOB))
         + length(CAST(metadata.source_scope AS BLOB))
         + length(CAST(metadata.document_kind AS BLOB))
         + length(CAST(metadata.record_id AS BLOB))
         + length(CAST(metadata.path AS BLOB))
         + coalesce(length(CAST(search_group.source_scope AS BLOB)), 0)
         + coalesce(length(CAST(search_group.group_id AS BLOB)), 0)
         + coalesce(length(CAST(search_group.name AS BLOB)), 0)
         + coalesce(length(CAST(search_group.kind AS BLOB)), 0)
         + coalesce(length(CAST(search_group.path AS BLOB)), 0)
         + coalesce(length(CAST(search_group.target_hint AS BLOB)), 0)
         + coalesce(length(CAST(search_group.language_id AS BLOB)), 0) + 180
     FROM code_repository_search_metadata metadata
     INNER JOIN code_repository_search search_row
        ON search_row.rowid = metadata.search_rowid
       AND search_row.source_scope = metadata.source_scope
       AND search_row.document_kind = metadata.document_kind
       AND search_row.record_id = metadata.record_id
       AND search_row.path = metadata.path
     LEFT JOIN code_repository_reference_search_groups search_group
       ON search_group.source_scope = metadata.source_scope
      AND search_group.group_id = metadata.record_id
      AND search_group.path = metadata.path
     WHERE metadata.source_scope = ?1 AND metadata.document_kind = 'reference'
     ORDER BY metadata.record_id LIMIT ?2";

pub(super) const CLEANUP_SCAN_AFTER: &str = "SELECT metadata.search_rowid,
         length(CAST(metadata.record_id AS BLOB)),
         length(CAST(search_row.source_scope AS BLOB))
         + length(CAST(search_row.document_kind AS BLOB))
         + length(CAST(search_row.record_id AS BLOB))
         + length(CAST(search_row.path AS BLOB))
         + length(CAST(search_row.language_id AS BLOB))
         + length(CAST(search_row.content AS BLOB))
         + length(CAST(metadata.source_scope AS BLOB))
         + length(CAST(metadata.document_kind AS BLOB))
         + length(CAST(metadata.record_id AS BLOB))
         + length(CAST(metadata.path AS BLOB))
         + coalesce(length(CAST(search_group.source_scope AS BLOB)), 0)
         + coalesce(length(CAST(search_group.group_id AS BLOB)), 0)
         + coalesce(length(CAST(search_group.name AS BLOB)), 0)
         + coalesce(length(CAST(search_group.kind AS BLOB)), 0)
         + coalesce(length(CAST(search_group.path AS BLOB)), 0)
         + coalesce(length(CAST(search_group.target_hint AS BLOB)), 0)
         + coalesce(length(CAST(search_group.language_id AS BLOB)), 0) + 180
     FROM code_repository_search_metadata metadata
     INNER JOIN code_repository_search search_row
        ON search_row.rowid = metadata.search_rowid
       AND search_row.source_scope = metadata.source_scope
       AND search_row.document_kind = metadata.document_kind
       AND search_row.record_id = metadata.record_id
       AND search_row.path = metadata.path
     LEFT JOIN code_repository_reference_search_groups search_group
       ON search_group.source_scope = metadata.source_scope
      AND search_group.group_id = metadata.record_id
      AND search_group.path = metadata.path
     WHERE metadata.source_scope = ?1 AND metadata.document_kind = 'reference'
       AND metadata.record_id > ?2
     ORDER BY metadata.record_id LIMIT ?3";

pub(super) const DISCOVERY_SCAN_FIRST: &str = "SELECT reference.rowid,
         length(CAST(reference.reference_id AS BLOB)),
         length(CAST(reference.source_scope AS BLOB))
         + length(CAST(reference.reference_id AS BLOB))
         + length(CAST(reference.name AS BLOB))
         + length(CAST(reference.kind AS BLOB))
         + length(CAST(reference.path AS BLOB))
         + length(CAST(coalesce(reference.target_hint, '') AS BLOB))
         + length(CAST(coalesce(file.language_id, '') AS BLOB)) + 81
     FROM code_repository_references reference
     LEFT JOIN code_repository_files file
       ON file.source_scope = reference.source_scope AND file.path = reference.path
     WHERE reference.source_scope = ?1
     ORDER BY reference.reference_id LIMIT ?2";

pub(super) const DISCOVERY_SCAN_AFTER: &str = "SELECT reference.rowid,
         length(CAST(reference.reference_id AS BLOB)),
         length(CAST(reference.source_scope AS BLOB))
         + length(CAST(reference.reference_id AS BLOB))
         + length(CAST(reference.name AS BLOB))
         + length(CAST(reference.kind AS BLOB))
         + length(CAST(reference.path AS BLOB))
         + length(CAST(coalesce(reference.target_hint, '') AS BLOB))
         + length(CAST(coalesce(file.language_id, '') AS BLOB)) + 81
     FROM code_repository_references reference
     LEFT JOIN code_repository_files file
       ON file.source_scope = reference.source_scope AND file.path = reference.path
     WHERE reference.source_scope = ?1 AND reference.reference_id > ?2
     ORDER BY reference.reference_id LIMIT ?3";

pub(super) const BUILD_SCAN_FIRST: &str = "SELECT search_group.rowid,
         length(CAST(search_group.group_id AS BLOB)),
         length(CAST(search_group.source_scope AS BLOB))
         + length(CAST('reference' AS BLOB))
         + length(CAST(search_group.group_id AS BLOB))
         + length(CAST(search_group.path AS BLOB))
         + length(CAST(search_group.language_id AS BLOB))
         + 4 * (length(CAST(search_group.name AS BLOB)) + 1
             + length(CAST(search_group.kind AS BLOB)) + 1
             + length(CAST(search_group.target_hint AS BLOB)) + 1
             + length(CAST(search_group.path AS BLOB)))
         + length(CAST(search_group.source_scope AS BLOB))
         + length(CAST('reference' AS BLOB))
         + length(CAST(search_group.group_id AS BLOB))
         + length(CAST(search_group.path AS BLOB)) + 108
     FROM code_repository_reference_search_groups search_group
     WHERE search_group.source_scope = ?1
     ORDER BY search_group.group_id LIMIT ?2";

pub(super) const BUILD_SCAN_AFTER: &str = "SELECT search_group.rowid,
         length(CAST(search_group.group_id AS BLOB)),
         length(CAST(search_group.source_scope AS BLOB))
         + length(CAST('reference' AS BLOB))
         + length(CAST(search_group.group_id AS BLOB))
         + length(CAST(search_group.path AS BLOB))
         + length(CAST(search_group.language_id AS BLOB))
         + 4 * (length(CAST(search_group.name AS BLOB)) + 1
             + length(CAST(search_group.kind AS BLOB)) + 1
             + length(CAST(search_group.target_hint AS BLOB)) + 1
             + length(CAST(search_group.path AS BLOB)))
         + length(CAST(search_group.source_scope AS BLOB))
         + length(CAST('reference' AS BLOB))
         + length(CAST(search_group.group_id AS BLOB))
         + length(CAST(search_group.path AS BLOB)) + 108
     FROM code_repository_reference_search_groups search_group
     WHERE search_group.source_scope = ?1 AND search_group.group_id > ?2
     ORDER BY search_group.group_id LIMIT ?3";

pub(super) const CLEANUP_FETCH_CURSOR: &str = "SELECT record_id
     FROM code_repository_search_metadata
     WHERE source_scope = ?1 AND document_kind = 'reference' AND search_rowid = ?2";
pub(super) const DISCOVERY_FETCH_CURSOR: &str = "SELECT reference_id
     FROM code_repository_references WHERE source_scope = ?1 AND rowid = ?2";
pub(super) const BUILD_FETCH_CURSOR: &str = "SELECT group_id
     FROM code_repository_reference_search_groups WHERE source_scope = ?1 AND rowid = ?2";

pub(super) const CLEANUP_GROUPS_FIRST: &str = "DELETE FROM code_repository_reference_search_groups
     WHERE source_scope = ?1 AND group_id IN (
         SELECT record_id FROM code_repository_search_metadata
         WHERE source_scope = ?1 AND document_kind = 'reference' AND record_id <= ?2
     )";
pub(super) const CLEANUP_GROUPS_AFTER: &str = "DELETE FROM code_repository_reference_search_groups
     WHERE source_scope = ?1 AND group_id IN (
         SELECT record_id FROM code_repository_search_metadata
         WHERE source_scope = ?1 AND document_kind = 'reference'
           AND record_id > ?2 AND record_id <= ?3
     )";
pub(super) const CLEANUP_SEARCH_FIRST: &str = "DELETE FROM code_repository_search
     WHERE rowid IN (
         SELECT search_rowid FROM code_repository_search_metadata
         WHERE source_scope = ?1 AND document_kind = 'reference' AND record_id <= ?2
     )";
pub(super) const CLEANUP_SEARCH_AFTER: &str = "DELETE FROM code_repository_search
     WHERE rowid IN (
         SELECT search_rowid FROM code_repository_search_metadata
         WHERE source_scope = ?1 AND document_kind = 'reference'
           AND record_id > ?2 AND record_id <= ?3
     )";
pub(super) const CLEANUP_METADATA_FIRST: &str = "DELETE FROM code_repository_search_metadata
     WHERE source_scope = ?1 AND document_kind = 'reference' AND record_id <= ?2";
pub(super) const CLEANUP_METADATA_AFTER: &str = "DELETE FROM code_repository_search_metadata
     WHERE source_scope = ?1 AND document_kind = 'reference'
       AND record_id > ?2 AND record_id <= ?3";

pub(super) const DISCOVERY_UPSERT_FIRST: &str =
    "INSERT INTO code_repository_reference_search_groups (
         source_scope, group_id, name, kind, path, target_hint, language_id, occurrence_count
     )
     SELECT ?1, MIN(reference.reference_id), reference.name, reference.kind,
            reference.path, coalesce(reference.target_hint, ''),
            coalesce(file.language_id, ''), COUNT(*)
     FROM code_repository_references reference
     LEFT JOIN code_repository_files file
       ON file.source_scope = reference.source_scope AND file.path = reference.path
     WHERE reference.source_scope = ?1 AND reference.reference_id <= ?2
     GROUP BY reference.name, reference.kind, reference.path,
              coalesce(reference.target_hint, ''), coalesce(file.language_id, '')
     ON CONFLICT (source_scope, name, kind, path, target_hint) DO UPDATE SET
         group_id = min(group_id, excluded.group_id),
         language_id = excluded.language_id,
         occurrence_count = occurrence_count + excluded.occurrence_count
     RETURNING group_id";

pub(super) const DISCOVERY_UPSERT_AFTER: &str =
    "INSERT INTO code_repository_reference_search_groups (
         source_scope, group_id, name, kind, path, target_hint, language_id, occurrence_count
     )
     SELECT ?1, MIN(reference.reference_id), reference.name, reference.kind,
            reference.path, coalesce(reference.target_hint, ''),
            coalesce(file.language_id, ''), COUNT(*)
     FROM code_repository_references reference
     LEFT JOIN code_repository_files file
       ON file.source_scope = reference.source_scope AND file.path = reference.path
     WHERE reference.source_scope = ?1 AND reference.reference_id > ?2
       AND reference.reference_id <= ?3
     GROUP BY reference.name, reference.kind, reference.path,
              coalesce(reference.target_hint, ''), coalesce(file.language_id, '')
     ON CONFLICT (source_scope, name, kind, path, target_hint) DO UPDATE SET
         group_id = min(group_id, excluded.group_id),
         language_id = excluded.language_id,
         occurrence_count = occurrence_count + excluded.occurrence_count
     RETURNING group_id";

pub(super) const BUILD_INSERT_SEARCH_FIRST: &str = "INSERT INTO code_repository_search (
         source_scope, document_kind, record_id, path, language_id, content
     )
     SELECT source_scope, 'reference', group_id, path, language_id,
            CASE WHEN trim(name) = '' THEN '' ELSE name END
            || CASE WHEN trim(kind) = '' THEN ''
                    WHEN trim(name) = '' THEN kind ELSE ' ' || kind END
            || CASE WHEN trim(target_hint) = '' THEN ''
                    WHEN trim(name) = '' AND trim(kind) = '' THEN target_hint
                    ELSE ' ' || target_hint END
            || CASE WHEN trim(path) = '' THEN ''
                    WHEN trim(name) = '' AND trim(kind) = ''
                     AND trim(target_hint) = '' THEN path
                    ELSE ' ' || path END
     FROM code_repository_reference_search_groups
     WHERE source_scope = ?1 AND group_id <= ?2 ORDER BY group_id";
pub(super) const BUILD_INSERT_SEARCH_AFTER: &str = "INSERT INTO code_repository_search (
         source_scope, document_kind, record_id, path, language_id, content
     )
     SELECT source_scope, 'reference', group_id, path, language_id,
            CASE WHEN trim(name) = '' THEN '' ELSE name END
            || CASE WHEN trim(kind) = '' THEN ''
                    WHEN trim(name) = '' THEN kind ELSE ' ' || kind END
            || CASE WHEN trim(target_hint) = '' THEN ''
                    WHEN trim(name) = '' AND trim(kind) = '' THEN target_hint
                    ELSE ' ' || target_hint END
            || CASE WHEN trim(path) = '' THEN ''
                    WHEN trim(name) = '' AND trim(kind) = ''
                     AND trim(target_hint) = '' THEN path
                    ELSE ' ' || path END
     FROM code_repository_reference_search_groups
     WHERE source_scope = ?1 AND group_id > ?2 AND group_id <= ?3 ORDER BY group_id";
pub(super) const BUILD_INTERVAL_COUNT: &str = "SELECT COUNT(*)
     FROM code_repository_search
     WHERE rowid > ?1 AND rowid <= ?2";
pub(super) const BUILD_INSERT_METADATA: &str = "INSERT INTO code_repository_search_metadata (
         source_scope, document_kind, record_id, path, search_rowid
     )
     SELECT source_scope, document_kind, record_id, path, rowid
     FROM code_repository_search
     WHERE rowid > ?1 AND rowid <= ?2 AND source_scope = ?3";
