use super::*;

#[test]
fn file_row_mapping_preserves_projection_identity_and_version() {
    let connection = Connection::open_in_memory().expect("database should open");

    let file = connection
        .query_row(
            "
            SELECT 'software-file', 'repository', 'scope', 'src/lib.rs', 'rust',
                   'source', 'parsed', 7
            ",
            [],
            file_from_row,
        )
        .expect("software file should decode");

    assert_eq!(file.software_file_id, "software-file");
    assert_eq!(file.path, "src/lib.rs");
    assert_eq!(file.file_role, "source");
    assert_eq!(file.created_graph_version, GraphVersion::new(7));
}

#[test]
fn file_page_assigns_owned_roles_before_domain_validation() {
    let connection = Connection::open_in_memory().expect("database should open");
    connection
        .execute_batch(
            "
            CREATE TABLE code_repository_files (
                repository_id TEXT NOT NULL,
                source_scope TEXT NOT NULL,
                path TEXT NOT NULL,
                language_id TEXT NOT NULL,
                parse_status TEXT NOT NULL
            );
            CREATE TABLE code_repository_symbols (
                repository_id TEXT NOT NULL,
                source_scope TEXT NOT NULL,
                path TEXT NOT NULL,
                name TEXT NOT NULL,
                kind TEXT NOT NULL,
                line_start INTEGER NOT NULL,
                line_end INTEGER NOT NULL
            );
            INSERT INTO code_repository_files VALUES
                ('repository', 'scope', 'Cargo.toml', 'toml', 'parsed'),
                ('repository', 'other', 'src/lib.rs', 'rust', 'parsed');
            ",
        )
        .expect("file source schema should initialize");

    let files = software_file_page(&connection, "scope", GraphVersion::new(3), 10, 0)
        .expect("software files should load");

    assert_eq!(files.len(), 1);
    assert_eq!(files[0].file_role, "dependency_manifest");
    assert_eq!(files[0].created_graph_version, GraphVersion::new(3));
}

#[test]
fn file_page_marks_only_root_authorized_topic_shards() {
    let connection = Connection::open_in_memory().expect("database should open");
    connection
        .execute_batch(
            "
            CREATE TABLE code_repository_files (
                repository_id TEXT NOT NULL,
                source_scope TEXT NOT NULL,
                path TEXT NOT NULL,
                language_id TEXT NOT NULL,
                parse_status TEXT NOT NULL
            );
            CREATE TABLE code_repository_symbols (
                repository_id TEXT NOT NULL,
                source_scope TEXT NOT NULL,
                path TEXT NOT NULL,
                name TEXT NOT NULL,
                kind TEXT NOT NULL,
                line_start INTEGER NOT NULL,
                line_end INTEGER NOT NULL
            );
            INSERT INTO code_repository_files VALUES
                ('repository', 'scope', '.knowledge/knowledge-map.yaml', 'yaml', 'parsed'),
                ('repository', 'scope', '.knowledge/topics/current.yaml', 'yaml', 'parsed'),
                ('repository', 'scope', '.knowledge/topics/orphan.yaml', 'yaml', 'parsed');
            INSERT INTO code_repository_symbols VALUES
                ('repository', 'scope', '.knowledge/knowledge-map.yaml',
                 '.knowledge/topics/current.yaml', 'knowledge_map_topic_shard_ref', 5, 5),
                ('repository', 'scope', '.knowledge/knowledge-map.yaml',
                 'Build', 'knowledge_map_topic_shard_topic', 5, 5),
                ('repository', 'scope', '.knowledge/topics/current.yaml',
                 'Build', 'knowledge_map_topic_shard', 3, 3),
                ('repository', 'scope', '.knowledge/topics/orphan.yaml',
                 'Orphan', 'knowledge_map_topic_shard', 3, 3);
            ",
        )
        .expect("file source schema should initialize");

    let files = software_file_page(&connection, "scope", GraphVersion::new(3), 10, 0)
        .expect("software files should load");

    assert_eq!(files[0].file_role, "knowledge_map_manifest");
    assert_eq!(files[1].file_role, "knowledge_map_topic_shard");
    assert_eq!(files[2].file_role, "configuration");
}
