use super::*;
use crate::domain::{CodeQueryKind, CodeRepositorySelector, FreshnessPolicy};

#[test]
fn canonical_call_query_work_budget() {
    let connection = Connection::open_in_memory().unwrap();
    crate::storage::sqlite::code::schema::initialize_code_schema(&connection).unwrap();
    crate::storage::sqlite::code::schema::ensure_code_query_indexes(&connection).unwrap();
    connection.execute("INSERT INTO code_repositories(repository_id,alias,root_path,path_filters_json,language_filters_json,state,indexed_file_count,symbol_count,reference_count,chunk_count,stale) VALUES('repo','repo','/repo','[]','[]','fresh',1,1,0,0,0)", []).unwrap();
    connection.execute_batch("INSERT INTO code_repository_files(repository_id,source_scope,file_id,path,language_id,blob_hash,byte_len,line_count,parse_status) VALUES('repo','scope','file','src/Many.java','java','blob',100000,40000,'parsed');
        INSERT INTO code_repository_symbols(repository_id,source_scope,symbol_snapshot_id,canonical_symbol_id,file_id,path,language_id,name,qualified_name,kind,signature,byte_start,byte_end,line_start,line_end) VALUES('repo','scope','symbol:target','repo://target','file','src/Many.java','java','target','target','method','void target() {}',0,10,1,40000);
        WITH RECURSIVE n(x) AS (SELECT 1 UNION ALL SELECT x+1 FROM n WHERE x<32768)
        INSERT INTO code_repository_calls(repository_id,source_scope,call_id,file_id,path,caller_symbol_snapshot_id,caller_name,callee_symbol_snapshot_id,callee_name,line_start,line_end)
        SELECT 'repo','scope','call:'||x,'file','src/Many.java','symbol:target','target','symbol:target','target',x,x FROM n;").unwrap();
    connection.execute_batch("WITH RECURSIVE n(x) AS (SELECT 1 UNION ALL SELECT x+1 FROM n WHERE x<2048)
        INSERT INTO code_repository_symbols(repository_id,source_scope,symbol_snapshot_id,canonical_symbol_id,file_id,path,language_id,name,qualified_name,kind,signature,byte_start,byte_end,line_start,line_end)
        SELECT 'repo','scope','symbol:noise:'||x,'repo://target','file','src/Many.java','java','target','target','field','int target;',0,10,x,x FROM n;").unwrap();
    let status = CodeRepositoryStatus {
        repository_id: "repo".into(),
        alias: "repo".into(),
        root_path: "/repo".into(),
        path_filters: vec![],
        language_filters: vec![],
        last_indexed_scope_id: Some("scope".into()),
        last_indexed_commit: Some("commit".into()),
        tree_hash: Some("tree".into()),
        state: "fresh".into(),
        indexed_file_count: 1,
        symbol_count: 1,
        reference_count: 0,
        chunk_count: 0,
        stale: false,
        degraded_reason: None,
    };
    for (kind, direction) in [
        (CodeQueryKind::Callers, "callers"),
        (CodeQueryKind::Callees, "callees"),
    ] {
        let request = CodeRetrievalRequest::new(
            "repo://target",
            CodeRepositorySelector::new("repo", "commit", vec![], vec![]).unwrap(),
            kind,
            10,
            FreshnessPolicy::AllowStale,
        )
        .unwrap();
        let identity = super::super::identity_query::call_identity_query(&request).unwrap();
        let result = search_call_identity_rows(&connection, &status, &request, &identity).unwrap();
        assert_eq!(result.rows.len(), 200);
        assert!(result.saturated);
        let steps = row_budget::LAST_STEPS.get();
        println!(
            "SELF_ITERATION_METRIC {{\"name\":\"canonical_call_{direction}_vm_steps\",\"value\":{steps},\"budget\":150000}}"
        );
        assert!(steps > 0 && steps < 150_000, "{direction}: {steps}");
        let column = identity.match_column();
        // The retained full-row SQL is also the pre-fix identity query. Measure
        // it on identical facts so the fast gate's budget has an auditable red control.
        let control_sql = call_rows_sql(&format!("AND {column} = ?"));
        row_budget::run(&connection, row_budget::MAX_PROGRESS_CALLBACKS, || {
            let mut statement = connection.prepare(&control_sql)?;
            let rows = statement.query_map(
                rusqlite::params!["scope", "symbol:target", 201],
                row_to_call,
            )?;
            rows.collect::<Result<Vec<_>, _>>()
                .map_err(StorageError::from)
        })
        .unwrap();
        let old_steps = row_budget::LAST_STEPS.get();
        println!("PRE_FIX_CONTROL {direction} vm_steps={old_steps}");
        assert!(
            old_steps > 150_000,
            "pre-fix control must fail the fast budget: {old_steps}"
        );
        let sql = ordered_call_rows_sql(
            &format!("AND {column} = ? AND f.is_generated = 0"),
            "c.path ASC, c.line_start ASC",
        );
        let plan = connection
            .prepare(&format!("EXPLAIN QUERY PLAN {sql}"))
            .unwrap()
            .query_map(rusqlite::params!["scope", "symbol:target", 201], |row| {
                Ok((row.get::<_, i64>(1)?, row.get::<_, String>(3)?))
            })
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert!(
            !plan
                .iter()
                .any(|(parent, line)| *parent == 0 && line.contains("TEMP B-TREE")),
            "{plan:?}"
        );
        connection
            .execute("UPDATE code_repository_files SET is_generated=7", [])
            .unwrap();
        let generated =
            search_call_identity_rows(&connection, &status, &request, &identity).unwrap();
        assert_eq!(generated.rows.len(), 200);
        assert!(generated.rows.iter().all(|row| row.is_generated));
        let mut excluded = request.clone();
        excluded.exclude_generated = true;
        assert!(
            search_call_identity_rows(&connection, &status, &excluded, &identity)
                .unwrap()
                .rows
                .is_empty()
        );
        connection
            .execute("UPDATE code_repository_files SET is_generated=0", [])
            .unwrap();
        let mut filtered = request.clone();
        filtered.query_path_substrings = vec!["absent-path".into()];
        let exhausted = row_budget::run(&connection, 0, || {
            search_call_identity_rows_with_budget(&connection, &status, &filtered, &identity)
        });
        assert!(
            matches!(exhausted, Err(StorageError::QueryBudgetExceeded(ref message)) if message.contains("call query incomplete"))
        );
    }
}
