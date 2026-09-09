use super::*;

#[test]
fn copy_cost_charges_missing_legacy_projection_and_materialized_rows() {
    let store = crate::storage::SqliteGraphStore::open_in_memory().unwrap();
    let connection = store.connection.lock().unwrap();
    connection.execute_batch("PRAGMA foreign_keys=OFF;
        INSERT INTO code_repository_files(repository_id,source_scope,file_id,path,language_id,blob_hash,byte_len,line_count,parse_status)
        VALUES('repo','scope','file','App.java','java','blob',1,1,'parsed');").unwrap();
    let sql = format!("SELECT {COPY_ROWS}, {COPY_BYTES} FROM code_repository_files source");
    let cost = || {
        connection
            .query_row(&sql, [], |row| {
                Ok((row.get::<_, usize>(0)?, row.get::<_, usize>(1)?))
            })
            .unwrap()
    };
    assert_eq!(cost(), (1, 141));
    connection
        .execute("DELETE FROM code_repository_java_namespaces", [])
        .unwrap();
    assert_eq!(cost(), (1, 141));
    connection
        .execute(
            "UPDATE code_repository_files SET java_namespace_json=?1",
            [r#"{"package":"demo","top_level_types":["One","Two"],"complete":true}"#],
        )
        .unwrap();
    assert_eq!(cost(), (3, 3 * 145 + 6));
    connection
        .execute("UPDATE code_repository_files SET language_id='rust'", [])
        .unwrap();
    assert_eq!(cost(), (0, 0));
}
