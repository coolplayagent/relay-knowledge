//! SQL fixtures for class ownership and directional selection boundaries.
use crate::domain::{
    CodeQueryKind, CodeRepositorySelector, CodeRepositoryStatus, CodeRetrievalRequest,
    FreshnessPolicy,
};
use rusqlite::{Connection, params};

pub(super) fn database() -> Connection {
    let connection = Connection::open_in_memory().unwrap();
    crate::storage::sqlite::schema::initialization::initialize_schema(&connection).unwrap();
    // These unit fixtures exercise read SQL directly; service integration tests
    // establish the catalog/scope through the normal durable publication path.
    connection.execute_batch("PRAGMA foreign_keys=OFF;
        CREATE INDEX IF NOT EXISTS code_repository_symbols_name_path_lookup ON code_repository_symbols(source_scope,name,path);
        CREATE INDEX IF NOT EXISTS code_repository_symbols_path_line_lookup ON code_repository_symbols(source_scope,path,line_end,line_start);
        CREATE INDEX IF NOT EXISTS code_repository_calls_lookup ON code_repository_calls(source_scope,callee_name,caller_name,path);
        CREATE INDEX IF NOT EXISTS code_repository_calls_caller_lookup ON code_repository_calls(source_scope,caller_name,path,line_start);
        INSERT INTO code_repository_files (repository_id,source_scope,file_id,path,language_id,blob_hash,byte_len,line_count,parse_status,is_generated)
        VALUES ('repo','scope','target-file','src/Target.java','java','hash',1000,100,'parsed',0),
               ('repo','scope','caller-file','src/Caller.java','java','hash',1000,100,'parsed',0);").unwrap();
    for (id, name, owner, kind, start, end) in [
        ("target", "Target", "demo.Target", "class", 0, 900),
        ("method", "execute", "demo.Target.execute", "method", 10, 30),
        (
            "overload",
            "execute",
            "demo.Target.execute",
            "method",
            40,
            60,
        ),
        (
            "constructor",
            "Target",
            "demo.Target.Target",
            "constructor",
            70,
            90,
        ),
        ("nested", "Nested", "demo.Target.Nested", "class", 100, 200),
        (
            "nested-method",
            "execute",
            "demo.Target.Nested.execute",
            "method",
            110,
            130,
        ),
        ("field", "execute", "demo.Target.execute", "field", 210, 215),
        ("sibling", "Other", "demo.Other", "class", 901, 999),
        (
            "sibling-method",
            "execute",
            "demo.Other.execute",
            "method",
            910,
            930,
        ),
    ] {
        connection.execute("INSERT INTO code_repository_symbols
            (repository_id,source_scope,symbol_snapshot_id,canonical_symbol_id,file_id,path,language_id,name,qualified_name,kind,signature,byte_start,byte_end,line_start,line_end)
            VALUES ('repo','scope',?1,?2,'target-file','src/Target.java','java',?3,?2,?4,'',?5,?6,?5,?6)",
            params![id,owner,name,kind,start,end]).unwrap();
    }
    connection.execute_batch("INSERT INTO code_repository_symbols
        (repository_id,source_scope,symbol_snapshot_id,canonical_symbol_id,file_id,path,language_id,name,qualified_name,kind,signature,byte_start,byte_end,line_start,line_end)
        VALUES ('repo','scope','caller','demo.Caller.run','caller-file','src/Caller.java','java','run','demo.Caller.run','method','',10,30,1,3);").unwrap();
    for (id, caller, caller_name, callee, callee_name, path) in [
        (
            "incoming",
            "caller",
            "run",
            Some("method"),
            "execute",
            "src/Caller.java",
        ),
        (
            "overloaded",
            "caller",
            "run",
            Some("overload"),
            "execute",
            "src/Caller.java",
        ),
        (
            "nested-call",
            "caller",
            "run",
            Some("nested-method"),
            "execute",
            "src/Caller.java",
        ),
        (
            "sibling-call",
            "caller",
            "run",
            Some("sibling-method"),
            "execute",
            "src/Caller.java",
        ),
        (
            "unknown",
            "caller",
            "run",
            None,
            "execute",
            "src/Caller.java",
        ),
        (
            "outgoing",
            "method",
            "execute",
            None,
            "println",
            "src/Target.java",
        ),
        (
            "initializer",
            "target",
            "Target",
            None,
            "initialize",
            "src/Target.java",
        ),
    ] {
        connection.execute("INSERT INTO code_repository_calls
            (repository_id,source_scope,call_id,file_id,path,caller_symbol_snapshot_id,caller_name,callee_symbol_snapshot_id,callee_name,target_hint,resolution_state,confidence_basis_points,confidence_tier,line_start,line_end)
            VALUES ('repo','scope',?1,'file',?2,?3,?4,?5,?6,?6,CASE WHEN ?5 IS NULL THEN 'unresolved' ELSE 'resolved' END,8000,'inferred',2,2)",
            params![id,path,caller,caller_name,callee,callee_name]).unwrap();
    }
    connection
}

pub(super) fn status() -> CodeRepositoryStatus {
    CodeRepositoryStatus {
        repository_id: "repo".into(),
        alias: "repo".into(),
        root_path: "/repo".into(),
        path_filters: vec![],
        language_filters: vec![],
        last_indexed_scope_id: Some("scope".into()),
        last_indexed_commit: Some("commit".into()),
        tree_hash: Some("tree".into()),
        state: "fresh".into(),
        indexed_file_count: 2,
        symbol_count: 10,
        reference_count: 0,
        chunk_count: 0,
        stale: false,
        degraded_reason: None,
    }
}

pub(super) fn request(query: &str, kind: CodeQueryKind) -> CodeRetrievalRequest {
    CodeRetrievalRequest::new(
        query,
        CodeRepositorySelector::new("repo", "commit", vec![], vec![]).unwrap(),
        kind,
        10,
        FreshnessPolicy::AllowStale,
    )
    .unwrap()
}
