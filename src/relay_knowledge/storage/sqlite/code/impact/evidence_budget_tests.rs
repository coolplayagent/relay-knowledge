//! Impact evidence must retain late matches without exceeding SQLite bind limits.

use std::collections::BTreeSet;

use rusqlite::{Connection, limits::Limit, params};

use super::evidence_test_support::{callers_for_symbols, chunks_for_paths, importers_for_modules};
use crate::domain::{CodeImpactRequest, CodeRepositorySelector, CodeRepositoryStatus};

#[test]
fn caller_evidence_batches_ids_and_deleted_names_without_losing_set_semantics() {
    let connection = database();
    let ids = (0..1_101)
        .map(|index| format!("symbol-{index:04}"))
        .collect::<Vec<_>>();
    let names = (0..1_101)
        .map(|index| format!("deleted-{index:04}'quoted"))
        .collect::<Vec<_>>();
    for index in [0, 500, 1_100] {
        connection
            .execute(
                "INSERT INTO code_repository_calls VALUES (
                'scope', 'file', 'src/lib.rs', NULL, 'caller', ?1, 'target',
                ?2, ?2, 'target', 'resolved', 10000, 'extracted')",
                params![ids[index], index + 1],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO code_repository_calls VALUES (
                'scope', 'file', 'src/lib.rs', NULL, 'caller', NULL, ?1,
                ?2, ?2, 'removed', 'unresolved', 5000, 'heuristic')",
                params![names[index], index + 2],
            )
            .unwrap();
    }
    // A matching deleted name must never match a call resolved to another symbol.
    connection
        .execute(
            "INSERT INTO code_repository_calls VALUES (
            'scope', 'file', 'src/lib.rs', NULL, 'caller', 'unrelated', ?1,
            2000, 2000, NULL, 'resolved', 10000, 'extracted')",
            [&names[0]],
        )
        .unwrap();
    connection.execute_batch(
        "INSERT INTO code_repository_calls SELECT
         'other',file_id,path,caller_symbol_snapshot_id,caller_name,
         callee_symbol_snapshot_id,callee_name,line_start,line_end,target_hint,
         resolution_state,confidence_basis_points,confidence_tier
         FROM code_repository_calls;
         INSERT INTO code_repository_files (source_scope,path,language_id,is_generated) VALUES ('other','src/lib.rs','rust',0);",
    ).unwrap();

    let mut duplicate_ids = ids.clone();
    duplicate_ids.extend(ids.iter().cloned());
    let mut duplicate_names = names.clone();
    duplicate_names.extend(names.iter().cloned());
    for (selected_ids, selected_names, expected) in [
        (ids.as_slice(), &[][..], 3),
        (&[][..], names.as_slice(), 3),
        (duplicate_ids.as_slice(), duplicate_names.as_slice(), 6),
    ] {
        let hits = callers_for_symbols(
            &connection,
            &status(),
            selected_ids,
            selected_names,
            &request(),
        )
        .expect("all seed batches must fit the connection bind budget");
        assert_eq!(hits.len(), expected);
        assert!(hits.iter().all(|hit| hit.scope_id == "scope"));
        assert!(
            hits.windows(2)
                .all(|pair| pair[0].line_range.start <= pair[1].line_range.start)
        );
        assert!(hits.iter().any(|hit| hit.line_range.start >= 1_101));
        assert!(
            hits.iter()
                .all(|hit| hit.edge_kind.as_deref() == Some("call"))
        );
    }
    let mut filtered = request();
    filtered.repository.language_filters = vec!["python".into()];
    assert!(
        callers_for_symbols(&connection, &status(), &ids, &names, &filtered)
            .unwrap()
            .is_empty()
    );
    filtered.repository.language_filters.clear();
    filtered.repository.path_filters = vec!["unrelated".into()];
    assert!(
        callers_for_symbols(&connection, &status(), &ids, &names, &filtered)
            .unwrap()
            .is_empty()
    );
}

#[test]
fn chunk_evidence_batches_paths_and_preserves_order_scope_and_filters() {
    let connection = database();
    let paths = (0..1_101)
        .map(|index| format!("src/file-{index:04}.rs"))
        .collect::<BTreeSet<_>>();
    for index in [0, 500, 1_100] {
        let path = format!("src/file-{index:04}.rs");
        connection.execute(
            "INSERT INTO code_repository_files (source_scope,path,language_id,is_generated) VALUES ('scope', ?1, 'rust', 0)",
            [&path],
        ).unwrap();
        connection
            .execute(
                "INSERT INTO code_repository_chunks VALUES (
                'scope','file',?1,'rust','fn example() {}',0,15,1,1,NULL)",
                [&path],
            )
            .unwrap();
    }
    connection.execute_batch(
        "INSERT INTO code_repository_files (source_scope,path,language_id,is_generated) SELECT 'other',path,language_id,is_generated
         FROM code_repository_files;
         INSERT INTO code_repository_chunks SELECT 'other',file_id,path,language_id,
         content,byte_start,byte_end,line_start,line_end,symbol_snapshot_id
         FROM code_repository_chunks;",
    ).unwrap();
    let hits = chunks_for_paths(&connection, &status(), &paths, &request())
        .expect("late path batches must remain queryable");
    assert_eq!(
        hits.iter().map(|hit| hit.path.as_str()).collect::<Vec<_>>(),
        ["src/file-0000.rs", "src/file-0500.rs", "src/file-1100.rs"]
    );
    assert!(hits.iter().all(|hit| hit.scope_id == "scope"));
    let mut filtered = request();
    filtered.repository.path_filters = vec!["src/file-1100.rs".into()];
    assert_eq!(
        chunks_for_paths(&connection, &status(), &paths, &filtered)
            .unwrap()
            .len(),
        1
    );
    filtered.repository.language_filters = vec!["python".into()];
    assert!(
        chunks_for_paths(&connection, &status(), &paths, &filtered)
            .unwrap()
            .is_empty()
    );
}

#[test]
fn dense_evidence_keeps_late_best_matches_with_a_one_result_budget() {
    let connection = database();
    connection
        .execute_batch(
            "INSERT INTO code_repository_files (source_scope,path,language_id,is_generated)
         VALUES ('scope','src/a.rs','rust',0);
         CREATE TABLE code_repository_imports (
            source_scope TEXT,file_id TEXT,path TEXT,module TEXT,line_start INTEGER,
            line_end INTEGER,target_hint TEXT,resolution_state TEXT,
            confidence_basis_points INTEGER,confidence_tier TEXT);",
        )
        .unwrap();
    let seeds = (0..1_101)
        .map(|index| format!("seed-{index:04}"))
        .collect::<Vec<_>>();
    for (index, seed) in seeds.iter().enumerate() {
        let path = if index == 1_100 {
            "src/a.rs"
        } else {
            "src/lib.rs"
        };
        connection
            .execute(
                "INSERT INTO code_repository_calls VALUES (
                'scope','file',?1,NULL,'caller',?2,'target',?3,?3,
                'target','resolved',10000,'extracted')",
                params![path, seed, index],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO code_repository_imports VALUES (
                'scope','file',?1,?2,?3,?3,NULL,'resolved',10000,'extracted')",
                params![path, seed, index],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO code_repository_chunks VALUES (
                'scope','file','src/lib.rs','rust',?1,0,15,?2,?2,NULL)",
                params![format!("fn example_{index}() {{}}"), index],
            )
            .unwrap();
    }
    let mut bounded = request();
    bounded.limit = 1;
    let callers = callers_for_symbols(&connection, &status(), &seeds, &[], &bounded).unwrap();
    assert_eq!(callers.len(), 1);
    assert_eq!(callers[0].path, "src/a.rs");
    let importers = importers_for_modules(&connection, &status(), &seeds, &bounded).unwrap();
    assert_eq!(importers.len(), 1);
    assert_eq!(importers[0].path, "src/a.rs");
    let chunks = chunks_for_paths(
        &connection,
        &status(),
        &BTreeSet::from(["src/lib.rs".into()]),
        &bounded,
    )
    .unwrap();
    assert_eq!(chunks.len(), 1);
    assert_eq!(chunks[0].line_range.start, 0);
}

fn database() -> Connection {
    let connection = Connection::open_in_memory().unwrap();
    connection
        .execute_batch(
            "CREATE TABLE code_repository_files (
            source_scope TEXT,path TEXT,language_id TEXT,is_generated INTEGER,
            parse_status TEXT DEFAULT 'parsed',degraded_reason TEXT);
         CREATE TABLE code_repository_symbols (
            source_scope TEXT,symbol_snapshot_id TEXT,canonical_symbol_id TEXT);
         CREATE TABLE code_repository_calls (
            source_scope TEXT,file_id TEXT,path TEXT,caller_symbol_snapshot_id TEXT,
            caller_name TEXT,callee_symbol_snapshot_id TEXT,callee_name TEXT,
            line_start INTEGER,line_end INTEGER,target_hint TEXT,resolution_state TEXT,
            confidence_basis_points INTEGER,confidence_tier TEXT);
         CREATE TABLE code_repository_chunks (
            source_scope TEXT,file_id TEXT,path TEXT,language_id TEXT,content TEXT,
            byte_start INTEGER,byte_end INTEGER,line_start INTEGER,line_end INTEGER,
            symbol_snapshot_id TEXT);
         INSERT INTO code_repository_files (source_scope,path,language_id,is_generated)
         VALUES ('scope','src/lib.rs','rust',0);",
        )
        .unwrap();
    connection.set_limit(Limit::SQLITE_LIMIT_VARIABLE_NUMBER, 999);
    connection
}

fn status() -> CodeRepositoryStatus {
    CodeRepositoryStatus {
        content_integrity: Default::default(),
        repository_id: "repo".into(),
        alias: "repo".into(),
        root_path: "/repo".into(),
        path_filters: Vec::new(),
        language_filters: Vec::new(),
        last_indexed_scope_id: Some("scope".into()),
        last_indexed_commit: Some("commit".into()),
        tree_hash: Some("tree".into()),
        state: "fresh".into(),
        indexed_file_count: 0,
        symbol_count: 0,
        reference_count: 0,
        chunk_count: 0,
        stale: false,
        degraded_reason: None,
    }
}

fn request() -> CodeImpactRequest {
    CodeImpactRequest::new(
        CodeRepositorySelector::new("repo", "commit", Vec::new(), Vec::new()).unwrap(),
        "base",
        "head",
        10,
    )
    .unwrap()
}
