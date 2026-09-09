use super::*;
use crate::domain::CodeIndexResourceBudget;
#[test]
fn file_clone_page_accounts_for_triggered_namespace_rows_and_bytes() {
    let store = crate::storage::SqliteGraphStore::open_in_memory().unwrap();
    let mut connection = store.connection.lock().unwrap();
    connection.execute_batch("PRAGMA foreign_keys=OFF").unwrap();
    let payload=serde_json::json!({"package":"demo", "complete":true, "top_level_types":(0..10).map(|i|format!("Type{i}")).collect::<Vec<_>>()}).to_string();
    for path in ["a.java", "b.java"] {
        connection.execute("INSERT INTO code_repository_files(repository_id,source_scope,file_id,path,language_id,blob_hash,byte_len,line_count,parse_status,java_namespace_json) VALUES('repo','base',?1,?1,'java','blob',1,1,'parsed',?2)",rusqlite::params![path,payload]).unwrap();
    }
    let current = sample_progress("new", "repo", CodeIndexResourceBudget::default());
    let tx = connection.transaction().unwrap();
    let table = table_at(0).unwrap();
    let page = load_page(&tx, table, &current, 7, 1_000_000).unwrap();
    assert_eq!(page.row_count, 1);
    assert_eq!(page.projection_rows, 11);
    assert!(page.has_more);
    assert!(load_page(&tx, table, &current, 7, page.bytes - 1).is_err());
    assert_eq!(
        copy_prefix(&tx, table, &current, page.last.as_ref().unwrap()).unwrap(),
        1
    );
    assert_eq!(
        tx.query_row(
            "SELECT count(*) FROM code_repository_java_types WHERE source_scope='new'",
            [],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        10
    );
    assert_eq!(
        tx.query_row(
            "SELECT count(*) FROM code_repository_files WHERE source_scope='new'",
            [],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        1
    );
    tx.rollback().unwrap();
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
}

fn sample_progress(
    source_scope: &str,
    repository_id: &str,
    budget: CodeIndexResourceBudget,
) -> progress::CloneProgress {
    progress::CloneProgress {
        source_scope: source_scope.to_owned(),
        repository_id: repository_id.to_owned(),
        base_scope: "base".to_owned(),
        task_id: "task".to_owned(),
        delta_digest: "digest".to_owned(),
        phase: progress::PHASE_TABLES.to_owned(),
        table_ordinal: 0,
        completed_page_ordinal: 0,
        cursor_key: None,
        cursor_tiebreaker: None,
        completed_table_ordinal: None,
        expected_table_rows: None,
        scanned_table_rows: 0,
        copied_table_rows: 0,
        scanned_total_rows: 0,
        copied_total_rows: 0,
        copied_total_bytes: 0,
        cloned_file_count: 0,
        cloned_symbol_count: 0,
        cloned_reference_count: 0,
        cloned_chunk_count: 0,
        cloned_diagnostic_count: 0,
        cloned_reference_group_count: 0,
        cloned_search_document_count: 0,
        base_manifest_reference_count: 0,
        base_manifest_group_count: 0,
        scanned_reference_occurrence_count: 0,
        scanned_reference_row_count: 0,
        scanned_reference_group_count: 0,
        scanned_reference_search_owner_count: 0,
        base_source_fact_row_upper_bound: 1,
        page_row_limit: budget.max_rows_per_batch,
        page_byte_limit: budget.max_bytes_per_batch,
    }
}
