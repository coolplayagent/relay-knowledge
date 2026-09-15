use super::*;
#[test]
fn diagnostic_pages_count_distinct_files_and_fail_for_retired_scopes() {
    let mut connection = Connection::open_in_memory().unwrap();
    connection.execute_batch("CREATE TABLE code_repository_scopes(source_scope TEXT, repository_id TEXT, retiring INTEGER, stale INTEGER); CREATE TABLE code_repository_file_diagnostics(repository_id TEXT, source_scope TEXT, path TEXT, parse_status TEXT, message TEXT); INSERT INTO code_repository_scopes VALUES ('s','r',0,0);").unwrap();
    for (path, message) in [
        ("src/a.py", "a"),
        ("src/a.py", "b"),
        ("src/b.py", "c"),
        ("src-other/c.py", "d"),
    ] {
        connection
            .execute(
                "INSERT INTO code_repository_file_diagnostics VALUES ('r','s',?1,'partial',?2)",
                params![path, message],
            )
            .unwrap();
    }
    assert_eq!(
        content_integrity(&connection, Some("s".into()))
            .unwrap()
            .degraded_file_count,
        Some(3)
    );
    let mut request = CodeDiagnosticsPageRequest {
        repository_id: "r".into(),
        source_scope: "s".into(),
        path_filters: vec!["src".into()],
        limit: 1,
        after: None,
    };
    let mut observed = Vec::new();
    loop {
        let result = page(&mut connection, request.clone()).unwrap();
        assert_eq!(result.degraded_file_count, 2);
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
    connection
        .execute("UPDATE code_repository_scopes SET retiring=1", [])
        .unwrap();
    assert!(page(&mut connection, request).is_err());
}
