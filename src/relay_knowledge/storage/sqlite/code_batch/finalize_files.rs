use std::collections::BTreeMap;

use rusqlite::{Transaction, params};

use crate::storage::StorageError;

pub(super) fn load_file_languages(
    transaction: &Transaction<'_>,
    source_scope: &str,
) -> Result<BTreeMap<String, String>, StorageError> {
    let mut statement = transaction.prepare(
        "
        SELECT path, language_id
        FROM code_repository_files
        WHERE source_scope = ?1
        ",
    )?;
    let rows = statement.query_map(params![source_scope], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
    })?;
    let pairs = rows
        .collect::<Result<Vec<_>, _>>()
        .map_err(StorageError::from)?;

    Ok(pairs.into_iter().collect())
}
