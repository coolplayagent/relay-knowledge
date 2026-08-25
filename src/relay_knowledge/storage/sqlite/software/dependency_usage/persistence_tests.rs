use rusqlite::{Connection, params};

use super::*;
use crate::domain::{CodeRepositorySelector, FreshnessPolicy, SoftwareGlobalKind};

#[test]
fn usage_persistence_round_trip_filters_and_deletes_scope() {
    let connection = usage_schema();
    insert_usages(
        &connection,
        &[
            usage("rust", "src/lib.rs", "serde"),
            usage("python", "scripts/tool.py", "requests"),
        ],
    )
    .expect("usage batch should insert");
    let selector = CodeRepositorySelector::new(
        "repo",
        "commit",
        vec!["src".to_owned()],
        vec!["rust".to_owned()],
    )
    .expect("selector should validate");
    let request = SoftwareGlobalRequest::new(
        selector,
        SoftwareGlobalKind::Dependencies,
        FreshnessPolicy::AllowStale,
        10,
    )
    .expect("request should validate");

    let usages = usages_for_scope(&connection, "scope", &request, 10).expect("usages should query");

    assert_eq!(usages.len(), 1);
    assert_eq!(usages[0].package_name, "serde");
    assert_eq!(usages[0].created_graph_version, GraphVersion::new(7));

    delete_scope(&connection, "scope").expect("scope should delete");
    assert!(
        usages_for_scope(&connection, "scope", &request, 10)
            .expect("deleted scope should query")
            .is_empty()
    );
}

#[test]
fn import_evidence_rejects_cap_plus_one() {
    let connection = Connection::open_in_memory().expect("database should open");
    connection
        .execute_batch(
            "
            CREATE TABLE code_repository_files (
                source_scope TEXT NOT NULL,
                path TEXT NOT NULL,
                language_id TEXT NOT NULL,
                is_generated INTEGER NOT NULL DEFAULT 0
            );
            CREATE TABLE code_repository_imports (
                repository_id TEXT NOT NULL,
                source_scope TEXT NOT NULL,
                module TEXT NOT NULL,
                target_hint TEXT,
                resolution_state TEXT NOT NULL,
                path TEXT NOT NULL,
                line_start INTEGER NOT NULL,
                line_end INTEGER NOT NULL,
                confidence_basis_points INTEGER NOT NULL
            );
            INSERT INTO code_repository_files VALUES ('scope', 'a.rs', 'rust', 0);
            INSERT INTO code_repository_files VALUES ('scope', 'b.rs', 'rust', 0);
            INSERT INTO code_repository_imports
            VALUES ('repo', 'scope', 'a', NULL, 'external', 'a.rs', 1, 1, 9000);
            INSERT INTO code_repository_imports
            VALUES ('repo', 'scope', 'b', NULL, 'external', 'b.rs', 1, 1, 9000);
            ",
        )
        .expect("import evidence fixture should initialize");

    let error = match import_evidence(&connection, "scope", 1) {
        Ok(_) => panic!("two imports should exceed a one-row cap"),
        Err(error) => error,
    };

    assert!(matches!(error, StorageError::CapacityExceeded(message)
        if message.contains("import evidence")));
}

#[test]
fn import_evidence_excludes_generated_rows_without_removing_code_facts() {
    let connection = Connection::open_in_memory().expect("database should open");
    connection
        .execute_batch(
            "
            CREATE TABLE code_repository_files (
                source_scope TEXT NOT NULL,
                path TEXT NOT NULL,
                language_id TEXT NOT NULL,
                is_generated INTEGER NOT NULL DEFAULT 0
            );
            CREATE TABLE code_repository_imports (
                repository_id TEXT NOT NULL,
                source_scope TEXT NOT NULL,
                module TEXT NOT NULL,
                target_hint TEXT,
                resolution_state TEXT NOT NULL,
                path TEXT NOT NULL,
                line_start INTEGER NOT NULL,
                line_end INTEGER NOT NULL,
                confidence_basis_points INTEGER NOT NULL
            );
            INSERT INTO code_repository_files
            VALUES ('scope', 'dist/vendor.min.js', 'javascript', 1);
            INSERT INTO code_repository_files
            VALUES ('scope', 'src/app.js', 'javascript', 0);
            ",
        )
        .expect("import evidence fixture should initialize");
    let oversized = "x".repeat(32 * 1_024 + 1);
    connection
        .execute(
            "INSERT INTO code_repository_imports
             VALUES ('repo', 'scope', ?1, ?1, 'external',
                     'dist/vendor.min.js', 1, 1, 9000)",
            params![oversized],
        )
        .expect("generated import should insert");
    connection
        .execute(
            "INSERT INTO code_repository_imports
             VALUES ('repo', 'scope', 'react', 'react', 'external',
                     'src/app.js', 1, 1, 9000)",
            [],
        )
        .expect("handwritten import should insert");

    let imports = import_evidence(&connection, "scope", 1)
        .expect("generated evidence should not consume the dependency matching cap");
    let code_import_count = connection
        .query_row("SELECT COUNT(*) FROM code_repository_imports", [], |row| {
            row.get::<_, i64>(0)
        })
        .expect("code import count should load");

    assert_eq!(imports.len(), 1);
    assert_eq!(imports[0].evidence_path, "src/app.js");
    assert_eq!(code_import_count, 2);
}

fn usage_schema() -> Connection {
    let connection = Connection::open_in_memory().expect("database should open");
    connection
        .execute_batch(
            "
            CREATE TABLE software_global_status (
                source_scope TEXT PRIMARY KEY,
                stale INTEGER NOT NULL,
                last_error TEXT
            );
            ",
        )
        .expect("status schema should initialize");
    super::super::schema::initialize_schema(&connection)
        .expect("dependency usage schema should initialize");
    connection
}

fn usage(language_id: &str, evidence_path: &str, package_name: &str) -> SoftwareDependencyUsage {
    SoftwareDependencyUsage {
        usage_id: format!("{language_id}:{package_name}"),
        component_id: format!("component:{package_name}"),
        repository_id: "repo".to_owned(),
        source_scope: "scope".to_owned(),
        ecosystem: language_id.to_owned(),
        package_name: package_name.to_owned(),
        language_id: language_id.to_owned(),
        module: package_name.to_owned(),
        target_hint: Some(package_name.to_owned()),
        resolution_state: "unresolved".to_owned(),
        evidence_path: evidence_path.to_owned(),
        evidence_line_range: RepositoryCodeRange { start: 3, end: 3 },
        confidence_basis_points: 8_500,
        created_graph_version: GraphVersion::new(7),
    }
}
