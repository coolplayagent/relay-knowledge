//! Exact schema gate for the durable incremental-clone owner.

use rusqlite::Connection;

use crate::storage::StorageError;

use super::introspection::{
    table_column_is_not_null, table_column_is_nullable, table_has_primary_key_columns,
};

const TABLE: &str = "code_repository_incremental_clone_progress";
const PROGRESS_DDL: &str = r#"
CREATE TABLE code_repository_incremental_clone_progress (
    source_scope TEXT NOT NULL PRIMARY KEY,
    repository_id TEXT NOT NULL,
    base_scope TEXT NOT NULL,
    task_id TEXT NOT NULL,
    delta_digest TEXT NOT NULL,
    protocol_version INTEGER NOT NULL CHECK (protocol_version = 1),
    phase TEXT NOT NULL CHECK (
        phase IN ('tables', 'search', 'clone_complete')
    ),
    table_ordinal INTEGER NOT NULL CHECK (table_ordinal >= 0),
    completed_page_ordinal INTEGER NOT NULL CHECK (completed_page_ordinal >= 0),
    cursor_key TEXT,
    cursor_tiebreaker TEXT,
    completed_table_ordinal INTEGER CHECK (completed_table_ordinal >= 0),
    expected_table_rows INTEGER CHECK (expected_table_rows >= 0),
    scanned_table_rows INTEGER NOT NULL CHECK (scanned_table_rows >= 0),
    copied_table_rows INTEGER NOT NULL CHECK (copied_table_rows >= 0),
    scanned_total_rows INTEGER NOT NULL CHECK (scanned_total_rows >= 0),
    copied_total_rows INTEGER NOT NULL CHECK (copied_total_rows >= 0),
    copied_total_bytes INTEGER NOT NULL CHECK (copied_total_bytes >= 0),
    cloned_file_count INTEGER NOT NULL CHECK (cloned_file_count >= 0),
    cloned_symbol_count INTEGER NOT NULL CHECK (cloned_symbol_count >= 0),
    cloned_reference_count INTEGER NOT NULL CHECK (cloned_reference_count >= 0),
    cloned_chunk_count INTEGER NOT NULL CHECK (cloned_chunk_count >= 0),
    cloned_diagnostic_count INTEGER NOT NULL CHECK (cloned_diagnostic_count >= 0),
    cloned_reference_group_count INTEGER NOT NULL
        CHECK (cloned_reference_group_count >= 0),
    cloned_search_document_count INTEGER NOT NULL
        CHECK (cloned_search_document_count >= 0),
    base_manifest_reference_count INTEGER NOT NULL
        CHECK (base_manifest_reference_count >= 0),
    base_manifest_group_count INTEGER NOT NULL
        CHECK (base_manifest_group_count >= 0),
    scanned_reference_occurrence_count INTEGER NOT NULL
        CHECK (scanned_reference_occurrence_count >= 0),
    scanned_reference_row_count INTEGER NOT NULL
        CHECK (scanned_reference_row_count >= 0),
    scanned_reference_group_count INTEGER NOT NULL
        CHECK (scanned_reference_group_count >= 0),
    scanned_reference_search_owner_count INTEGER NOT NULL
        CHECK (scanned_reference_search_owner_count >= 0),
    base_source_fact_row_upper_bound INTEGER NOT NULL
        CHECK (base_source_fact_row_upper_bound > 0),
    page_row_limit INTEGER NOT NULL CHECK (page_row_limit > 0),
    page_byte_limit INTEGER NOT NULL CHECK (page_byte_limit > 0),
    updated_at_ms INTEGER NOT NULL,
    FOREIGN KEY (source_scope) REFERENCES code_repository_scopes(source_scope)
        ON DELETE CASCADE,
    FOREIGN KEY (base_scope) REFERENCES code_repository_scopes(source_scope)
        ON DELETE RESTRICT
)
"#;
const AFFECTED_PATHS_DDL: &str = r#"
CREATE TABLE code_repository_incremental_clone_affected_paths (
    source_scope TEXT NOT NULL,
    path TEXT NOT NULL,
    PRIMARY KEY (source_scope, path),
    FOREIGN KEY (source_scope)
        REFERENCES code_repository_incremental_clone_progress(source_scope)
        ON DELETE CASCADE
)
"#;
const COLUMNS: &[&str] = &[
    "source_scope",
    "repository_id",
    "base_scope",
    "task_id",
    "delta_digest",
    "protocol_version",
    "phase",
    "table_ordinal",
    "completed_page_ordinal",
    "cursor_key",
    "cursor_tiebreaker",
    "completed_table_ordinal",
    "expected_table_rows",
    "scanned_table_rows",
    "copied_table_rows",
    "scanned_total_rows",
    "copied_total_rows",
    "copied_total_bytes",
    "cloned_file_count",
    "cloned_symbol_count",
    "cloned_reference_count",
    "cloned_chunk_count",
    "cloned_diagnostic_count",
    "cloned_reference_group_count",
    "cloned_search_document_count",
    "base_manifest_reference_count",
    "base_manifest_group_count",
    "scanned_reference_occurrence_count",
    "scanned_reference_row_count",
    "scanned_reference_group_count",
    "scanned_reference_search_owner_count",
    "base_source_fact_row_upper_bound",
    "page_row_limit",
    "page_byte_limit",
    "updated_at_ms",
];

pub(in crate::storage::sqlite) fn schema_is_current(
    connection: &Connection,
) -> Result<bool, StorageError> {
    if !checkpoint_fact_proof_is_current(connection)?
        || !table_has_exact_plain_columns(connection, TABLE, COLUMNS)?
        || !table_has_primary_key_columns(connection, TABLE, &["source_scope"])?
        || !has_exact_index_inventory(connection, TABLE, true)?
        || table_has_triggers(connection, TABLE)?
    {
        return Ok(false);
    }
    for column in COLUMNS.iter().copied().filter(|column| {
        !matches!(
            *column,
            "cursor_key" | "cursor_tiebreaker" | "completed_table_ordinal" | "expected_table_rows"
        )
    }) {
        if !table_column_is_not_null(connection, TABLE, column)? {
            return Ok(false);
        }
    }
    for column in [
        "cursor_key",
        "cursor_tiebreaker",
        "completed_table_ordinal",
        "expected_table_rows",
    ] {
        if !table_column_is_nullable(connection, TABLE, column)? {
            return Ok(false);
        }
    }
    let definition = connection.query_row(
        "SELECT sql FROM sqlite_master WHERE type = 'table' AND name = ?1",
        [TABLE],
        |row| row.get::<_, String>(0),
    )?;
    if normalize_definition(&definition) != normalize_definition(PROGRESS_DDL) {
        return Ok(false);
    }
    let definition = normalize_definition(&definition);
    if definition.contains("collate")
        || [
        "source_scope text not null primary key",
        "protocol_version integer not null check (protocol_version = 1)",
        "phase text not null check ( phase in ('tables', 'search', 'clone_complete') )",
        "table_ordinal integer not null check (table_ordinal >= 0)",
        "completed_page_ordinal integer not null check (completed_page_ordinal >= 0)",
        "completed_table_ordinal integer check (completed_table_ordinal >= 0)",
        "expected_table_rows integer check (expected_table_rows >= 0)",
        "scanned_table_rows integer not null check (scanned_table_rows >= 0)",
        "copied_table_rows integer not null check (copied_table_rows >= 0)",
        "scanned_total_rows integer not null check (scanned_total_rows >= 0)",
        "copied_total_rows integer not null check (copied_total_rows >= 0)",
        "copied_total_bytes integer not null check (copied_total_bytes >= 0)",
        "cloned_file_count integer not null check (cloned_file_count >= 0)",
        "cloned_symbol_count integer not null check (cloned_symbol_count >= 0)",
        "cloned_reference_count integer not null check (cloned_reference_count >= 0)",
        "cloned_chunk_count integer not null check (cloned_chunk_count >= 0)",
        "cloned_diagnostic_count integer not null check (cloned_diagnostic_count >= 0)",
        "cloned_reference_group_count integer not null check (cloned_reference_group_count >= 0)",
        "cloned_search_document_count integer not null check (cloned_search_document_count >= 0)",
        "base_manifest_reference_count integer not null check (base_manifest_reference_count >= 0)",
        "base_manifest_group_count integer not null check (base_manifest_group_count >= 0)",
        "scanned_reference_occurrence_count integer not null check (scanned_reference_occurrence_count >= 0)",
        "scanned_reference_row_count integer not null check (scanned_reference_row_count >= 0)",
        "scanned_reference_group_count integer not null check (scanned_reference_group_count >= 0)",
        "scanned_reference_search_owner_count integer not null check (scanned_reference_search_owner_count >= 0)",
        "base_source_fact_row_upper_bound integer not null check (base_source_fact_row_upper_bound > 0)",
        "page_row_limit integer not null check (page_row_limit > 0)",
        "page_byte_limit integer not null check (page_byte_limit > 0)",
    ]
        .iter()
        .any(|shape| !definition.contains(shape))
    {
        return Ok(false);
    }
    let mut statement = connection.prepare(&format!("PRAGMA foreign_key_list({TABLE})"))?;
    let mut foreign_keys = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, String>(6)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    foreign_keys.sort();
    let mut expected = vec![
        (
            "code_repository_scopes".to_owned(),
            "base_scope".to_owned(),
            "source_scope".to_owned(),
            "RESTRICT".to_owned(),
        ),
        (
            "code_repository_scopes".to_owned(),
            "source_scope".to_owned(),
            "source_scope".to_owned(),
            "CASCADE".to_owned(),
        ),
    ];
    expected.sort();
    if foreign_keys != expected
        || !table_has_exact_plain_columns(
            connection,
            "code_repository_incremental_clone_affected_paths",
            &["source_scope", "path"],
        )?
        || !table_has_primary_key_columns(
            connection,
            "code_repository_incremental_clone_affected_paths",
            &["source_scope", "path"],
        )?
        || !table_column_is_not_null(
            connection,
            "code_repository_incremental_clone_affected_paths",
            "source_scope",
        )?
        || !table_column_is_not_null(
            connection,
            "code_repository_incremental_clone_affected_paths",
            "path",
        )?
        || !has_exact_index_inventory(
            connection,
            "code_repository_incremental_clone_affected_paths",
            false,
        )?
        || table_has_triggers(
            connection,
            "code_repository_incremental_clone_affected_paths",
        )?
    {
        return Ok(false);
    }
    let mut paths_foreign_keys = connection
        .prepare("PRAGMA foreign_key_list(code_repository_incremental_clone_affected_paths)")?;
    let paths_foreign_keys = paths_foreign_keys
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, String>(6)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let paths_definition = connection.query_row(
        "SELECT sql FROM sqlite_master
             WHERE type = 'table'
               AND name = 'code_repository_incremental_clone_affected_paths'",
        [],
        |row| row.get::<_, String>(0),
    )?;
    if normalize_definition(&paths_definition) != normalize_definition(AFFECTED_PATHS_DDL) {
        return Ok(false);
    }
    let paths_definition = normalize_definition(&paths_definition);
    Ok(!paths_definition.contains("collate")
        && paths_definition.contains("source_scope text not null")
        && paths_definition.contains("path text not null")
        && paths_definition.contains("primary key (source_scope, path)")
        && paths_foreign_keys.as_slice()
            == [(
                "code_repository_incremental_clone_progress".to_owned(),
                "source_scope".to_owned(),
                "source_scope".to_owned(),
                "CASCADE".to_owned(),
            )]
            .as_slice())
}

fn checkpoint_fact_proof_is_current(connection: &Connection) -> Result<bool, StorageError> {
    let mut statement =
        connection.prepare("PRAGMA table_xinfo(code_repository_index_checkpoints)")?;
    let columns = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, bool>(3)?,
                row.get::<_, Option<String>>(4)?,
                row.get::<_, i64>(5)?,
                row.get::<_, i64>(6)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let column_is_exact = columns
        .iter()
        .any(|(name, kind, not_null, default, primary, hidden)| {
            name == "committed_fact_row_count"
                && kind.eq_ignore_ascii_case("INTEGER")
                && *not_null
                && default.as_deref() == Some("0")
                && *primary == 0
                && *hidden == 0
        });
    let receipt_column_is_exact =
        columns
            .iter()
            .any(|(name, kind, not_null, default, primary, hidden)| {
                name == "incremental_summary_json"
                    && kind.eq_ignore_ascii_case("TEXT")
                    && !*not_null
                    && default.is_none()
                    && *primary == 0
                    && *hidden == 0
            });
    if !column_is_exact
        || !receipt_column_is_exact
        || !checkpoint_has_exact_trigger_inventory(connection)?
        || !checkpoint_has_exact_index_inventory(connection)?
    {
        return Ok(false);
    }
    let definition = connection.query_row(
        "SELECT sql FROM sqlite_master
         WHERE type = 'table' AND name = 'code_repository_index_checkpoints'",
        [],
        |row| row.get::<_, String>(0),
    )?;
    let compact = definition
        .chars()
        .filter(|character| !character.is_whitespace())
        .flat_map(char::to_lowercase)
        .collect::<String>();
    let proof_column = "committed_fact_row_countintegernotnulldefault0";
    let Some(proof_start) = compact.find("committed_fact_row_count") else {
        return Ok(false);
    };
    let proof_tail = &compact[proof_start..];
    let proof_end = proof_tail.find([',', ')']).unwrap_or(proof_tail.len());
    let receipt_column = "incremental_summary_jsontext";
    let Some(receipt_start) = compact.find("incremental_summary_json") else {
        return Ok(false);
    };
    let receipt_tail = &compact[receipt_start..];
    let receipt_end = receipt_tail.find([',', ')']).unwrap_or(receipt_tail.len());
    Ok(&proof_tail[..proof_end] == proof_column
        && compact.matches("committed_fact_row_count").count() == 1
        && &receipt_tail[..receipt_end] == receipt_column
        && compact.matches("incremental_summary_json").count() == 1
        && !compact.contains("check(")
        && !compact.contains("collate")
        && !compact.contains("withoutrowid"))
}

fn checkpoint_has_exact_index_inventory(connection: &Connection) -> Result<bool, StorageError> {
    let table = "code_repository_index_checkpoints";
    let mut statement = connection.prepare(&format!("PRAGMA index_list({table})"))?;
    let indexes = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(1)?,
                row.get::<_, bool>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, bool>(4)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    if !matches!(indexes.len(), 3 | 4) {
        return Ok(false);
    }
    let Some((primary, true, _, false)) = indexes.iter().find(|(_, _, origin, _)| origin == "pk")
    else {
        return Ok(false);
    };
    let expected: [(&str, &[(&str, bool)]); 3] = [
        (
            "code_repository_index_checkpoints_repository_scope",
            &[("repository_id", false), ("source_scope", false)],
        ),
        (
            "code_repository_index_checkpoints_publication_retention",
            &[
                ("repository_id", false),
                ("state", false),
                ("updated_at_ms", true),
                ("source_scope", true),
            ],
        ),
        (
            "code_repository_index_checkpoints_scope_activity",
            &[
                ("repository_id", false),
                ("source_scope", false),
                ("state", false),
                ("updated_at_ms", true),
            ],
        ),
    ];
    if !index_has_exact_binary_keys(connection, primary, &[("source_scope", false)])? {
        return Ok(false);
    }
    for (name, keys) in expected.into_iter().take(indexes.len() - 1) {
        let Some((_, false, origin, false)) =
            indexes.iter().find(|(actual, _, _, _)| actual == name)
        else {
            return Ok(false);
        };
        if origin != "c" {
            return Ok(false);
        }
        if !index_has_exact_binary_keys(connection, name, keys)? {
            return Ok(false);
        }
    }
    Ok(true)
}

const CHECKPOINT_TRIGGER_DEFINITIONS: &[(&str, &str)] = &[
    (
        "code_repository_retention_activity_checkpoint_delete",
        r#"CREATE TRIGGER code_repository_retention_activity_checkpoint_delete
        AFTER DELETE ON code_repository_index_checkpoints
        WHEN OLD.state IN ('complete', 'completed') BEGIN
            INSERT INTO code_repository_retention_activity_dirty (repository_id)
            SELECT OLD.repository_id
            WHERE EXISTS (
                SELECT 1 FROM code_repositories
                WHERE repository_id = OLD.repository_id
            ) AND NOT EXISTS (
                SELECT 1 FROM code_repository_retention_activity_dirty
                WHERE repository_id = OLD.repository_id
            );
        END"#,
    ),
    (
        "code_repository_retention_activity_checkpoint_insert",
        r#"CREATE TRIGGER code_repository_retention_activity_checkpoint_insert
        AFTER INSERT ON code_repository_index_checkpoints
        WHEN NEW.state IN ('complete', 'completed') BEGIN
            INSERT INTO code_repository_retention_activity_dirty (repository_id)
            SELECT NEW.repository_id
            WHERE NOT EXISTS (
                SELECT 1 FROM code_repository_retention_activity_dirty
                WHERE repository_id = NEW.repository_id
            );
        END"#,
    ),
    (
        "code_repository_retention_activity_checkpoint_update",
        r#"CREATE TRIGGER code_repository_retention_activity_checkpoint_update
        AFTER UPDATE OF repository_id, source_scope, state, updated_at_ms
        ON code_repository_index_checkpoints
        WHEN OLD.state IN ('complete', 'completed')
          OR NEW.state IN ('complete', 'completed') BEGIN
            INSERT INTO code_repository_retention_activity_dirty (repository_id)
            SELECT OLD.repository_id
            WHERE EXISTS (
                SELECT 1 FROM code_repositories
                WHERE repository_id = OLD.repository_id
            ) AND NOT EXISTS (
                SELECT 1 FROM code_repository_retention_activity_dirty
                WHERE repository_id = OLD.repository_id
            );
            INSERT INTO code_repository_retention_activity_dirty (repository_id)
            SELECT NEW.repository_id
            WHERE NOT EXISTS (
                SELECT 1 FROM code_repository_retention_activity_dirty
                WHERE repository_id = NEW.repository_id
            );
        END"#,
    ),
    (
        "code_repository_retention_catalog_checkpoint_delete",
        r#"CREATE TRIGGER code_repository_retention_catalog_checkpoint_delete
        AFTER DELETE ON code_repository_index_checkpoints
        WHEN OLD.state IN ('complete', 'completed') BEGIN
            UPDATE code_repository_retention_catalog SET revision = revision + 1
            WHERE catalog_id = 1;
        END"#,
    ),
    (
        "code_repository_retention_catalog_checkpoint_insert",
        r#"CREATE TRIGGER code_repository_retention_catalog_checkpoint_insert
        AFTER INSERT ON code_repository_index_checkpoints
        WHEN NEW.state IN ('complete', 'completed') BEGIN
            UPDATE code_repository_retention_catalog SET revision = revision + 1
            WHERE catalog_id = 1;
        END"#,
    ),
    (
        "code_repository_retention_catalog_checkpoint_update",
        r#"CREATE TRIGGER code_repository_retention_catalog_checkpoint_update
        AFTER UPDATE OF repository_id, source_scope, state, updated_at_ms
        ON code_repository_index_checkpoints
        WHEN OLD.state IN ('complete', 'completed')
          OR NEW.state IN ('complete', 'completed') BEGIN
            UPDATE code_repository_retention_catalog SET revision = revision + 1
            WHERE catalog_id = 1;
        END"#,
    ),
];

fn checkpoint_has_exact_trigger_inventory(connection: &Connection) -> Result<bool, StorageError> {
    let mut statement = connection.prepare(
        "SELECT name, sql
         FROM sqlite_master
         WHERE type = 'trigger' AND tbl_name = 'code_repository_index_checkpoints'
         ORDER BY name",
    )?;
    let actual = statement
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    if actual.is_empty() {
        return Ok(true);
    }
    Ok(actual.len() == CHECKPOINT_TRIGGER_DEFINITIONS.len()
        && actual.iter().zip(CHECKPOINT_TRIGGER_DEFINITIONS).all(
            |((actual_name, actual_definition), (expected_name, expected_definition))| {
                actual_name == expected_name
                    && normalize_definition(actual_definition)
                        == normalize_definition(expected_definition)
            },
        ))
}

fn normalize_definition(definition: &str) -> String {
    definition
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_ascii_lowercase()
}

fn table_has_exact_plain_columns(
    connection: &Connection,
    table: &str,
    expected: &[&str],
) -> Result<bool, StorageError> {
    let mut statement = connection.prepare(&format!("PRAGMA table_xinfo({table})"))?;
    let columns = statement
        .query_map([], |row| {
            Ok((row.get::<_, String>(1)?, row.get::<_, i64>(6)?))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(columns.len() == expected.len()
        && columns
            .iter()
            .zip(expected.iter().copied())
            .all(|((actual, hidden), expected)| actual == expected && *hidden == 0))
}

fn has_exact_index_inventory(
    connection: &Connection,
    table: &str,
    require_task_index: bool,
) -> Result<bool, StorageError> {
    let mut statement = connection.prepare(&format!("PRAGMA index_list({table})"))?;
    let indexes = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(1)?,
                row.get::<_, bool>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, bool>(4)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let expected_count = if require_task_index { 2 } else { 1 };
    if indexes.len() != expected_count {
        return Ok(false);
    }
    let Some((primary_name, true, _, false)) =
        indexes.iter().find(|(_, _, origin, _)| origin == "pk")
    else {
        return Ok(false);
    };
    let primary_columns = if require_task_index {
        &["source_scope"][..]
    } else {
        &["source_scope", "path"][..]
    };
    if !index_has_exact_binary_ascending_keys(connection, primary_name, primary_columns)? {
        return Ok(false);
    }
    if !require_task_index {
        return Ok(true);
    }
    let Some((task_name, false, origin, false)) = indexes
        .iter()
        .find(|(name, _, _, _)| name == "code_repository_incremental_clone_progress_task")
    else {
        return Ok(false);
    };
    if origin != "c" {
        return Ok(false);
    }
    index_has_exact_binary_ascending_keys(connection, task_name, &["task_id", "source_scope"])
}

fn index_has_exact_binary_ascending_keys(
    connection: &Connection,
    index: &str,
    expected: &[&str],
) -> Result<bool, StorageError> {
    let expected = expected
        .iter()
        .map(|column| (*column, false))
        .collect::<Vec<_>>();
    index_has_exact_binary_keys(connection, index, &expected)
}

fn index_has_exact_binary_keys(
    connection: &Connection,
    index: &str,
    expected: &[(&str, bool)],
) -> Result<bool, StorageError> {
    let quoted = index.replace('"', "\"\"");
    let mut statement = connection.prepare(&format!("PRAGMA index_xinfo(\"{quoted}\")"))?;
    let keys = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, Option<String>>(2)?,
                row.get::<_, bool>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, bool>(5)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .filter(|(_, _, _, key)| *key)
        .collect::<Vec<_>>();
    Ok(keys.len() == expected.len()
        && keys.iter().zip(expected.iter().copied()).all(
            |((actual, descending, collation, _), (expected, expected_descending))| {
                actual.as_deref() == Some(expected)
                    && *descending == expected_descending
                    && collation == "BINARY"
            },
        ))
}

fn table_has_triggers(connection: &Connection, table: &str) -> Result<bool, StorageError> {
    connection
        .query_row(
            "SELECT EXISTS (
                 SELECT 1 FROM sqlite_master
                 WHERE type = 'trigger' AND tbl_name = ?1
             )",
            [table],
            |row| row.get::<_, bool>(0),
        )
        .map_err(StorageError::from)
}
