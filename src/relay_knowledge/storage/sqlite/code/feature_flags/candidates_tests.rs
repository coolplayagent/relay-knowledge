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

#[test]
fn metadata_intersection_precedes_seed_admission_when_each_filter_alone_is_broad() {
    let store = crate::storage::SqliteGraphStore::open_in_memory().unwrap();
    let mut connection = store.connection.lock().unwrap();
    // Match the deferred-index prerequisite enforced by public query admission.
    use crate::storage::sqlite::code::schema;
    assert!(schema::require_feature_flag_query_index(&connection).is_err());
    schema::ensure_code_query_indexes(&connection).unwrap();
    schema::require_feature_flag_query_index(&connection).unwrap();
    connection.execute_batch("PRAGMA foreign_keys=OFF").unwrap();
    let mut records = Vec::new();
    for n in 0..1100 {
        let mut text_only = record(
            &format!("text-{n}"),
            &format!("aaa_needle_{n:04}"),
            "config_key",
        );
        text_only.metadata.domain = Some("other".into());
        text_only.metadata.source_format = "yaml".into();
        text_only.metadata.hot_reload = Some(false);
        records.push(text_only);
        let mut metadata_only = record(
            &format!("metadata-{n}"),
            &format!("aab_unrelated_{n:04}"),
            "config_key",
        );
        metadata_only.metadata.domain = Some("selected".into());
        metadata_only.metadata.source_format = "properties".into();
        metadata_only.metadata.hot_reload = Some(true);
        records.push(metadata_only);
    }
    let mut target = record("target", "zzz_needle", "config_key");
    target.metadata.domain = Some("selected".into());
    target.metadata.source_format = "properties".into();
    target.metadata.hot_reload = Some(true);
    records.push(target);
    let tx = connection.transaction().unwrap();
    super::super::insert_records(&tx, &records).unwrap();
    tx.commit().unwrap();
    for metadata_filter in 0..4 {
        let mut query = request();
        query.query = Some("needle".into());
        query.limit = 1;
        if metadata_filter == 0 || metadata_filter == 3 {
            query.domain = Some("selected".into());
        }
        if metadata_filter == 1 || metadata_filter == 3 {
            query.source = Some("properties".into());
        }
        if metadata_filter == 2 || metadata_filter == 3 {
            query.hot_reload = Some(true);
        }
        let flags = super::super::knowledge::search(&connection, &status(), &query).unwrap();
        assert_eq!(flags.len(), 1);
        assert_eq!(flags[0].source_key, "zzz_needle");
    }
}

#[test]
fn metadata_and_query_can_occupy_different_usages_across_two_binding_hops() {
    let store = crate::storage::SqliteGraphStore::open_in_memory().unwrap();
    let mut connection = store.connection.lock().unwrap();
    connection.execute_batch("PRAGMA foreign_keys=OFF").unwrap();
    let mut definition = record("definition", "selected.key", "config_key");
    definition.metadata.bindings = vec!["First".into()];
    definition.metadata.domain = Some("selected".into());
    definition.metadata.source_format = "properties".into();
    definition.metadata.hot_reload = Some(true);
    let mut alias = record("alias", "First", "config_symbol");
    alias.metadata.bindings = vec!["Second".into()];
    let mut usage = record("usage", "Second", "config_symbol");
    usage.excerpt = "needle alpha".into();
    let mut sibling = record("sibling", "selected.key", "config_key");
    sibling.excerpt = "beta".into();
    let mut historical = definition.clone();
    historical.usage_id = "historical".into();
    historical.source_scope = "other-scope".into();
    historical.metadata.domain = Some("historical".into());
    let tx = connection.transaction().unwrap();
    super::super::insert_records(&tx, &[definition, alias, usage, sibling, historical]).unwrap();
    tx.commit().unwrap();
    let mut query = request();
    query.query = Some("needle alpha beta".into());
    query.domain = Some("selected".into());
    query.source = Some("properties".into());
    query.hot_reload = Some(true);
    query.limit = 1;
    let flags = super::super::knowledge::search(&connection, &status(), &query).unwrap();
    assert_eq!(flags.len(), 1);
    assert_eq!(flags[0].source_key, "selected.key");
    assert_eq!(flags[0].usages.len(), 4);
    query.domain = Some("historical".into());
    assert!(
        super::super::knowledge::search(&connection, &status(), &query)
            .unwrap()
            .is_empty()
    );
    query.domain = Some("selected".into());
    query.repository.path_filters = vec!["src/usage.java".into()];
    assert!(
        super::super::knowledge::search(&connection, &status(), &query)
            .unwrap()
            .is_empty()
    );
}

#[test]
fn combined_metadata_requires_one_usage_and_overflow_is_a_distinct_state() {
    let store = crate::storage::SqliteGraphStore::open_in_memory().unwrap();
    let mut connection = store.connection.lock().unwrap();
    connection.execute_batch("PRAGMA foreign_keys=OFF").unwrap();
    let mut seed = record("seed", "needle", "config_key");
    seed.metadata.domain = Some("selected".into());
    seed.metadata.bindings = vec!["First".into()];
    let mut sibling = record("sibling", "needle", "config_key");
    sibling.metadata.source_format = "properties".into();
    let mut alias = record("alias", "First", "config_symbol");
    alias.metadata.bindings = vec!["Second".into()];
    let usage = record("usage", "Second", "config_symbol");
    let tx = connection.transaction().unwrap();
    super::super::insert_records(&tx, &[seed, sibling, alias, usage]).unwrap();
    tx.commit().unwrap();
    let mut query = request();
    query.query = Some("needle".into());
    query.domain = Some("selected".into());
    query.source = Some("properties".into());
    assert!(
        super::super::knowledge::search(&connection, &status(), &query)
            .unwrap()
            .is_empty()
    );
    let plan = metadata_groups::plan(
        "flag.source_scope = ?",
        &[Value::Text("scope".into())],
        &query,
        MAX_CLOSURE_ROUNDS,
        2,
    )
    .unwrap();
    let sql = format!(
        "WITH RECURSIVE {} SELECT {} FROM code_repository_feature_flags flag WHERE flag.usage_id = 'seed'",
        plan.prefix, plan.state
    );
    let state: i64 = connection
        .query_row(&sql, params_from_iter(plan.values), |row| row.get(0))
        .unwrap();
    assert_eq!(
        state, 2,
        "an over-budget neighborhood must not become a filtered-empty result"
    );
}

#[test]
fn metadata_frontier_distinguishes_closed_cycles_from_unseen_fifth_hops() {
    for (last, expected) in [(0, 1), (4, 1), (5, 2)] {
        let store = crate::storage::SqliteGraphStore::open_in_memory().unwrap();
        let mut connection = store.connection.lock().unwrap();
        connection.execute_batch("PRAGMA foreign_keys=OFF").unwrap();
        let records = (0..=last)
            .map(|n| {
                let mut row = record(&format!("node-{n}"), &format!("Link{n}"), "config_symbol");
                // The last node has a self-loop; reverse edges also revisit
                // earlier depths. Only a previously unseen identity overflows.
                row.metadata.bindings = vec![format!("Link{}", (n + 1).min(last))];
                row.metadata.domain = (n == last).then(|| "selected".into());
                row
            })
            .collect::<Vec<_>>();
        let tx = connection.transaction().unwrap();
        super::super::insert_records(&tx, &records).unwrap();
        tx.commit().unwrap();
        let mut query = request();
        query.query = Some("Link0".into());
        query.domain = Some("selected".into());
        let plan = metadata_groups::plan(
            "flag.source_scope = ?",
            &[Value::Text("scope".into())],
            &query,
            MAX_CLOSURE_ROUNDS,
            MAX_BINDING_IDENTITIES,
        )
        .unwrap();
        let sql = format!(
            "WITH RECURSIVE {} SELECT {} FROM code_repository_feature_flags flag WHERE flag.usage_id = 'node-0'",
            plan.prefix, plan.state
        );
        let actual: i64 = connection
            .query_row(&sql, params_from_iter(plan.values), |row| row.get(0))
            .unwrap();
        assert_eq!(actual, expected, "last node {last}");
        if expected == 2 {
            let error = ranked_keys(
                &connection,
                "flag.source_scope = ?",
                &[Value::Text("scope".into())],
                &query,
            )
            .unwrap_err();
            assert!(
                error
                    .to_string()
                    .contains("metadata binding closure budget exhausted")
            );
        }
    }
}

#[test]
fn metadata_links_preserve_source_key_and_reference_with_missing_or_null_bindings() {
    for bindings in ["{}", r#"{"bindings":null}"#] {
        let store = crate::storage::SqliteGraphStore::open_in_memory().unwrap();
        let mut connection = store.connection.lock().unwrap();
        connection.execute_batch("PRAGMA foreign_keys=OFF").unwrap();
        let mut seed = record("seed", "needle", "config_key");
        seed.metadata.bindings = vec!["RawIdentity".into()];
        let owner = record("owner", "RawIdentity", "config_symbol");
        let mut reference = record("reference", "DifferentIdentity", "config_symbol");
        reference.metadata.domain = Some("selected".into());
        let tx = connection.transaction().unwrap();
        super::super::insert_records(&tx, &[seed, owner, reference]).unwrap();
        tx.commit().unwrap();
        connection.execute(
            "UPDATE code_repository_feature_flags SET metadata_json = json_set(?, '$.referenced_symbol', 'DifferentIdentity') WHERE usage_id = 'owner'",
            [bindings],
        ).unwrap();
        let mut query = request();
        query.query = Some("needle".into());
        query.domain = Some("selected".into());
        let plan = metadata_groups::plan(
            "flag.source_scope = ?",
            &[Value::Text("scope".into())],
            &query,
            MAX_CLOSURE_ROUNDS,
            MAX_BINDING_IDENTITIES,
        )
        .unwrap();
        let sql = format!(
            "WITH RECURSIVE {} SELECT {} FROM code_repository_feature_flags flag WHERE flag.usage_id = 'seed'",
            plan.prefix, plan.state
        );
        let state: i64 = connection
            .query_row(&sql, params_from_iter(plan.values), |row| row.get(0))
            .unwrap();
        assert_eq!(
            state, 1,
            "source key and redirected reference both remain reachable"
        );
    }
}

#[test]
fn full_metadata_query_retains_bound_aliases_with_broad_disjoint_noise() {
    let store = crate::storage::SqliteGraphStore::open_in_memory().unwrap();
    let mut connection = store.connection.lock().unwrap();
    // Match the deferred-index prerequisite enforced by public query admission.
    use crate::storage::sqlite::code::schema;
    assert!(schema::require_feature_flag_query_index(&connection).is_err());
    schema::ensure_code_query_indexes(&connection).unwrap();
    schema::require_feature_flag_query_index(&connection).unwrap();
    connection.execute_batch("PRAGMA foreign_keys=OFF").unwrap();
    let mut records = Vec::new();
    for n in 0..1100 {
        for (prefix, domain) in [("aaa_needle", "other"), ("aab_unrelated", "selected")] {
            let mut row = record(
                &format!("{prefix}-{n}"),
                &format!("{prefix}_{n}"),
                "config_key",
            );
            row.metadata.domain = Some(domain.into());
            row.metadata.source_format = "yaml".into();
            row.metadata.hot_reload = Some(domain == "selected");
            records.push(row);
        }
    }
    for (id, key, domain) in [
        ("target", "zzz_needle", "selected"),
        ("definition", "selected.key", "alias-selected"),
    ] {
        let mut row = record(id, key, "config_key");
        row.metadata.domain = Some(domain.into());
        row.metadata.source_format = "properties".into();
        row.metadata.hot_reload = Some(true);
        records.push(row);
    }
    let mut binding = record("binding", "selected.key", "config_key");
    binding.metadata.bindings = vec!["Keys.FIRST".into()];
    binding.excerpt = "NeedleReader".into();
    records.push(binding);
    let mut usage = record("usage", "Keys.FIRST", "config_symbol");
    usage.metadata.referenced_symbol = Some("Keys.FIRST".into());
    usage.excerpt = "NeedleReader".into();
    records.push(usage);
    let tx = connection.transaction().unwrap();
    super::super::insert_records(&tx, &records).unwrap();
    tx.commit().unwrap();
    for (domain, source, hot_reload, expected) in [
        (Some("selected"), None, None, vec!["zzz_needle"]),
        (
            None,
            Some("properties"),
            None,
            vec!["selected.key", "zzz_needle"],
        ),
        (None, None, Some(true), vec!["selected.key", "zzz_needle"]),
        (
            Some("selected"),
            Some("properties"),
            Some(true),
            vec!["zzz_needle"],
        ),
        (
            Some("alias-selected"),
            Some("properties"),
            None,
            vec!["selected.key"],
        ),
    ] {
        let mut query = request();
        query.query = Some("needle".into());
        query.domain = domain.map(str::to_owned);
        query.source = source.map(str::to_owned);
        query.hot_reload = hot_reload;
        query.limit = 50;
        // Exercise the complete ranked SQL, hydration and final matching under
        // one production VM budget, including query evidence on the alias.
        let flags = super::super::knowledge::search(&connection, &status(), &query).unwrap();
        let actual = flags
            .iter()
            .map(|flag| flag.source_key.as_str())
            .collect::<BTreeSet<_>>();
        assert_eq!(
            actual,
            expected.into_iter().collect(),
            "{domain:?}/{source:?}/{hot_reload:?}"
        );
    }
}
