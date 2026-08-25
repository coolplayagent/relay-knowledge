use std::collections::BTreeSet;

use rusqlite::{Connection, params};

use crate::{code::source_roots::source_module_candidates, storage::StorageError};

pub(super) fn local_modules(
    connection: &Connection,
    source_scope: &str,
    file_limit: usize,
    module_limit: usize,
) -> Result<BTreeSet<String>, StorageError> {
    let mut statement = connection.prepare(
        "
        SELECT path
        FROM code_repository_files
        WHERE source_scope = ?1
          AND language_id = 'python'
        LIMIT ?2
        ",
    )?;
    let rows = statement.query_map(
        params![source_scope, file_limit.saturating_add(1) as i64],
        |row| row.get::<_, String>(0),
    )?;
    let mut modules = BTreeSet::new();
    let mut file_count = 0_usize;
    for row in rows {
        file_count = file_count.saturating_add(1);
        if file_count > file_limit {
            return Err(StorageError::CapacityExceeded(format!(
                "software dependency Python files exceed the bounded limit {file_limit}"
            )));
        }
        let path = row?;
        for candidate in source_module_candidates(&path) {
            if let Some(module) = module_from_file_path(&candidate) {
                modules.insert(super::matching::normalize_key(&module));
                if modules.len() > module_limit {
                    return Err(StorageError::CapacityExceeded(format!(
                        "software dependency Python local modules exceed the bounded limit {module_limit}"
                    )));
                }
            }
        }
    }

    Ok(modules)
}

pub(super) fn module_from_file_path(path: &str) -> Option<String> {
    let path = path.trim().trim_start_matches("./");
    let module_path = path
        .strip_suffix("/__init__.py")
        .or_else(|| path.strip_suffix("/__init__.pyw"))
        .or_else(|| path.strip_suffix(".py"))
        .or_else(|| path.strip_suffix(".pyw"))?;
    (!module_path.is_empty()).then(|| module_path.replace('/', "."))
}

#[cfg(test)]
#[path = "python_tests.rs"]
mod tests;
