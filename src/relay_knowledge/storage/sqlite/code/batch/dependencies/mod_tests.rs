//! Direct tests for dependency fact and search-document persistence.

use rusqlite::{Connection, params};

use super::insert_dependency_records;
use crate::domain::{CodeDependencyRecord, RepositoryCodeRange};

#[test]
fn dependency_records_publish_facts_and_search_metadata_in_one_transaction() {
    let mut connection = dependency_database();
    let transaction = connection.transaction().expect("transaction should open");

    insert_dependency_records(&transaction, &[dependency_record()])
        .expect("dependency should persist");
    transaction.commit().expect("transaction should commit");

    let persisted = connection
        .query_row(
            "SELECT package_name, requirement, resolved_version, is_lockfile
             FROM code_repository_dependencies
             WHERE source_scope = ?1 AND dependency_id = ?2",
            params!["scope", "cargo:serde"],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, Option<String>>(1)?,
                    row.get::<_, Option<String>>(2)?,
                    row.get::<_, bool>(3)?,
                ))
            },
        )
        .expect("dependency fact should load");
    assert_eq!(
        persisted,
        (
            "serde".to_owned(),
            Some("^1".to_owned()),
            Some("1.0.219".to_owned()),
            true,
        )
    );

    let search_content = connection
        .query_row(
            "SELECT content FROM code_repository_search
             WHERE source_scope = ?1 AND document_kind = 'dependency' AND record_id = ?2",
            params!["scope", "cargo:serde"],
            |row| row.get::<_, String>(0),
        )
        .expect("dependency search document should load");
    assert!(search_content.contains("serde"));
    assert!(search_content.contains("1.0.219"));
    assert_eq!(
        connection
            .query_row(
                "SELECT COUNT(*) FROM code_repository_search_metadata
                 WHERE source_scope = ?1 AND document_kind = 'dependency' AND record_id = ?2",
                params!["scope", "cargo:serde"],
                |row| row.get::<_, usize>(0),
            )
            .expect("search metadata should count"),
        1
    );
}

fn dependency_record() -> CodeDependencyRecord {
    CodeDependencyRecord {
        repository_id: "repo".to_owned(),
        source_scope: "scope".to_owned(),
        dependency_id: "cargo:serde".to_owned(),
        file_id: "cargo-toml".to_owned(),
        path: "Cargo.lock".to_owned(),
        language_id: "toml".to_owned(),
        ecosystem: "cargo".to_owned(),
        package_name: "serde".to_owned(),
        requirement: Some("^1".to_owned()),
        resolved_version: Some("1.0.219".to_owned()),
        dependency_group: "runtime".to_owned(),
        source_kind: "lockfile".to_owned(),
        is_lockfile: true,
        line_range: RepositoryCodeRange { start: 10, end: 12 },
        excerpt: "serde 1.0.219".to_owned(),
    }
}

fn dependency_database() -> Connection {
    let connection = Connection::open_in_memory().expect("database should open");
    connection
        .execute_batch(
            "
            CREATE TABLE code_repository_dependencies (
                repository_id TEXT NOT NULL,
                source_scope TEXT NOT NULL,
                dependency_id TEXT NOT NULL,
                file_id TEXT NOT NULL,
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
                line_end INTEGER NOT NULL,
                excerpt TEXT NOT NULL,
                PRIMARY KEY (source_scope, dependency_id)
            );
            CREATE VIRTUAL TABLE code_repository_search USING fts5(
                source_scope UNINDEXED,
                document_kind UNINDEXED,
                record_id UNINDEXED,
                path UNINDEXED,
                language_id UNINDEXED,
                content
            );
            CREATE TABLE code_repository_search_metadata (
                source_scope TEXT NOT NULL,
                document_kind TEXT NOT NULL,
                record_id TEXT NOT NULL,
                path TEXT NOT NULL,
                search_rowid INTEGER NOT NULL UNIQUE,
                PRIMARY KEY (source_scope, document_kind, record_id)
            );
            ",
        )
        .expect("dependency schema should be created");
    connection
}
