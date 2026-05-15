use std::{
    collections::BTreeMap,
    ffi::OsString,
    path::{Path, PathBuf},
};

use rusqlite::Connection;

use crate::storage::StorageError;

type SchemaDefinition = BTreeMap<String, SchemaEntry>;

#[derive(Debug, PartialEq, Eq)]
struct SchemaEntry {
    kind: String,
    table: String,
    sql: String,
}

pub(super) fn database_requires_reset(connection: &Connection) -> Result<bool, StorageError> {
    let existing_schema = application_schema(connection)?;
    if existing_schema.is_empty() {
        return Ok(false);
    }

    Ok(existing_schema != current_application_schema()?)
}

pub(super) fn remove_database_files(path: &Path) -> Result<(), StorageError> {
    remove_file_if_exists(path)?;
    remove_file_if_exists(&sqlite_sidecar_path(path, "-wal"))?;
    remove_file_if_exists(&sqlite_sidecar_path(path, "-shm"))?;
    remove_file_if_exists(&sqlite_sidecar_path(path, "-journal"))?;

    Ok(())
}

fn current_application_schema() -> Result<SchemaDefinition, StorageError> {
    let connection = Connection::open_in_memory()?;
    super::initialize_schema(&connection)?;

    application_schema(&connection)
}

fn application_schema(connection: &Connection) -> Result<SchemaDefinition, StorageError> {
    let mut statement = connection.prepare(
        "
        SELECT type, name, tbl_name, sql
        FROM sqlite_master
        WHERE name NOT LIKE 'sqlite_%'
          AND sql IS NOT NULL
        ORDER BY type ASC, name ASC
        ",
    )?;
    let rows = statement.query_map([], |row| {
        let kind = row.get::<_, String>(0)?;
        let name = row.get::<_, String>(1)?;
        let table = row.get::<_, String>(2)?;
        let sql = row.get::<_, String>(3)?;
        Ok((name, SchemaEntry { kind, table, sql }))
    })?;

    let mut schema = SchemaDefinition::new();
    for row in rows {
        let (name, entry) = row?;
        schema.insert(name, entry);
    }

    Ok(schema)
}

fn sqlite_sidecar_path(path: &Path, suffix: &str) -> PathBuf {
    let mut raw = OsString::from(path.as_os_str());
    raw.push(suffix);
    PathBuf::from(raw)
}

fn remove_file_if_exists(path: &Path) -> Result<(), StorageError> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(StorageError::from(error)),
    }
}
