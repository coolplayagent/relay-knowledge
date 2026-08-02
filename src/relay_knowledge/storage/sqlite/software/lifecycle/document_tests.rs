use rusqlite::Connection;

use super::load;

#[test]
fn load_groups_ordered_chunks_into_source_documents() {
    let connection = Connection::open_in_memory().expect("sqlite should open");
    connection
        .execute_batch(
            "
            CREATE TABLE code_repository_chunks (
                repository_id TEXT NOT NULL,
                source_scope TEXT NOT NULL,
                chunk_id TEXT NOT NULL,
                path TEXT NOT NULL,
                language_id TEXT NOT NULL,
                content TEXT NOT NULL,
                line_start INTEGER NOT NULL
            );
            INSERT INTO code_repository_chunks (
                repository_id, source_scope, chunk_id, path, language_id, content, line_start
            ) VALUES
                ('repo', 'scope', 'chunk-2', 'src/lib.rs', 'rust', 'third', 3),
                ('repo', 'scope', 'chunk-1', 'src/lib.rs', 'rust', 'first\nsecond', 1),
                ('repo', 'scope', 'chunk-doc', 'README.md', 'markdown', '# Title', 1),
                ('repo', 'other', 'chunk-other', 'ignored.rs', 'rust', 'ignored', 1);
            ",
        )
        .expect("chunk fixtures should insert");

    let documents = load(&connection, "scope").expect("documents should load");

    assert_eq!(documents.len(), 2);
    assert_eq!(documents[0].path, "README.md");
    assert_eq!(documents[1].path, "src/lib.rs");
    assert_eq!(
        documents[1]
            .lines
            .iter()
            .map(|line| (line.number, line.text.as_str()))
            .collect::<Vec<_>>(),
        vec![(1, "first"), (2, "second"), (3, "third")]
    );
}
