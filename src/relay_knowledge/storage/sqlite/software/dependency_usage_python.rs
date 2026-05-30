use std::collections::BTreeSet;

use rusqlite::{Connection, params};

use crate::{code::source_roots::source_module_candidates, storage::StorageError};

pub(super) fn local_modules(
    connection: &Connection,
    source_scope: &str,
) -> Result<BTreeSet<String>, StorageError> {
    let mut statement = connection.prepare(
        "
        SELECT path
        FROM code_repository_files
        WHERE source_scope = ?1
          AND language_id = 'python'
        ",
    )?;
    let rows = statement.query_map(params![source_scope], |row| row.get::<_, String>(0))?;
    let mut modules = BTreeSet::new();
    for row in rows {
        let path = row?;
        for candidate in source_module_candidates(&path) {
            if let Some(module) = module_from_file_path(&candidate) {
                modules.insert(super::normalize_key(&module));
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
