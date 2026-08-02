use super::*;

#[test]
fn creates_checkpoint_and_claimable_task_schema() {
    let connection = Connection::open_in_memory().expect("database should open");

    initialize_index_task_schema(&connection).expect("index task schema should initialize");
    initialize_index_task_schema(&connection).expect("index task schema should be idempotent");

    let table_count: i64 = connection
        .query_row(
            "
            SELECT COUNT(*)
            FROM sqlite_schema
            WHERE type = 'table'
              AND name IN (
                  'code_repository_index_checkpoints',
                  'code_repository_index_tasks'
              )
            ",
            [],
            |row| row.get(0),
        )
        .expect("index task tables should be inspectable");
    assert_eq!(table_count, 2);

    let index_count: i64 = connection
        .query_row(
            "
            SELECT COUNT(*)
            FROM sqlite_schema
            WHERE type = 'index'
              AND name IN (
                  'code_repository_index_tasks_claimable',
                  'code_repository_index_tasks_repository'
              )
            ",
            [],
            |row| row.get(0),
        )
        .expect("task indexes should be inspectable");
    assert_eq!(index_count, 2);
}
