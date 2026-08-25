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

    let files = software_file_page(&connection, "scope", GraphVersion::new(3), 10, None)
        .expect("software files should load");

    assert_eq!(files.len(), 1);
    assert_eq!(files[0].file_role, "dependency_manifest");
    assert_eq!(files[0].created_graph_version, GraphVersion::new(3));
}

#[test]
fn file_projection_reuses_one_insert_prepare_across_keyset_pages() {
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    use rusqlite::hooks::{AuthAction, AuthContext, Authorization};

    const FILE_COUNT: usize = 1_025;

    let connection = Connection::open_in_memory().expect("database should open");
    connection
        .execute_batch(
            "
            CREATE TABLE code_repository_files (
                repository_id TEXT NOT NULL,
                source_scope TEXT NOT NULL,
                path TEXT NOT NULL,
                language_id TEXT NOT NULL,
                parse_status TEXT NOT NULL,
                PRIMARY KEY (source_scope, path)
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
            CREATE TABLE software_files (
                software_file_id TEXT PRIMARY KEY,
                repository_id TEXT NOT NULL,
                source_scope TEXT NOT NULL,
                path TEXT NOT NULL,
                language_id TEXT NOT NULL,
                file_role TEXT NOT NULL,
                parse_status TEXT NOT NULL,
                created_graph_version INTEGER NOT NULL
            );
            WITH RECURSIVE file_number(value) AS (
                VALUES (0)
                UNION ALL
                SELECT value + 1 FROM file_number WHERE value < 1024
            )
            INSERT INTO code_repository_files (
                repository_id, source_scope, path, language_id, parse_status
            )
            SELECT 'repository', 'scope', printf('src/file-%04d.rs', value), 'rust', 'parsed'
            FROM file_number;
            ",
        )
        .expect("file projection schema and source rows should initialize");

    let insert_prepare_count = Arc::new(AtomicUsize::new(0));
    let observed_insert_prepares = Arc::clone(&insert_prepare_count);
    connection.authorizer(Some(move |context: AuthContext<'_>| {
        if matches!(
            context.action,
            AuthAction::Insert {
                table_name: "software_files"
            }
        ) {
            observed_insert_prepares.fetch_add(1, Ordering::Relaxed);
        }
        Authorization::Allow
    }));

    let projected = materialize_files(&connection, "scope", GraphVersion::new(9))
        .expect("software files should materialize");
    let rows = connection
        .prepare(
            "
            SELECT path, file_role, parse_status, created_graph_version
            FROM software_files
            WHERE source_scope = 'scope'
            ORDER BY path ASC
            ",
        )
        .expect("projected files should be queryable")
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, u64>(3)?,
            ))
        })
        .expect("projected file rows should decode")
        .collect::<Result<Vec<_>, _>>()
        .expect("projected file rows should collect");
    let expected = (0..FILE_COUNT)
        .map(|value| {
            (
                format!("src/file-{value:04}.rs"),
                "source".to_owned(),
                "parsed".to_owned(),
                9_u64,
            )
        })
        .collect::<Vec<_>>();
    let distinct_ids = connection
        .query_row(
            "SELECT COUNT(DISTINCT software_file_id) FROM software_files WHERE source_scope = 'scope'",
            [],
            |row| row.get::<_, usize>(0),
        )
        .expect("projected software file ids should count");

    assert_eq!(projected, FILE_COUNT);
    assert_eq!(rows, expected);
    assert_eq!(distinct_ids, FILE_COUNT);
    assert_eq!(insert_prepare_count.load(Ordering::Relaxed), 1);
}
