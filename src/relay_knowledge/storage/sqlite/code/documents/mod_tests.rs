use rusqlite::{Connection, params, params_from_iter, types::Value};

use super::{
    MAX_DOCUMENT_BYTES, MAX_DOCUMENT_FILES, MAX_DOCUMENT_PATH_FILTERS, SqlPathPredicate,
    bounded_document_content_query, bounded_file_metadata_query, read_indexed_markdown,
};
use crate::storage::StorageError;

#[test]
fn reads_only_scoped_markdown_and_reassembles_chunks_in_source_order() {
    let mut connection = document_connection();

    let documents = read_indexed_markdown(&mut connection, "scope-a", &["docs".to_owned()], 2, 64)
        .expect("bounded Markdown documents should load");

    assert_eq!(documents.len(), 2);
    assert_eq!(documents[0].path, "docs/a.md");
    assert_eq!(documents[0].language_id, "markdown");
    assert_eq!(documents[0].content, "alpha\nbeta");
    assert_eq!(documents[1].path, "docs/b.md");
    assert_eq!(documents[1].content, "gamma");
}

#[test]
fn rejects_invalid_and_exhausted_document_budgets() {
    let mut connection = document_connection();

    assert!(matches!(
        read_indexed_markdown(&mut connection, "scope-a", &[], 0, 64),
        Err(StorageError::InvalidInput(message))
            if message == format!(
                "repository document file limit must be between 1 and {MAX_DOCUMENT_FILES}"
            )
    ));
    assert!(matches!(
        read_indexed_markdown(
            &mut connection,
            "scope-a",
            &[],
            1,
            MAX_DOCUMENT_BYTES + 1,
        ),
        Err(StorageError::InvalidInput(message))
            if message == format!(
                "repository document byte limit must be between 1 and {MAX_DOCUMENT_BYTES}"
            )
    ));
    assert!(matches!(
        read_indexed_markdown(&mut connection, "scope-a", &[], 1, 64),
        Err(StorageError::InvalidInput(message))
            if message == "repository document file budget exhausted"
    ));
    assert!(matches!(
        read_indexed_markdown(
            &mut connection,
            "scope-a",
            &["docs/a.md".to_owned()],
            1,
            8,
        ),
        Err(StorageError::InvalidInput(message))
            if message == "repository document byte budget exhausted"
    ));
}

#[test]
fn preflights_file_and_byte_budgets_before_materializing_content() {
    let mut file_limited = document_connection();
    file_limited
        .execute(
            "UPDATE code_repository_chunks SET content = ?1 WHERE path = 'docs/a.md'",
            params![Value::Blob(vec![0xff])],
        )
        .expect("invalid text fixture should update");
    assert!(matches!(
        read_indexed_markdown(&mut file_limited, "scope-a", &[".".to_owned()], 1, 64),
        Err(StorageError::InvalidInput(message))
            if message == "repository document file budget exhausted"
    ));

    let mut byte_limited = document_connection();
    byte_limited
        .execute(
            "UPDATE code_repository_files SET byte_len = 65 WHERE path = 'docs/a.md'",
            [],
        )
        .expect("indexed byte fixture should update");
    byte_limited
        .execute(
            "UPDATE code_repository_chunks SET content = ?1 WHERE path = 'docs/a.md'",
            params![Value::Blob(vec![0xff])],
        )
        .expect("invalid text fixture should update");
    assert!(matches!(
        read_indexed_markdown(
            &mut byte_limited,
            "scope-a",
            &["docs/a.md".to_owned()],
            1,
            64,
        ),
        Err(StorageError::InvalidInput(message))
            if message == "repository document byte budget exhausted"
    ));
}

#[test]
fn path_filter_excludes_large_outside_content_before_it_is_read() {
    let mut connection = document_connection();
    for index in 0..512 {
        insert_document(
            &connection,
            "scope-a",
            &format!("archive/{index:04}.md"),
            1024 * 1024,
            Value::Blob(vec![0xff]),
        );
    }

    let documents =
        read_indexed_markdown(&mut connection, "scope-a", &["docs/a.md".to_owned()], 1, 10)
            .expect("out-of-path blobs must not consume the selected document budget");

    assert_eq!(documents.len(), 1);
    assert_eq!(documents[0].path, "docs/a.md");
    assert_eq!(documents[0].content, "alpha\nbeta");
}

#[test]
fn root_and_multiple_literal_roots_match_rust_path_semantics() {
    let mut connection = document_connection();
    let root_documents =
        read_indexed_markdown(&mut connection, "scope-a", &[".".to_owned()], 2, 15)
            .expect("dot should select the complete repository root");
    assert_eq!(
        root_documents
            .iter()
            .map(|document| document.path.as_str())
            .collect::<Vec<_>>(),
        ["docs/a.md", "docs/b.md"]
    );

    insert_document(
        &connection,
        "scope-like",
        r"docs/a%_\root/one.md",
        7,
        Value::Text("literal".to_owned()),
    );
    insert_document(
        &connection,
        "scope-like",
        "docs/axxxroot/decoy.md",
        5,
        Value::Text("decoy".to_owned()),
    );
    insert_document(
        &connection,
        "scope-like",
        r"DOCS/a%_\root/case.md",
        4,
        Value::Blob(vec![0xff]),
    );
    insert_document(
        &connection,
        "scope-like",
        "guides/two.md",
        5,
        Value::Text("guide".to_owned()),
    );

    let documents = read_indexed_markdown(
        &mut connection,
        "scope-like",
        &[r"./docs/a%_\root/".to_owned(), "guides".to_owned()],
        2,
        12,
    )
    .expect("multiple roots must preserve literal SQL metacharacters and case");

    assert_eq!(
        documents
            .iter()
            .map(|document| document.path.as_str())
            .collect::<Vec<_>>(),
        [r"docs/a%_\root/one.md", "guides/two.md"]
    );
}

#[test]
fn materialized_bytes_and_path_filter_count_remain_bounded() {
    let mut connection = document_connection();
    connection
        .execute(
            "UPDATE code_repository_files SET byte_len = 1 WHERE path = 'docs/a.md'",
            [],
        )
        .expect("indexed byte fixture should update");
    assert!(matches!(
        read_indexed_markdown(
            &mut connection,
            "scope-a",
            &["docs/a.md".to_owned()],
            1,
            9,
        ),
        Err(StorageError::InvalidInput(message))
            if message == "repository document byte budget exhausted"
    ));

    let excessive_filters = (0..=MAX_DOCUMENT_PATH_FILTERS)
        .map(|index| format!("docs/{index}"))
        .collect::<Vec<_>>();
    assert!(matches!(
        read_indexed_markdown(&mut connection, "scope-a", &excessive_filters, 1, 64),
        Err(StorageError::InvalidInput(message))
            if message == format!(
                "repository document path filter limit must not exceed {MAX_DOCUMENT_PATH_FILTERS}"
            )
    ));

    let mut empty_chunks = document_connection();
    insert_document(
        &empty_chunks,
        "scope-empty",
        "docs/empty.md",
        0,
        Value::Text(String::new()),
    );
    let okf_content = "---\ntype: research\nname: Focus\n---\n";
    insert_document(
        &empty_chunks,
        "scope-empty",
        "docs/focus.md",
        okf_content.len(),
        Value::Text(okf_content.to_owned()),
    );
    let mixed_documents = read_indexed_markdown(
        &mut empty_chunks,
        "scope-empty",
        &["docs".to_owned()],
        2,
        okf_content.len(),
    )
    .expect("one empty Markdown file must not block a valid OKF document");
    assert_eq!(mixed_documents.len(), 2);
    assert_eq!(mixed_documents[0].path, "docs/empty.md");
    assert!(mixed_documents[0].content.is_empty());
    assert_eq!(mixed_documents[1].path, "docs/focus.md");
    assert_eq!(mixed_documents[1].content, okf_content);

    empty_chunks
        .execute(
            "INSERT INTO code_repository_chunks VALUES
                ('scope-empty', 'empty-2', 'docs/empty.md', '', 0, 0),
                ('scope-empty', 'empty-3', 'docs/empty.md', '', 0, 0)",
            [],
        )
        .expect("empty chunk fixtures should insert");
    assert!(matches!(
        read_indexed_markdown(
            &mut empty_chunks,
            "scope-empty",
            &["docs/empty.md".to_owned()],
            1,
            1,
        ),
        Err(StorageError::InvalidInput(message))
            if message == "repository document 'docs/empty.md' is not lossless in the indexed snapshot; re-index the repository"
    ));
}

#[test]
fn rejects_trimmed_legacy_chunks_instead_of_projecting_changed_markdown() {
    let mut connection = document_connection();
    connection
        .execute(
            "UPDATE code_repository_chunks
             SET content = 'alpha', byte_end = 6
             WHERE source_scope = 'scope-a' AND chunk_id = 'a-first'",
            [],
        )
        .expect("legacy trimmed chunk fixture should update");

    assert!(matches!(
        read_indexed_markdown(
            &mut connection,
            "scope-a",
            &["docs/a.md".to_owned()],
            1,
            64,
        ),
        Err(StorageError::InvalidInput(message))
            if message == "repository document 'docs/a.md' is not lossless in the indexed snapshot; re-index the repository"
    ));
}

#[test]
fn filtered_query_plans_seek_files_and_chunks_by_path() {
    let connection = document_connection();
    let predicate = SqlPathPredicate::new(&["docs".to_owned()]).expect("path predicate");
    let (file_sql, file_values) = bounded_file_metadata_query("scope-a", &predicate, 3);
    let file_plan = explain_query_plan(&connection, &file_sql, &file_values);
    assert!(
        file_plan
            .iter()
            .any(|detail| detail.contains("source_scope=? AND path>? AND path<?")),
        "file preflight must use the BINARY path range: {file_plan:?}"
    );

    let (content_sql, content_values) = bounded_document_content_query("scope-a", &predicate);
    let content_plan = explain_query_plan(&connection, &content_sql, &content_values);
    assert!(
        content_plan.iter().any(|detail| {
            detail.contains("code_repository_chunks_lookup (source_scope=? AND path=?)")
        }),
        "content reads must seek chunks for each filtered file: {content_plan:?}"
    );
}

fn document_connection() -> Connection {
    let connection = Connection::open_in_memory().expect("in-memory database should open");
    connection
        .execute_batch(
            "
            CREATE TABLE code_repository_files (
                source_scope TEXT NOT NULL,
                path TEXT NOT NULL,
                language_id TEXT NOT NULL,
                byte_len INTEGER NOT NULL,
                PRIMARY KEY (source_scope, path)
            );
            CREATE TABLE code_repository_chunks (
                source_scope TEXT NOT NULL,
                chunk_id TEXT NOT NULL,
                path TEXT NOT NULL,
                content TEXT NOT NULL,
                byte_start INTEGER NOT NULL,
                byte_end INTEGER NOT NULL,
                PRIMARY KEY (source_scope, chunk_id)
            );
            CREATE INDEX code_repository_chunks_lookup
                ON code_repository_chunks(source_scope, path);
            INSERT INTO code_repository_files VALUES
                ('scope-a', 'docs/a.md', 'markdown', 10),
                ('scope-a', 'docs/b.md', 'markdown', 5),
                ('scope-a', 'src/main.rs', 'rust', 12),
                ('scope-b', 'docs/other.md', 'markdown', 5);
            INSERT INTO code_repository_chunks VALUES
                ('scope-a', 'a-second', 'docs/a.md', 'beta', 6, 10),
                ('scope-a', 'a-first', 'docs/a.md', 'alpha\n', 0, 6),
                ('scope-a', 'b-first', 'docs/b.md', 'gamma', 0, 5),
                ('scope-a', 'rust-first', 'src/main.rs', 'fn main() {}', 0, 12),
                ('scope-b', 'other-first', 'docs/other.md', 'other', 0, 5);
            ",
        )
        .expect("document fixture schema should initialize");
    connection
}

fn insert_document(
    connection: &Connection,
    source_scope: &str,
    path: &str,
    byte_len: usize,
    content: Value,
) {
    let content_len = match &content {
        Value::Text(content) => content.len(),
        Value::Blob(content) => content.len(),
        _ => panic!("document fixtures only support text or blob content"),
    };
    connection
        .execute(
            "INSERT INTO code_repository_files VALUES (?1, ?2, 'markdown', ?3)",
            params![source_scope, path, byte_len],
        )
        .expect("document file fixture should insert");
    connection
        .execute(
            "INSERT INTO code_repository_chunks VALUES (?1, ?2, ?3, ?4, 0, ?5)",
            params![
                source_scope,
                format!("{path}#chunk"),
                path,
                content,
                content_len
            ],
        )
        .expect("document chunk fixture should insert");
}

fn explain_query_plan(connection: &Connection, sql: &str, values: &[Value]) -> Vec<String> {
    let mut statement = connection
        .prepare(&format!("EXPLAIN QUERY PLAN {sql}"))
        .expect("query plan should prepare");
    statement
        .query_map(params_from_iter(values.iter()), |row| {
            row.get::<_, String>(3)
        })
        .expect("query plan should execute")
        .map(|row| row.expect("query-plan detail should decode"))
        .collect()
}
