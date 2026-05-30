use rusqlite::{OptionalExtension, params, params_from_iter, types::Value};

use crate::storage::StorageError;

const MAX_PATH_DELETE_PATHS_PER_STATEMENT: usize = 500;

pub(super) fn delete_scope_index(
    transaction: &rusqlite::Transaction<'_>,
    source_scope: &str,
) -> Result<(), StorageError> {
    for table in [
        "code_repository_path_tombstones",
        "code_repository_file_diagnostics",
        "code_repository_chunks",
        "code_repository_calls",
        "code_repository_feature_flags",
        "code_repository_dependencies",
        "code_repository_imports",
        "code_repository_references",
        "code_repository_symbols",
        "code_repository_files",
        "code_repository_search",
        "software_components",
        "software_dependency_usages",
        "software_sdk_usages",
        "software_global_status",
    ] {
        transaction.execute(
            &format!("DELETE FROM {table} WHERE source_scope = ?1"),
            params![source_scope],
        )?;
    }

    Ok(())
}

pub(super) fn delete_path_index(
    transaction: &rusqlite::Transaction<'_>,
    source_scope: &str,
    path: &str,
) -> Result<(), StorageError> {
    delete_path_indexes(transaction, source_scope, [path])
}

pub(super) fn path_indexes_exist<'path>(
    transaction: &rusqlite::Transaction<'_>,
    source_scope: &str,
    paths: impl IntoIterator<Item = &'path str>,
) -> Result<bool, StorageError> {
    let mut paths = paths.into_iter().collect::<Vec<_>>();
    paths.sort_unstable();
    paths.dedup();
    if paths.is_empty() {
        return Ok(false);
    }

    for path_chunk in paths.chunks(MAX_PATH_DELETE_PATHS_PER_STATEMENT) {
        let placeholders = std::iter::repeat_n("?", path_chunk.len())
            .collect::<Vec<_>>()
            .join(", ");
        let mut values = Vec::with_capacity(path_chunk.len() + 1);
        values.push(Value::Text(source_scope.to_owned()));
        values.extend(
            path_chunk
                .iter()
                .map(|path| Value::Text((*path).to_owned())),
        );
        let existing = transaction
            .query_row(
                &format!(
                    "SELECT 1 FROM code_repository_files WHERE source_scope = ? AND path IN ({placeholders}) LIMIT 1"
                ),
                params_from_iter(values),
                |_| Ok(()),
            )
            .optional()?;
        if existing.is_some() {
            return Ok(true);
        }
    }

    Ok(false)
}

pub(super) fn delete_path_indexes<'path>(
    transaction: &rusqlite::Transaction<'_>,
    source_scope: &str,
    paths: impl IntoIterator<Item = &'path str>,
) -> Result<(), StorageError> {
    let mut paths = paths.into_iter().collect::<Vec<_>>();
    paths.sort_unstable();
    paths.dedup();
    if paths.is_empty() {
        return Ok(());
    }

    for table in [
        "code_repository_file_diagnostics",
        "code_repository_chunks",
        "code_repository_calls",
        "code_repository_feature_flags",
        "code_repository_dependencies",
        "code_repository_imports",
        "code_repository_references",
        "code_repository_symbols",
        "code_repository_files",
        "code_repository_search",
    ] {
        for path_chunk in paths.chunks(MAX_PATH_DELETE_PATHS_PER_STATEMENT) {
            let placeholders = std::iter::repeat_n("?", path_chunk.len())
                .collect::<Vec<_>>()
                .join(", ");
            let mut values = Vec::with_capacity(path_chunk.len() + 1);
            values.push(Value::Text(source_scope.to_owned()));
            values.extend(
                path_chunk
                    .iter()
                    .map(|path| Value::Text((*path).to_owned())),
            );
            transaction.execute(
                &format!("DELETE FROM {table} WHERE source_scope = ? AND path IN ({placeholders})"),
                params_from_iter(values),
            )?;
        }
    }

    Ok(())
}

pub(super) fn count_code_rows(
    transaction: &rusqlite::Transaction<'_>,
    table: &'static str,
    source_scope: &str,
) -> Result<usize, StorageError> {
    transaction
        .query_row(
            &format!("SELECT COUNT(*) FROM {table} WHERE source_scope = ?1"),
            params![source_scope],
            |row| row.get(0),
        )
        .map_err(StorageError::from)
}

#[cfg(test)]
mod tests {
    use rusqlite::Connection;

    use super::*;

    const PATH_TABLES: &[&str] = &[
        "code_repository_file_diagnostics",
        "code_repository_chunks",
        "code_repository_calls",
        "code_repository_feature_flags",
        "code_repository_dependencies",
        "code_repository_imports",
        "code_repository_references",
        "code_repository_symbols",
        "code_repository_files",
    ];

    const SCOPE_TABLES: &[&str] = &[
        "code_repository_path_tombstones",
        "code_repository_file_diagnostics",
        "code_repository_chunks",
        "code_repository_calls",
        "code_repository_feature_flags",
        "code_repository_dependencies",
        "code_repository_imports",
        "code_repository_references",
        "code_repository_symbols",
        "code_repository_files",
        "software_components",
        "software_dependency_usages",
        "software_sdk_usages",
        "software_global_status",
    ];

    #[test]
    fn delete_scope_index_removes_software_projection_tables() {
        let mut connection = Connection::open_in_memory().expect("connection should open");
        for table in SCOPE_TABLES {
            connection
                .execute(
                    &format!("CREATE TABLE {table} (source_scope TEXT NOT NULL)"),
                    [],
                )
                .expect("table should create");
            connection
                .execute(
                    &format!("INSERT INTO {table} (source_scope) VALUES ('scope'), ('other')"),
                    [],
                )
                .expect("rows should insert");
        }
        connection
            .execute(
                "
                CREATE VIRTUAL TABLE code_repository_search USING fts5(
                    source_scope UNINDEXED,
                    document_kind UNINDEXED,
                    record_id UNINDEXED,
                    path UNINDEXED,
                    language_id UNINDEXED,
                    content
                )
                ",
                [],
            )
            .expect("search table should create");
        connection
            .execute(
                "
                INSERT INTO code_repository_search (
                    source_scope, document_kind, record_id, path, language_id, content
                )
                VALUES ('scope', 'symbol', 'a', 'src/a.rs', 'rust', 'target'),
                       ('other', 'symbol', 'b', 'src/b.rs', 'rust', 'target')
                ",
                [],
            )
            .expect("search rows should insert");

        let transaction = connection.transaction().expect("transaction should open");
        delete_scope_index(&transaction, "scope").expect("scope should delete");
        transaction.commit().expect("transaction should commit");

        for table in SCOPE_TABLES
            .iter()
            .copied()
            .chain(["code_repository_search"])
        {
            let deleted_remaining = connection
                .query_row(
                    &format!("SELECT COUNT(*) FROM {table} WHERE source_scope = 'scope'"),
                    [],
                    |row| row.get::<_, usize>(0),
                )
                .expect("deleted row count should load");
            let retained_remaining = connection
                .query_row(
                    &format!("SELECT COUNT(*) FROM {table} WHERE source_scope = 'other'"),
                    [],
                    |row| row.get::<_, usize>(0),
                )
                .expect("retained row count should load");
            assert_eq!(deleted_remaining, 0, "{table} should delete pruned scope");
            assert_eq!(retained_remaining, 1, "{table} should keep other scope");
        }
    }

    #[test]
    fn delete_path_indexes_removes_multiple_paths_from_all_path_tables() {
        let mut connection = Connection::open_in_memory().expect("connection should open");
        for table in PATH_TABLES {
            connection
                .execute(
                    &format!(
                        "CREATE TABLE {table} (source_scope TEXT NOT NULL, path TEXT NOT NULL)"
                    ),
                    [],
                )
                .expect("table should create");
        }
        connection
            .execute(
                "
                CREATE VIRTUAL TABLE code_repository_search USING fts5(
                    source_scope UNINDEXED,
                    document_kind UNINDEXED,
                    record_id UNINDEXED,
                    path UNINDEXED,
                    language_id UNINDEXED,
                    content
                )
                ",
                [],
            )
            .expect("search table should create");

        for path in ["src/a.rs", "src/b.rs", "src/c.rs"] {
            for table in PATH_TABLES {
                connection
                    .execute(
                        &format!("INSERT INTO {table} (source_scope, path) VALUES (?1, ?2)"),
                        rusqlite::params!["scope", path],
                    )
                    .expect("path row should insert");
            }
            connection
                .execute(
                    "
                    INSERT INTO code_repository_search (
                        source_scope, document_kind, record_id, path, language_id, content
                    )
                    VALUES (?1, 'symbol', ?2, ?2, 'rust', 'target')
                    ",
                    rusqlite::params!["scope", path],
                )
                .expect("search row should insert");
        }

        let transaction = connection.transaction().expect("transaction should open");
        assert!(
            path_indexes_exist(&transaction, "scope", ["src/a.rs", "src/b.rs"])
                .expect("path existence should load")
        );
        assert!(
            !path_indexes_exist(&transaction, "scope", ["src/missing.rs"])
                .expect("missing path existence should load")
        );
        delete_path_indexes(&transaction, "scope", ["src/a.rs", "src/b.rs", "src/a.rs"])
            .expect("paths should delete");
        transaction.commit().expect("transaction should commit");

        for table in PATH_TABLES
            .iter()
            .copied()
            .chain(["code_repository_search"])
        {
            let remaining = connection
                .query_row(
                    &format!("SELECT COUNT(*) FROM {table} WHERE source_scope = 'scope'"),
                    [],
                    |row| row.get::<_, usize>(0),
                )
                .expect("remaining row count should load");
            assert_eq!(remaining, 1, "{table} should keep only the unmatched path");
        }
    }
}
