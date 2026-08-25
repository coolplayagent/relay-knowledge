//! Path-filter SQL range and bind-order contracts.

use rusqlite::{Connection, params_from_iter};

use super::*;

#[test]
fn path_filters_use_a_boundary_checked_binary_range() {
    let filters = vec!["./src/provider.ts".to_owned()];
    let mut clauses = Vec::new();
    let mut values = Vec::new();

    push_path_filter_sql(&mut clauses, "i.path", &filters);
    push_path_filter_values(&mut values, &filters);

    assert_eq!(
        clauses,
        ["((i.path >= ? AND i.path < ? AND substr(i.path, length(?) + 1, 1) IN ('', '/')))"],
    );
    assert_eq!(
        values,
        [
            Value::Text("src/provider.ts".to_owned()),
            Value::Text("src/provider.ts0".to_owned()),
            Value::Text("src/provider.ts".to_owned()),
        ]
    );
}

#[test]
fn dot_directory_filters_keep_exact_and_descendant_paths() {
    let connection = Connection::open_in_memory().expect("database should open");
    connection
        .execute_batch(
            "
            CREATE TABLE paths (path TEXT NOT NULL);
            INSERT INTO paths VALUES
                ('.github'),
                ('.github/workflows/ci.yml'),
                ('.github-actions/wrong.yml'),
                ('.config/settings.toml'),
                ('.configuration/wrong.toml'),
                ('vendor/v1.2'),
                ('vendor/v1.2/src/lib.rs'),
                ('vendor/v1.20/wrong.rs');
            ",
        )
        .expect("path fixture should initialize");

    for (filter, expected) in [
        (".github", vec![".github", ".github/workflows/ci.yml"]),
        (".config/", vec![".config/settings.toml"]),
        (
            "vendor/v1.2/",
            vec!["vendor/v1.2", "vendor/v1.2/src/lib.rs"],
        ),
    ] {
        let mut clauses = Vec::new();
        let mut values = Vec::new();
        push_path_filter_sql(&mut clauses, "path", &[filter.to_owned()]);
        push_path_filter_values(&mut values, &[filter.to_owned()]);
        let sql = format!(
            "SELECT path FROM paths WHERE {} ORDER BY path ASC",
            clauses.join(" AND ")
        );
        let mut statement = connection.prepare(&sql).expect("path query should prepare");
        let paths = statement
            .query_map(params_from_iter(values), |row| row.get::<_, String>(0))
            .expect("path query should execute")
            .collect::<Result<Vec<_>, _>>()
            .expect("path rows should collect");
        let paths = paths.iter().map(String::as_str).collect::<Vec<_>>();

        assert_eq!(paths, expected, "filter={filter}");
    }
}

#[test]
fn scoped_path_filter_plan_seeks_the_scope_and_path_range() {
    let connection = Connection::open_in_memory().expect("database should open");
    connection
        .execute_batch(
            "
            CREATE TABLE scoped_paths (
                source_scope TEXT NOT NULL,
                path TEXT NOT NULL
            );
            CREATE INDEX scoped_paths_lookup ON scoped_paths(source_scope, path);
            ",
        )
        .expect("path-plan fixture should initialize");
    let mut clauses = Vec::new();
    let mut values = vec![Value::Text("scope".to_owned())];
    push_path_filter_sql(&mut clauses, "path", &[".github".to_owned()]);
    push_path_filter_values(&mut values, &[".github".to_owned()]);
    let sql = format!(
        "EXPLAIN QUERY PLAN SELECT path FROM scoped_paths \
         WHERE source_scope = ? AND {}",
        clauses.join(" AND ")
    );
    let plan = connection
        .prepare(&sql)
        .expect("path-range plan should prepare")
        .query_map(params_from_iter(values), |row| row.get::<_, String>(3))
        .expect("path-range plan should execute")
        .collect::<Result<Vec<_>, _>>()
        .expect("path-range plan should collect");

    assert!(
        plan.iter().any(|detail| {
            detail.contains("scoped_paths_lookup")
                && detail.contains("source_scope=?")
                && detail.contains("path>?")
                && detail.contains("path<?")
        }),
        "{plan:?}"
    );
}
