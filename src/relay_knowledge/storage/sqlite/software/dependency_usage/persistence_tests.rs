use rusqlite::Connection;

use super::*;
use crate::domain::{CodeRepositorySelector, FreshnessPolicy, SoftwareGlobalKind};

#[test]
fn usage_persistence_round_trip_filters_and_deletes_scope() {
    let connection = usage_schema();
    insert_usage(&connection, &usage("rust", "src/lib.rs", "serde"))
        .expect("Rust usage should insert");
    insert_usage(&connection, &usage("python", "scripts/tool.py", "requests"))
        .expect("Python usage should insert");
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
