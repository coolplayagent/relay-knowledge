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
