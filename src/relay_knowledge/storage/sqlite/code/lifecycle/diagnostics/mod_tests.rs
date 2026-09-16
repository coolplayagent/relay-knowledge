use super::*;
#[test]
fn diagnostic_pages_count_distinct_files_and_fail_for_retired_scopes() {
    let mut connection = diagnostic_connection();
    for (path, message) in [
        ("src/a.py", "a"),
        ("src/a.py", "b"),
        ("src/b.py", "c"),
        ("src-other/c.py", "d"),
    ] {
        connection
            .execute(
                "INSERT INTO code_repository_file_diagnostics VALUES ('r','s',?1,'partial',?2,NULL)",
                params![path, message],
            )
            .unwrap();
    }
    let mut request = CodeDiagnosticsPageRequest {
        repository_id: "r".into(),
        source_scope: "s".into(),
        resolved_commit_sha: "commit".into(),
        path_filters: vec!["src".into()],
        limit: 1,
        after: None,
    };
    let mut observed = Vec::new();
    loop {
        let result = page(&mut connection, request.clone()).unwrap();
        assert_eq!(result.degraded_file_count, 2);
        assert_eq!(
            result.scope_status.content_integrity.degraded_file_count,
            Some(3)
        );
        assert_eq!(
            result.scope_status.last_indexed_scope_id.as_deref(),
            Some("s")
        );
        observed.extend(result.diagnostics.iter().map(|d| d.message.clone()));
        if !result.has_more {
            break;
        }
        let last = result.diagnostics.last().unwrap();
        request.after = Some((last.path.clone(), last.message.clone()));
    }
    assert_eq!(observed, ["a", "b", "c"]);
    request.limit = 201;
    assert!(page(&mut connection, request.clone()).is_err());
    request.limit = 1;
    request.repository_id = "other".into();
    assert!(page(&mut connection, request.clone()).is_err());
    request.repository_id = "r".into();
    request.resolved_commit_sha = "other-commit".into();
    assert!(page(&mut connection, request.clone()).is_err());
    connection
        .execute(
            "INSERT INTO code_repository_commit_scopes VALUES ('r','s','other-commit')",
            [],
        )
        .unwrap();
    assert_eq!(
        page(&mut connection, request.clone())
            .unwrap()
            .scope_status
            .last_indexed_commit
            .as_deref(),
        Some("other-commit")
    );
    connection
        .execute("UPDATE code_repository_scopes SET retiring=1", [])
        .unwrap();
    assert!(page(&mut connection, request).is_err());
}

#[test]
fn source_io_child_filters_page_ancestor_directories_without_siblings_or_file_inflation() {
    let mut connection = diagnostic_connection();
    let directory_io = serde_json::json!({
        "action": "skipped", "path_kind": "directory", "operation": "read_directory",
        "error_kind": "permission_denied", "raw_os_error": 5,
    })
    .to_string();
    for (scope, path, message, io) in [
        (
            "s",
            "src",
            "enumeration denied",
            Some(directory_io.as_str()),
        ),
        ("s", "src", "unrelated file diagnostic", None),
        (
            "s",
            "src2",
            "unrelated directory",
            Some(directory_io.as_str()),
        ),
        ("s", "src/A.java", "file diagnostic", None),
        ("s", "src/B.java", "unrelated sibling", None),
        (
            "other-scope",
            "src",
            "other snapshot",
            Some(directory_io.as_str()),
        ),
    ] {
        connection
            .execute(
                "INSERT INTO code_repository_file_diagnostics VALUES ('r',?1,?2,'failed',?3,?4)",
                params![scope, path, message, io],
            )
            .unwrap();
    }
    let mut request = CodeDiagnosticsPageRequest {
        repository_id: "r".into(),
        source_scope: "s".into(),
        resolved_commit_sha: "commit".into(),
        path_filters: vec!["src/A.java".into()],
        limit: 1,
        after: None,
    };
    let first = page(&mut connection, request.clone()).unwrap();
    assert_eq!(first.degraded_file_count, 1);
    assert_eq!(first.diagnostics.len(), 1);
    assert_eq!(first.diagnostics[0].path, "src");
    assert_eq!(first.diagnostics[0].message, "enumeration denied");
    assert!(first.has_more);
    request.after = Some((
        first.diagnostics[0].path.clone(),
        first.diagnostics[0].message.clone(),
    ));
    let second = page(&mut connection, request.clone()).unwrap();
    assert_eq!(second.degraded_file_count, 1);
    assert_eq!(second.diagnostics.len(), 1);
    assert_eq!(second.diagnostics[0].path, "src/A.java");
    assert!(!second.has_more);

    request.after = None;
    request.path_filters = vec!["src/missing/nested.java".into()];
    let directory_only = page(&mut connection, request.clone()).unwrap();
    assert_eq!(directory_only.degraded_file_count, 0);
    assert_eq!(directory_only.diagnostics.len(), 1);
    assert_eq!(directory_only.diagnostics[0].path, "src");
    assert!(!directory_only.has_more);

    request.path_filters = vec!["src-other/A.java".into()];
    let unrelated = page(&mut connection, request).unwrap();
    assert_eq!(unrelated.degraded_file_count, 0);
    assert!(unrelated.diagnostics.is_empty());
    assert!(!unrelated.has_more);
}

fn diagnostic_connection() -> Connection {
    let connection = Connection::open_in_memory().unwrap();
    connection.execute_batch(
        "CREATE TABLE code_repositories(repository_id TEXT, alias TEXT, root_path TEXT);
         CREATE TABLE code_repository_scopes(
             source_scope TEXT, repository_id TEXT, retiring INTEGER, stale INTEGER,
             resolved_commit_sha TEXT, tree_hash TEXT, path_filters_json TEXT,
             language_filters_json TEXT, indexed_file_count INTEGER, symbol_count INTEGER,
             reference_count INTEGER, chunk_count INTEGER, degraded_reason TEXT);
         CREATE TABLE code_repository_commit_scopes(
             repository_id TEXT, source_scope TEXT, resolved_commit_sha TEXT);
         CREATE TABLE code_repository_file_diagnostics(
             repository_id TEXT, source_scope TEXT, path TEXT, parse_status TEXT, message TEXT, io_json TEXT);
         INSERT INTO code_repositories VALUES ('r','repo','/repo');
         INSERT INTO code_repository_scopes VALUES ('s','r',0,0,'commit','tree','[]','[]',3,0,0,0,NULL);"
    ).unwrap();
    connection
}
