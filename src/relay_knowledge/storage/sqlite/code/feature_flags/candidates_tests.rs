use super::super::test_support::{record, request, status};
use super::*;

#[test]
fn feature_flag_query_work_budget_ignores_metadata_only_seed_matches() {
    let store = crate::storage::SqliteGraphStore::open_in_memory().unwrap();
    let mut connection = store.connection.lock().unwrap();
    connection.execute_batch("PRAGMA foreign_keys=OFF").unwrap();
    let mut records = (0..1100)
        .map(|n| {
            let mut row = record(&format!("noise-{n}"), &format!("aaa{n:04}"), "config_key");
            row.metadata.domain = Some("needle".into());
            row.metadata.default_value = Some("needle".into());
            row.metadata.source_format = "needle".into();
            row.metadata.value_type = Some("needle".into());
            row.metadata.read_usage_id = Some("needle".into());
            row
        })
        .collect::<Vec<_>>();
    records.push(record("target", "zzzneedle", "config_key"));
    let tx = connection.transaction().unwrap();
    super::super::insert_records(&tx, &records).unwrap();
    tx.commit().unwrap();
    let mut query = request();
    query.query = Some("needle".into());
    query.limit = 1;
    let result = super::super::knowledge::search(&connection, &status(), &query).unwrap();
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].source_key, "zzzneedle");
    query.query = None;
    query.domain = Some("needle".into());
    assert_eq!(load(&connection, &status(), &query).unwrap().0.len(), 1);
}

#[test]
fn projected_binding_and_reference_text_keeps_unicode_and_literal_underscores() {
    let store = crate::storage::SqliteGraphStore::open_in_memory().unwrap();
    let mut connection = store.connection.lock().unwrap();
    connection.execute_batch("PRAGMA foreign_keys=OFF").unwrap();
    let mut binding = record("binding", "bound", "config_key");
    binding.metadata.bindings = vec!["demo.配置_FLAG".into()];
    let mut reference = record("reference", "referenced", "config_key");
    reference.metadata.referenced_symbol = Some("demo.配置_FLAG".into());
    let mut other = record("other", "other", "config_key");
    other.metadata.bindings = vec!["demo.配置XFLAG".into()];
    let tx = connection.transaction().unwrap();
    super::super::insert_records(&tx, &[binding, reference, other]).unwrap();
    tx.commit().unwrap();
    let mut query = request();
    query.query = Some("配置_FLAG".into());
    let rows = load(&connection, &status(), &query).unwrap().0;
    assert_eq!(rows.len(), 2);
    assert!(rows.iter().all(|row| row.source_key != "other"));
}

#[test]
fn exact_key_ignores_more_than_ten_thousand_unrelated_usages() {
    let store = crate::storage::SqliteGraphStore::open_in_memory().unwrap();
    let mut connection = store.connection.lock().unwrap();
    connection
        .execute_batch("PRAGMA foreign_keys=OFF;")
        .unwrap();
    let tx = connection.transaction().unwrap();
    let mut target = record("target", "feature_wanted", "config_key");
    target.metadata.domain = Some("task".to_owned());
    target.metadata.bindings = vec!["demo.Keys.WANTED".to_owned()];
    super::super::insert_records(
        &tx,
        &[
            record("noise", "feature_unrelated", "config_key"),
            target,
            record("read", "demo.Keys.WANTED", "config_symbol"),
            record("copied-old-id", "feature_wanted", "config_key"),
        ],
    )
    .unwrap();
    tx.commit().unwrap();
    connection.execute_batch("WITH RECURSIVE sequence(n) AS (SELECT 1 UNION ALL SELECT n+1 FROM sequence WHERE n < 11000)
      INSERT INTO code_repository_feature_flags SELECT repository_id, source_scope, 'noise-' || n,
      'noise-usage-' || n, file_id, path, language_id, name, source_kind, source_key, edge_kind,
      confidence_basis_points, confidence_tier, byte_start, byte_end, line_start, line_end, excerpt, metadata_json
      FROM code_repository_feature_flags, sequence WHERE usage_id = 'noise';").unwrap();
    let mut query = request();
    query.query = Some("feature_wanted".to_owned());
    query.limit = 1;
    let rows = load(&connection, &status(), &query).unwrap().0;
    assert_eq!(rows.len(), 3);
    assert!(rows.iter().any(|r| r.usage_id == "read"));
    query.query = None;
    query.domain = Some("task".to_owned());
    assert_eq!(load(&connection, &status(), &query).unwrap().0.len(), 3);
    query.consistency = true;
    assert!(
        load(&connection, &status(), &query)
            .unwrap_err()
            .to_string()
            .contains("incomplete")
    );
}

#[test]
fn closure_loads_both_directions_and_never_crosses_snapshot_or_path_scope() {
    let store = crate::storage::SqliteGraphStore::open_in_memory().unwrap();
    let mut connection = store.connection.lock().unwrap();
    connection
        .execute_batch("PRAGMA foreign_keys=OFF;")
        .unwrap();
    let mut key = record("key", "toggle", "config_key");
    key.metadata.bindings = vec!["demo.Keys.KEY".to_owned()];
    let mut getter = record("getter", "demo.Keys.KEY", "config_symbol");
    getter.metadata.bindings = vec!["demo.Config.getToggle".to_owned()];
    let usage = record("usage", "demo.Config.getToggle", "config_symbol");
    let mut outside = key.clone();
    outside.source_scope = "historical".to_owned();
    outside.source_key = "wrong".to_owned();
    let tx = connection.transaction().unwrap();
    super::super::insert_records(&tx, &[key, getter, usage, outside]).unwrap();
    tx.commit().unwrap();
    for text in ["toggle", "demo.Config.getToggle"] {
        let mut query = request();
        query.query = Some(text.to_owned());
        query.limit = 1;
        assert_eq!(load(&connection, &status(), &query).unwrap().0.len(), 3);
    }
    let mut query = request();
    query.query = Some("demo.Config.getToggle".to_owned());
    query.repository.path_filters = vec!["src/usage.java".to_owned()];
    let rows = load(&connection, &status(), &query).unwrap().0;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].source_kind, "config_symbol");
}
