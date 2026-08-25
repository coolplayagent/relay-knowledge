use rusqlite::Connection;

use super::*;

#[test]
fn empty_component_set_short_circuits_before_import_reads() {
    let connection = Connection::open_in_memory().expect("database should open");

    let usages = derive_dependency_usages(&connection, "scope", GraphVersion::new(1), &[])
        .expect("empty matching index should not require import tables");

    assert!(usages.is_empty());
}

#[test]
fn dependency_usage_output_rejects_cap_plus_one_without_duplicate_growth() {
    let mut usages = Vec::new();
    let mut seen = BTreeSet::new();
    let first = usage("first");
    insert_bounded_usage(&mut usages, &mut seen, first.clone(), 1).expect("first usage should fit");
    insert_bounded_usage(&mut usages, &mut seen, first, 1)
        .expect("duplicate usage should not consume capacity");

    let error = insert_bounded_usage(&mut usages, &mut seen, usage("second"), 1)
        .expect_err("unique cap plus one should fail");

    assert!(matches!(error, StorageError::CapacityExceeded(message)
        if message.contains("dependency usages")));
    assert_eq!(usages.len(), 1);
}

fn usage(id: &str) -> SoftwareDependencyUsage {
    SoftwareDependencyUsage {
        usage_id: id.to_owned(),
        component_id: "component".to_owned(),
        repository_id: "repo".to_owned(),
        source_scope: "scope".to_owned(),
        ecosystem: "cargo".to_owned(),
        package_name: "package".to_owned(),
        language_id: "rust".to_owned(),
        module: "package".to_owned(),
        target_hint: None,
        resolution_state: "external".to_owned(),
        evidence_path: "src/lib.rs".to_owned(),
        evidence_line_range: crate::domain::RepositoryCodeRange { start: 1, end: 1 },
        confidence_basis_points: 9_000,
        created_graph_version: GraphVersion::new(1),
    }
}
