use rusqlite::{Connection, OptionalExtension, params};

use crate::storage::StorageError;

#[derive(Debug)]
pub(super) struct TableColumn {
    pub(super) name: String,
    pub(super) not_null: bool,
    pub(super) default_value: Option<String>,
}

pub(super) fn table_has_columns(
    connection: &Connection,
    table: &str,
    required_columns: &[&str],
) -> Result<bool, StorageError> {
    let columns = table_columns(connection, table)?;
    Ok(required_columns
        .iter()
        .all(|required| columns.iter().any(|column| column == required)))
}

pub(super) fn table_has_exact_columns(
    connection: &Connection,
    table: &str,
    expected_columns: &[&str],
) -> Result<bool, StorageError> {
    let columns = table_columns(connection, table)?;
    Ok(columns.len() == expected_columns.len()
        && expected_columns
            .iter()
            .zip(columns.iter())
            .all(|(expected, actual)| *expected == actual))
}

pub(super) fn table_has_exact_plain_columns(
    connection: &Connection,
    table: &str,
    expected_columns: &[&str],
) -> Result<bool, StorageError> {
    if !table_exists(connection, table)? {
        return Ok(false);
    }
    let mut statement = connection.prepare(&format!("PRAGMA table_xinfo({table})"))?;
    let rows = statement.query_map([], |row| {
        Ok((row.get::<_, String>(1)?, row.get::<_, usize>(6)?))
    })?;
    let columns = rows.collect::<Result<Vec<_>, _>>()?;
    Ok(columns.len() == expected_columns.len()
        && columns
            .iter()
            .zip(expected_columns.iter())
            .all(|((name, hidden), expected)| name == expected && *hidden == 0))
}

pub(super) fn table_has_primary_key_columns(
    connection: &Connection,
    table: &str,
    expected_columns: &[&str],
) -> Result<bool, StorageError> {
    if !table_exists(connection, table)? {
        return Ok(false);
    }
    let mut statement = connection.prepare(&format!("PRAGMA table_info({table})"))?;
    let rows = statement.query_map([], |row| {
        Ok((row.get::<_, usize>(5)?, row.get::<_, String>(1)?))
    })?;
    let mut primary_key = rows
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .filter(|(ordinal, _)| *ordinal > 0)
        .collect::<Vec<_>>();
    primary_key.sort_by_key(|(ordinal, _)| *ordinal);
    Ok(primary_key
        .iter()
        .map(|(_, column)| column.as_str())
        .eq(expected_columns.iter().copied()))
}

pub(super) fn table_column_is_not_null(
    connection: &Connection,
    table: &str,
    expected_column: &str,
) -> Result<bool, StorageError> {
    if !table_exists(connection, table)? {
        return Ok(false);
    }
    Ok(table_column_info(connection, table)?
        .iter()
        .any(|column| column.name == expected_column && column.not_null))
}

pub(super) fn table_column_is_nullable(
    connection: &Connection,
    table: &str,
    expected_column: &str,
) -> Result<bool, StorageError> {
    if !table_exists(connection, table)? {
        return Ok(false);
    }
    Ok(table_column_info(connection, table)?
        .iter()
        .any(|column| column.name == expected_column && !column.not_null))
}

pub(super) fn table_has_unique_columns(
    connection: &Connection,
    table: &str,
    expected_columns: &[&str],
) -> Result<bool, StorageError> {
    if !table_exists(connection, table)? {
        return Ok(false);
    }
    let mut statement = connection.prepare(&format!("PRAGMA index_list({table})"))?;
    let rows = statement.query_map([], |row| {
        Ok((
            row.get::<_, String>(1)?,
            row.get::<_, bool>(2)?,
            row.get::<_, bool>(4)?,
        ))
    })?;
    let indexes = rows.collect::<Result<Vec<_>, _>>()?;

    for (index_name, unique, partial) in indexes {
        if unique && !partial && index_columns_equal(connection, &index_name, expected_columns)? {
            return Ok(true);
        }
    }
    Ok(false)
}

pub(super) fn index_has_columns(
    connection: &Connection,
    index: &str,
    expected_columns: &[&str],
) -> Result<bool, StorageError> {
    let table = connection
        .query_row(
            "SELECT tbl_name FROM sqlite_master WHERE type = 'index' AND name = ?1",
            params![index],
            |row| row.get::<_, String>(0),
        )
        .optional()?;
    let Some(table) = table else {
        return Ok(false);
    };
    let mut statement = connection.prepare(&format!("PRAGMA index_list({table})"))?;
    let rows = statement.query_map([], |row| {
        Ok((row.get::<_, String>(1)?, row.get::<_, bool>(4)?))
    })?;
    let indexes = rows.collect::<Result<Vec<_>, _>>()?;
    if !indexes
        .iter()
        .any(|(name, partial)| name == index && !partial)
    {
        return Ok(false);
    }
    index_columns_equal(connection, index, expected_columns)
}

pub(super) fn table_column_info(
    connection: &Connection,
    table: &str,
) -> Result<Vec<TableColumn>, StorageError> {
    let mut statement = connection.prepare(&format!("PRAGMA table_info({table})"))?;
    let rows = statement.query_map([], |row| {
        Ok(TableColumn {
            name: row.get(1)?,
            not_null: row.get(3)?,
            default_value: row.get(4)?,
        })
    })?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(StorageError::from)
}

pub(super) fn table_columns_have_no_defaults(
    connection: &Connection,
    table: &str,
) -> Result<bool, StorageError> {
    if !table_exists(connection, table)? {
        return Ok(false);
    }
    let mut statement = connection.prepare(&format!("PRAGMA table_xinfo({table})"))?;
    let defaults = statement.query_map([], |row| row.get::<_, Option<String>>(4))?;
    Ok(defaults
        .collect::<Result<Vec<_>, _>>()?
        .iter()
        .all(Option::is_none))
}

pub(super) fn table_has_exact_primary_key_index_surface(
    connection: &Connection,
    table: &str,
    expected_columns: &[&str],
) -> Result<bool, StorageError> {
    if !table_exists(connection, table)? {
        return Ok(false);
    }
    let mut statement = connection.prepare(&format!("PRAGMA index_list({table})"))?;
    let indexes = statement.query_map([], |row| {
        Ok((
            row.get::<_, String>(1)?,
            row.get::<_, bool>(2)?,
            row.get::<_, String>(3)?,
            row.get::<_, bool>(4)?,
        ))
    })?;
    let indexes = indexes.collect::<Result<Vec<_>, _>>()?;
    let [(name, unique, origin, partial)] = indexes.as_slice() else {
        return Ok(false);
    };
    Ok(*unique
        && origin == "pk"
        && !partial
        && index_columns_equal(connection, name, expected_columns)?)
}

pub(super) fn table_has_no_triggers(
    connection: &Connection,
    table: &str,
) -> Result<bool, StorageError> {
    connection
        .query_row(
            "SELECT NOT EXISTS (
                 SELECT 1 FROM sqlite_master WHERE type = 'trigger' AND tbl_name = ?1
             )",
            params![table],
            |row| row.get::<_, bool>(0),
        )
        .map_err(StorageError::from)
}

pub(super) fn table_exists(connection: &Connection, table: &str) -> Result<bool, StorageError> {
    connection
        .query_row(
            "SELECT EXISTS (
                 SELECT 1 FROM sqlite_master
                 WHERE type = 'table' AND name = ?1
             )",
            params![table],
            |row| row.get::<_, bool>(0),
        )
        .map_err(StorageError::from)
}

fn table_columns(connection: &Connection, table: &str) -> Result<Vec<String>, StorageError> {
    let mut statement = connection.prepare(&format!("PRAGMA table_info({table})"))?;
    let rows = statement.query_map([], |row| row.get::<_, String>(1))?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(StorageError::from)
}

fn index_columns_equal(
    connection: &Connection,
    index: &str,
    expected_columns: &[&str],
) -> Result<bool, StorageError> {
    let mut statement = connection.prepare(&format!("PRAGMA index_info({index})"))?;
    let rows = statement.query_map([], |row| row.get::<_, Option<String>>(2))?;
    let columns = rows.collect::<Result<Vec<_>, _>>()?;
    Ok(columns
        .iter()
        .map(|column| column.as_deref())
        .eq(expected_columns.iter().copied().map(Some)))
}

#[cfg(test)]
#[path = "introspection_tests.rs"]
mod tests;
