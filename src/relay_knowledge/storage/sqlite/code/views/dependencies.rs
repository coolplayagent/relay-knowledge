use rusqlite::{Connection, params_from_iter};

use crate::{
    domain::{CodebaseViewDependency, CodebaseViewRequest, RepositoryCodeRange},
    storage::StorageError,
};

use super::{FilterColumns, collect_rows, filtered_sql};

pub(super) fn dependencies(
    connection: &Connection,
    source_scope: &str,
    request: &CodebaseViewRequest,
    limit: usize,
) -> Result<Vec<CodebaseViewDependency>, StorageError> {
    let (sql, values) = filtered_sql(
        "
        SELECT dependency_id, path, language_id, ecosystem, package_name, requirement,
               resolved_version, dependency_group, source_kind, line_start, line_end
        FROM code_repository_dependencies
        WHERE source_scope = ?1
        ",
        source_scope,
        request,
        FilterColumns::new("path", Some("language_id")),
        |_, _| {},
        "
        ORDER BY is_lockfile ASC, path ASC, package_name ASC, dependency_id ASC
        ",
        limit,
    );
    let mut statement = connection.prepare(&sql)?;
    let rows = statement.query_map(params_from_iter(values.iter()), |row| {
        Ok(CodebaseViewDependency {
            dependency_id: row.get(0)?,
            path: row.get(1)?,
            language_id: row.get(2)?,
            ecosystem: row.get(3)?,
            package_name: row.get(4)?,
            requirement: row.get(5)?,
            resolved_version: row.get(6)?,
            dependency_group: row.get(7)?,
            source_kind: row.get(8)?,
            line_range: RepositoryCodeRange {
                start: row.get(9)?,
                end: row.get(10)?,
            },
        })
    })?;

    collect_rows(rows)
}

#[cfg(test)]
#[path = "dependencies_tests.rs"]
mod tests;
