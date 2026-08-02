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
