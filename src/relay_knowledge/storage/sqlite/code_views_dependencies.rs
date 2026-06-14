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
mod tests {
    use rusqlite::Connection;

    use crate::domain::{
        CodeRepositorySelector, CodebaseViewKind, CodebaseViewRequest, FreshnessPolicy,
    };

    use super::dependencies;

    #[test]
    fn dependency_snapshot_prioritizes_manifests_before_lockfiles() {
        let connection = Connection::open_in_memory().unwrap();
        connection
            .execute_batch(
                "
                CREATE TABLE code_repository_dependencies (
                    dependency_id TEXT NOT NULL,
                    source_scope TEXT NOT NULL,
                    path TEXT NOT NULL,
                    language_id TEXT NOT NULL,
                    ecosystem TEXT NOT NULL,
                    package_name TEXT NOT NULL,
                    requirement TEXT,
                    resolved_version TEXT,
                    dependency_group TEXT NOT NULL,
                    source_kind TEXT NOT NULL,
                    is_lockfile INTEGER NOT NULL,
                    line_start INTEGER NOT NULL,
                    line_end INTEGER NOT NULL
                );
                INSERT INTO code_repository_dependencies VALUES
                    ('dependency:lock', 'scope', 'Cargo.lock', 'rust', 'cargo', 'transitive', '1', '1.0.0', 'runtime', 'lockfile', 1, 1, 1),
                    ('dependency:manifest', 'scope', 'Cargo.toml', 'rust', 'cargo', 'direct', '^1', NULL, 'runtime', 'manifest', 0, 2, 2);
                ",
            )
            .unwrap();
        let request = CodebaseViewRequest::new(
            CodeRepositorySelector::new("repo", "HEAD", Vec::new(), Vec::new()).unwrap(),
            CodebaseViewKind::DependencyTour,
            FreshnessPolicy::AllowStale,
            10,
            Vec::new(),
        )
        .unwrap();

        let rows = dependencies(&connection, "scope", &request, 1).unwrap();

        assert_eq!(rows[0].package_name, "direct");
    }
}
