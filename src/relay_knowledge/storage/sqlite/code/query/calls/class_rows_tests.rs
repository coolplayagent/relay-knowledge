use super::super::class_test_support::{database, request, status};
use super::*;

#[test]
fn class_aggregation_does_not_intercept_hybrid_or_qualified_queries() {
    let connection = database();
    for (name, kind) in [
        ("Target", CodeQueryKind::Hybrid),
        ("Target", CodeQueryKind::Definition),
        ("demo.Target", CodeQueryKind::Callers),
        ("demo.Target", CodeQueryKind::Callees),
        ("Target.Nested", CodeQueryKind::Callers),
    ] {
        assert!(
            search(&connection, &status(), &request(name, kind))
                .unwrap()
                .is_none(),
            "must preserve the existing path for {name} / {kind:?}"
        );
    }
}

#[test]
fn aggregates_only_directional_structured_member_edges() {
    let connection = database();
    let callers = search(
        &connection,
        &status(),
        &request("Target", CodeQueryKind::Callers),
    )
    .unwrap()
    .unwrap();
    assert_eq!(callers.len(), 2);
    assert!(
        callers
            .iter()
            .all(|r| r.caller_name.as_deref() == Some("run"))
    );
    assert!(callers.iter().all(|r| matches!(
        r.callee_symbol_snapshot_id.as_deref(),
        Some("method" | "overload")
    )));
    let callees = search(
        &connection,
        &status(),
        &request("Target", CodeQueryKind::Callees),
    )
    .unwrap()
    .unwrap();
    assert_eq!(callees.len(), 2);
    assert!(callees.iter().all(|r| r.resolution_state == "unresolved"));
}

#[test]
fn class_empty_answers_do_not_fall_back_to_text_or_reverse_edges() {
    let connection = database();
    connection
        .execute(
            "DELETE FROM code_repository_calls WHERE call_id IN ('incoming','overloaded')",
            [],
        )
        .unwrap();
    let hits = super::super::search::search_calls(
        &connection,
        &status(),
        &request("Target", CodeQueryKind::Callers),
    )
    .unwrap();
    assert!(hits.is_empty());
    for query in [
        "execute",
        "Target.execute",
        "unrelated",
        "describe Target",
        "not/a/name",
    ] {
        assert!(
            search(
                &connection,
                &status(),
                &request(query, CodeQueryKind::Callers)
            )
            .unwrap()
            .is_none()
        );
    }
}

#[test]
fn exhausted_work_is_an_explicit_error_and_does_not_poison_the_connection() {
    let connection = Connection::open_in_memory().unwrap();
    connection
        .execute_batch(
            "CREATE VIEW code_repository_symbols AS
        WITH RECURSIVE seq(n) AS (VALUES(0) UNION ALL SELECT n+1 FROM seq WHERE n<999)
        SELECT 'scope' source_scope, printf('N%d',a.n+b.n+c.n) name,
               'class' kind, 'java' language_id, 'id' symbol_snapshot_id,
               'owner' qualified_name, 'file' path, 0 byte_start, 1 byte_end
        FROM seq a CROSS JOIN seq b CROSS JOIN seq c;",
        )
        .unwrap();
    let error = search(
        &connection,
        &status(),
        &request("Target", CodeQueryKind::Callers),
    )
    .err()
    .unwrap();
    assert!(matches!(error, StorageError::CapacityExceeded(_)));
    assert!(
        error
            .to_string()
            .contains("SQLite execution budget exhausted")
    );
    let total: i64 = connection.query_row("WITH RECURSIVE seq(n) AS (VALUES(0) UNION ALL SELECT n+1 FROM seq WHERE n<1000) SELECT sum(n) FROM seq", [], |row| row.get(0)).unwrap();
    assert_eq!(total, 500500);
}

#[test]
fn filters_apply_to_call_sites_before_the_candidate_limit() {
    let connection = database();
    let mut query = request("Target", CodeQueryKind::Callers);
    query.repository.path_filters = vec!["src/Caller.java".into()];
    query.query_path_substrings = vec!["Caller".into()];
    query.query_name_substrings = vec!["Caller.run".into()];
    query.repository.language_filters = vec!["java".into()];
    assert_eq!(
        search(&connection, &status(), &query)
            .unwrap()
            .unwrap()
            .len(),
        2
    );
    query.query_name_substrings = vec!["Target".into()];
    assert!(
        search(&connection, &status(), &query)
            .unwrap()
            .unwrap()
            .is_empty()
    );
    query.query_name_substrings.clear();
    query.repository.language_filters = vec!["python".into()];
    assert!(
        search(&connection, &status(), &query)
            .unwrap()
            .unwrap()
            .is_empty()
    );
    query.repository.language_filters.clear();
    query.repository.path_filters = vec!["src/Target.java".into()];
    assert!(
        search(&connection, &status(), &query)
            .unwrap()
            .unwrap()
            .is_empty()
    );
}

#[test]
fn excludes_generated_call_sites_and_bounds_wide_class_results() {
    let connection = database();
    connection
        .execute(
            "UPDATE code_repository_files SET is_generated=1 WHERE path='src/Caller.java'",
            [],
        )
        .unwrap();
    let mut query = request("Target", CodeQueryKind::Callers);
    query.exclude_generated = true;
    assert!(
        search(&connection, &status(), &query)
            .unwrap()
            .unwrap()
            .is_empty()
    );
    query.exclude_generated = false;
    for i in 0..512 {
        connection.execute("INSERT INTO code_repository_calls SELECT repository_id,source_scope,?1,file_id,path,caller_symbol_snapshot_id,caller_name,callee_symbol_snapshot_id,callee_name,target_hint,resolution_state,confidence_basis_points,confidence_tier,line_start,line_end FROM code_repository_calls WHERE call_id='incoming'", [format!("call-{i}")]).unwrap();
    }
    assert_eq!(
        search(&connection, &status(), &query)
            .unwrap()
            .unwrap()
            .len(),
        call_identity_candidate_limit(&query)
    );
}

#[test]
fn class_evidence_survives_projection_and_missing_scope_is_an_error() {
    let connection = database();
    let hits = super::super::search::search_calls(
        &connection,
        &status(),
        &request("Target", CodeQueryKind::Callers),
    )
    .unwrap();
    assert!(!hits.is_empty());
    assert!(
        hits.iter()
            .all(|hit| hit.canonical_symbol_id.as_deref() == Some("demo.Caller.run"))
    );
    let mut missing = status();
    missing.last_indexed_scope_id = None;
    assert!(
        search(
            &connection,
            &missing,
            &request("Target", CodeQueryKind::Callers)
        )
        .is_err()
    );
    assert_eq!(
        connection
            .query_row("SELECT 1", [], |row| row.get::<_, i64>(0))
            .unwrap(),
        1
    );
}
