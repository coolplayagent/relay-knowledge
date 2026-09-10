use super::super::test_support::{record, request, status};
use super::*;
use crate::domain::JavaImplicitPlatformRead;

fn file(connection: &Connection, path: &str, json: Option<&str>) {
    connection.execute("INSERT INTO code_repository_files(repository_id,source_scope,file_id,path,language_id,blob_hash,byte_len,line_count,parse_status,java_namespace_json)
        VALUES('repo','scope',?1,?1,'java','blob',1,1,'parsed',?2)",params![path,json]).unwrap();
}

#[test]
fn same_package_type_proof_precedes_seed_limits_and_provider_deletion_restores_raw_reads() {
    let store = crate::storage::SqliteGraphStore::open_in_memory().unwrap();
    let mut connection = store.connection.lock().unwrap();
    connection.execute_batch("PRAGMA foreign_keys=OFF").unwrap();
    file(
        &connection,
        "src/App.java",
        Some(
            r#"{"package":"demo","top_level_types":["App"],"complete":true,"source_set":{"kind":"repository"}}"#,
        ),
    );
    file(
        &connection,
        "other/OddFilename.java",
        Some(
            r#"{"package":"demo","top_level_types":["System"],"complete":true,"source_set":{"kind":"repository"}}"#,
        ),
    );
    let mut rows = Vec::new();
    for i in 0..1100 {
        let mut row = record(&format!("r{i}"), &format!("a{i:04}"), "config_key");
        row.path = "src/App.java".into();
        row.metadata.java_implicit_platform = Some(JavaImplicitPlatformRead {
            type_name: "System".into(),
        });
        rows.push(row);
    }
    let mut explicit = record("explicit", "zz_explicit", "config_key");
    explicit.path = "src/App.java".into();
    rows.push(explicit);
    let tx = connection.transaction().unwrap();
    super::super::insert_records(&tx, &rows).unwrap();
    tx.commit().unwrap();
    let mut query = request();
    query.limit = 1;
    query.repository.path_filters = vec!["src/App.java".into()];
    for consistency in [false, true] {
        query.consistency = consistency;
        let result = super::super::knowledge::search(&connection, &status(), &query).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].source_key, "zz_explicit");
    }
    // Changing packages also removes only this provider's shadowing evidence.
    connection.execute("UPDATE code_repository_files SET java_namespace_json=?1 WHERE path='other/OddFilename.java'",[r#"{"package":"other","top_level_types":["System"],"complete":true,"source_set":{"kind":"repository"}}"#]).unwrap();
    query.consistency = false;
    assert_eq!(
        super::super::knowledge::search(&connection, &status(), &query).unwrap()[0].source_key,
        "a0000"
    );
    connection
        .execute(
            "DELETE FROM code_repository_files WHERE path='other/OddFilename.java'",
            [],
        )
        .unwrap();
    assert_eq!(
        super::super::knowledge::search(&connection, &status(), &query).unwrap()[0].source_key,
        "a0000"
    );
    assert_eq!(
        connection
            .query_row(
                "SELECT count(*) FROM code_repository_feature_flags",
                [],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
        1101
    );
    let mut filesystem = status();
    filesystem.last_indexed_commit = Some("filesystem:tree".into());
    assert_eq!(
        super::super::knowledge::search(&connection, &filesystem, &query).unwrap()[0].source_key,
        "zz_explicit"
    );
    for root in [".", "./", " .\\ ", "./."] {
        filesystem.path_filters = vec![root.into(), "src".into()];
        assert_eq!(
            super::super::knowledge::search(&connection, &filesystem, &query).unwrap()[0]
                .source_key,
            "a0000"
        );
    }
    filesystem.path_filters = vec!["src".into()];
    assert_eq!(
        super::super::knowledge::search(&connection, &filesystem, &query).unwrap()[0].source_key,
        "zz_explicit"
    );
    let mut restricted = status();
    restricted.path_filters = vec!["src".into()];
    assert_eq!(
        super::super::knowledge::search(&connection, &restricted, &query).unwrap()[0].source_key,
        "zz_explicit"
    );
    file(&connection, "legacy.java", None);
    assert_eq!(
        super::super::knowledge::search(&connection, &status(), &query).unwrap()[0].source_key,
        "zz_explicit"
    );
    connection
        .execute(
            "DELETE FROM code_repository_java_namespaces WHERE path='legacy.java'",
            [],
        )
        .unwrap();
    // Missing legacy evidence is Unknown, never an empty namespace proof.
    assert_eq!(
        super::super::knowledge::search(&connection, &status(), &query).unwrap()[0].source_key,
        "zz_explicit"
    );
}

#[test]
fn source_set_visibility_is_per_module_and_preserves_repository_and_unknown_contracts() {
    let store = crate::storage::SqliteGraphStore::open_in_memory().unwrap();
    let mut connection = store.connection.lock().unwrap();
    connection.execute_batch("PRAGMA foreign_keys=OFF").unwrap();
    let evidence = |kind: &str, module: &str, name: &str| {
        serde_json::json!({
            "package":"p", "complete":true, "top_level_types":[name],
            "source_set":{"kind":kind,"module_root":module}
        })
        .to_string()
    };
    file(
        &connection,
        "App.java",
        Some(&evidence("main", "one", "App")),
    );
    file(
        &connection,
        "Provider.java",
        Some(&evidence("test", "one", "System")),
    );
    let mut row = record("read", "flag", "config_key");
    row.path = "App.java".into();
    row.metadata.java_implicit_platform = Some(JavaImplicitPlatformRead {
        type_name: "System".into(),
    });
    let tx = connection.transaction().unwrap();
    super::super::insert_records(&tx, &[row]).unwrap();
    tx.commit().unwrap();
    for (consumer, provider, module, expected) in [
        ("main", "test", "one", 1),
        ("main", "main", "two", 1),
        ("main", "main", "one", 0),
        ("test", "main", "one", 0),
        ("test", "test", "one", 0),
        ("test", "test", "two", 1),
        ("repository", "test", "two", 0),
        ("main", "repository", "two", 0),
        ("main", "unknown", "two", 0),
    ] {
        connection
            .execute(
                "UPDATE code_repository_files SET java_namespace_json=?1 WHERE path='App.java'",
                [evidence(consumer, "one", "App")],
            )
            .unwrap();
        connection.execute("UPDATE code_repository_files SET java_namespace_json=?1 WHERE path='Provider.java'",[evidence(provider,module,"System")]).unwrap();
        let mut query = request();
        query.repository.path_filters = vec!["App.java".into()];
        assert_eq!(
            super::super::knowledge::search(&connection, &status(), &query)
                .unwrap()
                .len(),
            expected,
            "{consumer}/{provider}/{module}"
        );
    }
}

#[test]
fn incomplete_namespaces_only_block_consumers_that_can_see_their_source_set() {
    let store = crate::storage::SqliteGraphStore::open_in_memory().unwrap();
    let mut connection = store.connection.lock().unwrap();
    connection.execute_batch("PRAGMA foreign_keys=OFF").unwrap();
    file(
        &connection,
        "App.java",
        Some(
            r#"{"package":"p","complete":true,"top_level_types":["App"],"source_set":{"kind":"main","module_root":"one"}}"#,
        ),
    );
    let mut row = record("read", "flag", "config_key");
    row.path = "App.java".into();
    row.metadata.java_implicit_platform = Some(JavaImplicitPlatformRead {
        type_name: "System".into(),
    });
    let tx = connection.transaction().unwrap();
    super::super::insert_records(&tx, &[row]).unwrap();
    tx.commit().unwrap();
    file(&connection, "Broken.java", None);
    for (kind, module, expected) in [
        ("test", "one", 1),
        ("main", "two", 1),
        ("main", "one", 0),
        ("repository", "", 0),
        ("unknown", "", 0),
    ] {
        let evidence=serde_json::json!({"package":"","complete":false,"top_level_types":[],"source_set":{"kind":kind,"module_root":module}}).to_string();
        connection
            .execute(
                "UPDATE code_repository_files SET java_namespace_json=?1 WHERE path='Broken.java'",
                [evidence],
            )
            .unwrap();
        assert_eq!(
            super::super::knowledge::search(&connection, &status(), &request())
                .unwrap()
                .len(),
            expected,
            "{kind}/{module}"
        );
    }
}
