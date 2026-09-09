use super::super::test_support::{record, request, status};
use super::*;

#[test]
fn consistency_compares_formats_within_each_configuration_namespace() {
    let mut property = record("property", "SAME", "config_key");
    property.metadata.source_format = "properties".into();
    property.edge_kind = "defines_config".into();
    let mut environment = record("environment", "SAME", "env_var");
    environment.metadata.source_format = "shell".into();
    let mut another = record("other", "OTHER", "config_key");
    another.metadata.source_format = "ini".into();
    let mut query = request();
    query.consistency = true;
    let groups = assemble(vec![property, environment, another], &status(), &query);
    let env = groups.iter().find(|g| g.source_kind == "env_var").unwrap();
    assert!(
        env.consistency_diagnostics
            .iter()
            .all(|d| !d.contains("missing_from_format"))
    );
    let property = groups
        .iter()
        .find(|g| g.source_kind == "config_key" && g.source_key == "SAME")
        .unwrap();
    assert!(
        property
            .consistency_diagnostics
            .iter()
            .any(|d| d.contains("missing_from_format: ini"))
    );
    assert!(
        property
            .consistency_diagnostics
            .iter()
            .all(|d| !d.contains("shell"))
    );
}

#[test]
fn query_terms_and_metadata_can_come_from_separate_authorized_alias_usages() {
    let store = crate::storage::SqliteGraphStore::open_in_memory().unwrap();
    let mut connection = store.connection.lock().unwrap();
    connection.execute_batch("PRAGMA foreign_keys=OFF").unwrap();
    let mut definition = record("definition", "real_key", "config_key");
    definition.metadata.source_format = "properties".into();
    definition.metadata.bindings = vec!["demo.Keys.KEY".into()];
    let mut usage = record("read", "demo.Keys.KEY", "config_symbol");
    usage.path = "src/App.java".into();
    usage.metadata.domain = Some("read-only-domain".into());
    let tx = connection.transaction().unwrap();
    super::super::insert_records(&tx, &[definition, usage]).unwrap();
    tx.commit().unwrap();
    let mut query = request();
    query.query = Some("App.java real_key".into());
    query.source = Some("properties".into());
    assert_eq!(search(&connection, &status(), &query).unwrap().len(), 1);
    query.domain = Some("read-only-domain".into());
    assert!(search(&connection, &status(), &query).unwrap().is_empty());
    query.domain = None;
    query.repository.path_filters = vec!["src/App.java".into()];
    assert!(search(&connection, &status(), &query).unwrap().is_empty());
}

#[test]
fn result_limit_is_filled_after_multiple_high_scoring_aliases_collapse() {
    let store = crate::storage::SqliteGraphStore::open_in_memory().unwrap();
    let mut connection = store.connection.lock().unwrap();
    connection.execute_batch("PRAGMA foreign_keys=OFF").unwrap();
    let mut definition = record("definition", "alpha", "config_key");
    definition.metadata.bindings = vec!["A".into(), "B".into(), "C".into()];
    let mut records = vec![definition, record("beta", "beta", "config_key")];
    for name in ["A", "B", "C"] {
        let mut read = record(name, name, "config_symbol");
        read.edge_kind = "guards_code".into();
        records.push(read);
    }
    let tx = connection.transaction().unwrap();
    super::super::insert_records(&tx, &records).unwrap();
    tx.commit().unwrap();
    let mut query = request();
    query.limit = 2;
    let groups = search(&connection, &status(), &query).unwrap();
    assert_eq!(
        groups
            .iter()
            .map(|g| g.source_key.as_str())
            .collect::<Vec<_>>(),
        ["alpha", "beta"]
    );
}

#[test]
fn exhausted_seed_budget_does_not_report_an_unproven_empty_result() {
    let store = crate::storage::SqliteGraphStore::open_in_memory().unwrap();
    let mut connection = store.connection.lock().unwrap();
    connection.execute_batch("PRAGMA foreign_keys=OFF").unwrap();
    let records = (0..1001)
        .map(|n| {
            let mut row = record(&format!("row-{n}"), &format!("key-{n}"), "config_key");
            row.excerpt = "common".into();
            row
        })
        .collect::<Vec<_>>();
    let tx = connection.transaction().unwrap();
    super::super::insert_records(&tx, &records).unwrap();
    tx.commit().unwrap();
    let mut query = request();
    query.limit = 1;
    query.query = Some("common".into());
    query.source = Some("properties".into());
    assert!(
        search(&connection, &status(), &query)
            .unwrap_err()
            .to_string()
            .contains("1000-seed budget")
    );
}

#[test]
fn ambiguous_configuration_getter_keeps_its_callsite_and_unknown_consistency() {
    let mut first = record("first", "first_enabled", "config_key");
    first.metadata.bindings = vec!["demo.Config.getEnabled".into()];
    let mut second = record("second", "second_enabled", "config_key");
    second.metadata.bindings = first.metadata.bindings.clone();
    let mut read = record("reader", "demo.Config.getEnabled", "config_getter");
    read.metadata.referenced_symbol = Some("demo.Config.getEnabled".into());
    let mut guard = read.clone();
    guard.usage_id = "guard".into();
    guard.edge_kind = "guards_code".into();
    guard.metadata.read_usage_id = Some("reader".into());
    let dto = record("dto", "demo.Dto.getName", "config_getter");
    let mut query = request();
    query.consistency = true;
    let groups = assemble(vec![first, second, read, guard, dto], &status(), &query);
    assert_eq!(groups.len(), 3);
    let ambiguous = groups
        .iter()
        .find(|g| g.source_kind == "config_getter")
        .unwrap();
    assert_eq!(ambiguous.usages.len(), 2);
    assert!(
        ambiguous
            .usages
            .iter()
            .all(|u| u.resolution_state == "ambiguous")
    );
    assert!(!ambiguous.analysis_complete);
    assert_eq!(ambiguous.consistency_diagnostics.len(), 1);
    assert!(ambiguous.consistency_diagnostics[0].starts_with("unknown:"));
    assert_eq!(
        ambiguous.usages[0].metadata.read_usage_id.as_deref(),
        Some("reader")
    );
}
