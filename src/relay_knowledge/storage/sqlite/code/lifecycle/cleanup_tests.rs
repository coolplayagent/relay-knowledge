use rusqlite::Connection;

use super::*;

const PATH_TABLES: &[&str] = &[
    "code_repository_file_diagnostics",
    "code_repository_chunks",
    "code_repository_calls",
    "code_repository_routes",
    "code_repository_feature_flags",
    "code_repository_framework_edges",
    "code_repository_framework_nodes",
    "code_repository_dependencies",
    "code_repository_imports",
    "code_repository_reference_search_groups",
    "code_repository_references",
    "code_repository_symbols",
    "code_repository_files",
];

const SCOPE_TABLES: &[&str] = &[
    "code_repository_path_tombstones",
    "code_repository_file_diagnostics",
    "code_repository_chunks",
    "code_repository_calls",
    "code_repository_routes",
    "code_repository_feature_flags",
    "code_repository_framework_edges",
    "code_repository_framework_nodes",
    "code_repository_dependencies",
    "code_repository_imports",
    "code_repository_reference_search_groups",
    "code_repository_reference_search_manifests",
    "code_repository_references",
    "code_repository_symbols",
    "code_repository_files",
    "software_components",
    "software_dependency_usages",
    "software_sdk_usages",
    "software_files",
    "software_topics",
    "software_relationships",
    "software_global_status",
    "software_build_targets",
    "software_iac_resources",
    "software_design_elements",
    "software_entities",
    "software_statements",
    "software_ontology_diagnostics",
    "business_domains",
    "business_terms",
    "business_term_aliases",
    "business_mappings",
    "business_knowledge_status",
];

#[test]
fn delete_scope_index_removes_software_projection_tables() {
    let mut connection = Connection::open_in_memory().expect("connection should open");
    for table in SCOPE_TABLES {
        connection
            .execute(
                &format!("CREATE TABLE {table} (source_scope TEXT NOT NULL)"),
                [],
            )
            .expect("table should create");
        connection
            .execute(
                &format!("INSERT INTO {table} (source_scope) VALUES ('scope'), ('other')"),
                [],
            )
            .expect("rows should insert");
    }
    connection
        .execute(
            "
                CREATE VIRTUAL TABLE code_repository_search USING fts5(
                    source_scope UNINDEXED,
                    document_kind UNINDEXED,
                    record_id UNINDEXED,
                    path UNINDEXED,
                    language_id UNINDEXED,
                    content
                )
                ",
            [],
        )
        .expect("search table should create");
    create_search_metadata_table(&connection);
    connection
        .execute(
            "
                INSERT INTO code_repository_search (
                    source_scope, document_kind, record_id, path, language_id, content
                )
                VALUES ('scope', 'symbol', 'a', 'src/a.rs', 'rust', 'target'),
                       ('other', 'symbol', 'b', 'src/b.rs', 'rust', 'target')
                ",
            [],
        )
        .expect("search rows should insert");
    backfill_search_metadata(&connection);

    let transaction = connection.transaction().expect("transaction should open");
    delete_scope_index(&transaction, "scope").expect("scope should delete");
    transaction.commit().expect("transaction should commit");

    for table in SCOPE_TABLES
        .iter()
        .copied()
        .chain(["code_repository_search"])
    {
        let deleted_remaining = connection
            .query_row(
                &format!("SELECT COUNT(*) FROM {table} WHERE source_scope = 'scope'"),
                [],
                |row| row.get::<_, usize>(0),
            )
            .expect("deleted row count should load");
        let retained_remaining = connection
            .query_row(
                &format!("SELECT COUNT(*) FROM {table} WHERE source_scope = 'other'"),
                [],
                |row| row.get::<_, usize>(0),
            )
            .expect("retained row count should load");
        assert_eq!(deleted_remaining, 0, "{table} should delete pruned scope");
        assert_eq!(retained_remaining, 1, "{table} should keep other scope");
    }
}

#[test]
fn delete_path_indexes_removes_multiple_paths_from_all_path_tables() {
    let mut connection = Connection::open_in_memory().expect("connection should open");
    for table in PATH_TABLES {
        connection
            .execute(
                &format!("CREATE TABLE {table} (source_scope TEXT NOT NULL, path TEXT NOT NULL)"),
                [],
            )
            .expect("table should create");
    }
    connection
        .execute(
            "
                CREATE VIRTUAL TABLE code_repository_search USING fts5(
                    source_scope UNINDEXED,
                    document_kind UNINDEXED,
                    record_id UNINDEXED,
                    path UNINDEXED,
                    language_id UNINDEXED,
                    content
                )
                ",
            [],
        )
        .expect("search table should create");
    create_search_metadata_table(&connection);
    create_reference_search_manifest_table(&connection);

    for path in ["src/a.rs", "src/b.rs", "src/c.rs"] {
        for table in PATH_TABLES {
            connection
                .execute(
                    &format!("INSERT INTO {table} (source_scope, path) VALUES (?1, ?2)"),
                    rusqlite::params!["scope", path],
                )
                .expect("path row should insert");
        }
        connection
            .execute(
                "
                    INSERT INTO code_repository_search (
                        source_scope, document_kind, record_id, path, language_id, content
                    )
                    VALUES (?1, 'symbol', ?2, ?2, 'rust', 'target')
                    ",
                rusqlite::params!["scope", path],
            )
            .expect("search row should insert");
    }
    backfill_search_metadata(&connection);
    connection
        .execute(
            "INSERT INTO code_repository_reference_search_manifests (
                 source_scope, reference_count, group_count
             ) VALUES ('scope', 3, 3)",
            [],
        )
        .expect("reference-search manifest should insert");

    let transaction = connection.transaction().expect("transaction should open");
    assert!(
        path_indexes_exist(&transaction, "scope", ["src/a.rs", "src/b.rs"])
            .expect("path existence should load")
    );
    assert!(
        !path_indexes_exist(&transaction, "scope", ["src/missing.rs"])
            .expect("missing path existence should load")
    );
    delete_path_indexes(&transaction, "scope", ["src/a.rs", "src/b.rs", "src/a.rs"])
        .expect("paths should delete");
    transaction.commit().expect("transaction should commit");

    for table in PATH_TABLES
        .iter()
        .copied()
        .chain(["code_repository_search"])
    {
        let remaining = connection
            .query_row(
                &format!("SELECT COUNT(*) FROM {table} WHERE source_scope = 'scope'"),
                [],
                |row| row.get::<_, usize>(0),
            )
            .expect("remaining row count should load");
        assert_eq!(remaining, 1, "{table} should keep only the unmatched path");
    }
    assert_eq!(
        connection
            .query_row(
                "SELECT reference_count, group_count
                 FROM code_repository_reference_search_manifests
                 WHERE source_scope = 'scope'",
                [],
                |row| Ok((row.get::<_, usize>(0)?, row.get::<_, usize>(1)?)),
            )
            .expect("reference-search manifest should load"),
        (1, 1)
    );
}

fn create_search_metadata_table(connection: &Connection) {
    connection
        .execute(
            "
                CREATE TABLE code_repository_search_metadata (
                    source_scope TEXT NOT NULL,
                    document_kind TEXT NOT NULL,
                    record_id TEXT NOT NULL,
                    path TEXT NOT NULL,
                    search_rowid INTEGER NOT NULL UNIQUE,
                    PRIMARY KEY (source_scope, document_kind, record_id)
                )
                ",
            [],
        )
        .expect("search metadata table should create");
}

fn create_reference_search_manifest_table(connection: &Connection) {
    connection
        .execute(
            "CREATE TABLE code_repository_reference_search_manifests (
                 source_scope TEXT PRIMARY KEY,
                 reference_count INTEGER NOT NULL,
                 group_count INTEGER NOT NULL
             )",
            [],
        )
        .expect("reference-search manifest table should create");
}

fn backfill_search_metadata(connection: &Connection) {
    connection
        .execute(
            "
                INSERT INTO code_repository_search_metadata (
                    source_scope, document_kind, record_id, path, search_rowid
                )
                SELECT source_scope, document_kind, record_id, path, rowid
                FROM code_repository_search
                ",
            [],
        )
        .expect("search metadata rows should insert");
}
