use super::*;
use rusqlite::params;

fn file(connection: &Connection, scope: &str, path: &str, evidence: Option<&str>) {
    connection.execute("INSERT INTO code_repository_files(repository_id,source_scope,file_id,path,language_id,blob_hash,byte_len,line_count,parse_status,java_namespace_json)
        VALUES('repo',?1,?2,?2,'java','blob',1,1,'parsed',?3)", params![scope,path,evidence]).unwrap();
}

#[test]
fn java_namespace_projection_tracks_copy_update_delete_and_legacy_unknown() {
    let store = crate::storage::SqliteGraphStore::open_in_memory().unwrap();
    let connection = store.connection.lock().unwrap();
    connection.execute_batch("PRAGMA foreign_keys=OFF").unwrap();
    let evidence = r#"{"package":"demo","top_level_types":["System","Other"],"complete":true,"source_set":{"kind":"repository"}}"#;
    file(&connection, "old", "Odd.java", Some(evidence));
    assert_eq!(
        connection
            .query_row(
                "SELECT count(*) FROM code_repository_java_types WHERE source_scope='old'",
                [],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
        2
    );
    connection.execute("INSERT INTO code_repository_files SELECT repository_id,'new',file_id,path,language_id,blob_hash,byte_len,line_count,parse_status,is_generated,degraded_reason,java_namespace_json FROM code_repository_files WHERE source_scope='old'",[]).unwrap();
    assert_eq!(
        connection
            .query_row(
                "SELECT count(*) FROM code_repository_java_types WHERE source_scope='new'",
                [],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
        2
    );
    connection
        .execute(
            "UPDATE code_repository_files SET java_namespace_json=NULL WHERE source_scope='new'",
            [],
        )
        .unwrap();
    assert_eq!(
        connection
            .query_row(
                "SELECT complete FROM code_repository_java_namespaces WHERE source_scope='new'",
                [],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
        0
    );
    assert_eq!(
        connection
            .query_row(
                "SELECT count(*) FROM code_repository_java_types WHERE source_scope='new'",
                [],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
        0
    );
    connection
        .execute(
            "DELETE FROM code_repository_files WHERE source_scope='old'",
            [],
        )
        .unwrap();
    assert_eq!(
        connection
            .query_row(
                "SELECT count(*) FROM code_repository_java_namespaces WHERE source_scope='old'",
                [],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
        0
    );
    for (path, value) in [
        ("broken.java", Some("not json")),
        ("legacy.java", None),
        (
            "invalid.java",
            Some(
                r#"{"package":"demo","top_level_types":[1],"complete":true,"source_set":{"kind":"repository"}}"#,
            ),
        ),
    ] {
        file(&connection, "new", path, value);
    }
    assert_eq!(
        crate::domain::JavaNamespaceEvidence::MAX_PROJECTED_NAME_BYTES,
        65_536
    );
    let large=serde_json::json!({"package":"p".repeat(4000),"complete":true,"source_set":{"kind":"repository"},"top_level_types":(0..20).map(|i|format!("Type{i}")).collect::<Vec<_>>()}).to_string();
    file(&connection, "new", "large.java", Some(&large));
    assert_eq!(
        connection
            .query_row(
                "SELECT count(*) FROM code_repository_java_namespaces WHERE complete<>0",
                [],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
        0
    );
    connection
        .execute(
            "UPDATE code_repository_files SET language_id='python' WHERE path='legacy.java'",
            [],
        )
        .unwrap();
    assert_eq!(
        connection
            .query_row(
                "SELECT count(*) FROM code_repository_java_namespaces WHERE path='legacy.java'",
                [],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
        0
    );
}

#[test]
fn namespace_schema_open_does_not_backfill_or_build_populated_query_indexes() {
    let store = crate::storage::SqliteGraphStore::open_in_memory().unwrap();
    let connection = store.connection.lock().unwrap();
    connection.execute_batch("PRAGMA foreign_keys=OFF").unwrap();
    super::super::search_schema::ensure_search_query_indexes(&connection).unwrap();
    file(
        &connection,
        "scope",
        "Any.java",
        Some(
            r#"{"package":"p","top_level_types":["System"],"complete":true,"source_set":{"kind":"repository"}}"#,
        ),
    );
    connection.execute_batch("DROP INDEX IF EXISTS code_repository_java_namespace_completeness_lookup; DROP INDEX IF EXISTS code_repository_java_type_lookup; DROP INDEX IF EXISTS code_repository_java_source_type_lookup").unwrap();
    initialize(&connection).unwrap();
    assert_eq!(connection.query_row("SELECT count(*) FROM sqlite_master WHERE type='index' AND name IN ('code_repository_java_namespace_completeness_lookup','code_repository_java_type_lookup')",[],|r|r.get::<_,i64>(0)).unwrap(),0);
    for cursor in [Some(20), Some(21)] {
        super::super::advance_search_query_indexes(&connection, cursor, false).unwrap();
    }
    let plan=connection.prepare("EXPLAIN QUERY PLAN SELECT 1 FROM code_repository_java_types WHERE source_scope=?1 AND package=?2 AND type_name=?3 LIMIT 1").unwrap().query_map(["scope","p","System"],|row|row.get::<_,String>(3)).unwrap().collect::<Result<Vec<_>,_>>().unwrap();
    assert!(
        plan.iter()
            .any(|row| row.contains("code_repository_java_type_lookup")
                && row.contains("source_scope=? AND package=? AND type_name=?")),
        "{plan:?}"
    );
    connection
        .execute(
            "DELETE FROM code_repository_java_namespaces WHERE source_scope='scope'",
            [],
        )
        .unwrap();
    initialize(&connection).unwrap();
    assert_eq!(
        connection
            .query_row(
                "SELECT count(*) FROM code_repository_java_namespaces",
                [],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
        0
    );
}

#[test]
fn source_set_upgrade_keeps_existing_rows_unknown_until_original_facts_are_reindexed() {
    let connection = Connection::open_in_memory().unwrap();
    connection.execute_batch("CREATE TABLE code_repository_files(source_scope TEXT,path TEXT,language_id TEXT,parse_status TEXT,java_namespace_json TEXT);
        CREATE TABLE code_repository_java_namespaces(source_scope TEXT,path TEXT,package TEXT,complete INTEGER,PRIMARY KEY(source_scope,path));
        CREATE TABLE code_repository_java_types(source_scope TEXT,path TEXT,package TEXT,type_name TEXT,PRIMARY KEY(source_scope,path,type_name));
        INSERT INTO code_repository_files VALUES('old','App.java','java','parsed','{}');
        INSERT INTO code_repository_java_namespaces VALUES('old','App.java','p',1);").unwrap();
    initialize(&connection).unwrap();
    assert_eq!(
        connection
            .query_row(
                "SELECT source_set_kind FROM code_repository_java_namespaces",
                [],
                |r| r.get::<_, String>(0)
            )
            .unwrap(),
        "unknown"
    );
    assert_eq!(connection.query_row("SELECT count(*) FROM sqlite_master WHERE type='index' AND name='code_repository_java_source_type_lookup'",[],|r|r.get::<_,i64>(0)).unwrap(),0);
    let evidence = r#"{"package":"p","complete":true,"top_level_types":["App"],"source_set":{"kind":"main","module_root":"module-a"}}"#;
    connection
        .execute(
            "UPDATE code_repository_files SET java_namespace_json=?1",
            [evidence],
        )
        .unwrap();
    assert_eq!(
        connection
            .query_row(
                "SELECT source_set_kind||':'||module_root FROM code_repository_java_types",
                [],
                |r| r.get::<_, String>(0)
            )
            .unwrap(),
        "main:module-a"
    );
    connection
        .execute(
            "UPDATE code_repository_files SET java_namespace_json=?1",
            [r#"{"package":"p","complete":true,"top_level_types":["App"]}"#],
        )
        .unwrap();
    assert_eq!(
        connection
            .query_row(
                "SELECT complete FROM code_repository_java_namespaces",
                [],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
        0
    );
    assert_eq!(
        connection
            .query_row("SELECT count(*) FROM code_repository_java_types", [], |r| r
                .get::<_, i64>(0))
            .unwrap(),
        0
    );
}
