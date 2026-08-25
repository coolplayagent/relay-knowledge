//! Direct tests for software projection scope selection and SQL filter binding.

use rusqlite::{Connection, types::Value};

use super::*;
use crate::domain::{
    CodeRepositorySelector, FreshnessPolicy, SoftwareGlobalKind, SoftwareGlobalRequest,
};

#[test]
fn path_filters_normalize_roots_and_escape_sql_wildcards() {
    let filters = vec![
        "./crates/core/".to_owned(),
        "sdk_%".to_owned(),
        ".".to_owned(),
    ];

    assert_eq!(
        path_filter_sql_for_column("evidence_path", &filters),
        r"AND ((evidence_path = ? OR evidence_path LIKE ? ESCAPE '\') OR (evidence_path = ? OR evidence_path LIKE ? ESCAPE '\'))"
    );
    let mut values = Vec::new();
    push_path_filter_values(&mut values, &filters);
    assert_eq!(
        values,
        vec![
            Value::Text("crates/core".to_owned()),
            Value::Text("crates/core/%".to_owned()),
            Value::Text("sdk_%".to_owned()),
            Value::Text("sdk\\_\\%/%".to_owned()),
        ]
    );
}

#[test]
fn language_filters_keep_order_for_matching_bind_values() {
    let filters = vec!["rust".to_owned(), "python".to_owned()];

    assert_eq!(
        language_filter_sql_for_column("language_id", &filters),
        "AND (language_id = ? OR language_id = ?)"
    );
    let mut values = vec![Value::Text("scope-1".to_owned())];
    push_language_filter_values(&mut values, &filters);
    assert_eq!(
        values,
        vec![
            Value::Text("scope-1".to_owned()),
            Value::Text("rust".to_owned()),
            Value::Text("python".to_owned()),
        ]
    );
}

#[test]
fn exact_scope_selection_prefers_repository_identity_over_alias() {
    let mut connection = scope_connection();
    connection
        .execute(
            "INSERT INTO code_repositories (repository_id) VALUES ('repo-id')",
            [],
        )
        .expect("repository should insert");
    connection
        .execute(
            "INSERT INTO code_repository_aliases (alias, repository_id)
             VALUES ('repo-id', 'alias-target')",
            [],
        )
        .expect("alias should insert");
    connection
        .execute(
            "INSERT INTO code_repository_scopes
             (repository_id, resolved_commit_sha, source_scope, path_filters_json,
              language_filters_json)
             VALUES ('repo-id', 'commit-1', 'identity-scope', '[]', '[]'),
                    ('alias-target', 'commit-1', 'alias-scope', '[]', '[]')",
            [],
        )
        .expect("scopes should insert");

    let request = request("repo-id", Vec::new(), Vec::new());
    assert_eq!(
        source_scope_for_request(&mut connection, &request).expect("scope should resolve"),
        "identity-scope"
    );
}

#[test]
fn broader_indexed_scope_can_serve_narrow_projection_filters() {
    let mut connection = scope_connection();
    connection
        .execute(
            "INSERT INTO code_repositories (repository_id) VALUES ('repo-id')",
            [],
        )
        .expect("repository should insert");
    connection
        .execute(
            "INSERT INTO code_repository_scopes
             (repository_id, resolved_commit_sha, source_scope, path_filters_json,
              language_filters_json)
             VALUES ('repo-id', 'commit-1', 'broad-scope', '[]', '[]')",
            [],
        )
        .expect("scope should insert");

    let request = request(
        "repo-id",
        vec!["crates/core".to_owned()],
        vec!["rust".to_owned()],
    );
    assert_eq!(
        source_scope_for_request(&mut connection, &request).expect("scope should resolve"),
        "broad-scope"
    );
}

fn request(
    repository: &str,
    path_filters: Vec<String>,
    language_filters: Vec<String>,
) -> SoftwareGlobalRequest {
    SoftwareGlobalRequest::new(
        CodeRepositorySelector::new(repository, "commit-1", path_filters, language_filters)
            .expect("selector should validate"),
        SoftwareGlobalKind::All,
        FreshnessPolicy::AllowStale,
        10,
    )
    .expect("request should validate")
}

fn scope_connection() -> Connection {
    let connection = Connection::open_in_memory().expect("sqlite should open");
    connection
        .execute_batch(
            "
            CREATE TABLE code_repositories (
                repository_id TEXT PRIMARY KEY
            );
            CREATE TABLE code_repository_aliases (
                alias TEXT PRIMARY KEY,
                repository_id TEXT NOT NULL
            );
            CREATE TABLE code_repository_scopes (
                repository_id TEXT NOT NULL,
                resolved_commit_sha TEXT NOT NULL,
                source_scope TEXT PRIMARY KEY,
                path_filters_json TEXT NOT NULL,
                language_filters_json TEXT NOT NULL,
                stale INTEGER NOT NULL DEFAULT 0,
                retiring INTEGER NOT NULL DEFAULT 0
            );
            ",
        )
        .expect("scope schema should initialize");
    connection
}
