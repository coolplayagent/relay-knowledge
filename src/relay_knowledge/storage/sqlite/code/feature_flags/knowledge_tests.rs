use super::super::test_support::{record, request, status};
use super::*;

#[test]
fn resolves_exact_bindings_in_two_hops_and_preserves_ambiguity() {
    let mut definition = record("definition", "toggle", "config_key");
    definition.metadata.bindings = vec!["a.Keys.KEY".to_owned()];
    let mut getter = record("getter", "a.Keys.KEY", "config_symbol");
    getter.metadata.bindings = vec!["a.Config.getToggle".to_owned()];
    let usage = record("usage", "a.Config.getToggle", "config_symbol");
    let external = record("external", "foreign.Keys.KEY", "config_symbol");
    let mut rows = vec![usage, getter, definition.clone(), external];
    assert_eq!(
        resolve(&mut rows),
        vec!["resolved", "resolved", "literal", "unresolved"]
    );
    assert_eq!(rows[0].source_key, "toggle");
    let mut duplicate = definition.clone();
    duplicate.source_key = "different".to_owned();
    let mut ambiguous = vec![
        record("read", "a.Keys.KEY", "config_symbol"),
        definition,
        duplicate,
    ];
    assert_eq!(resolve(&mut ambiguous)[0], "ambiguous");
    assert_eq!(ambiguous[0].source_kind, "config_symbol");
}

#[test]
fn scoped_query_round_trips_metadata_filters_and_deletion_without_live_source() {
    let store = crate::storage::SqliteGraphStore::open_in_memory().unwrap();
    let mut connection = store.connection.lock().unwrap();
    connection
        .execute_batch("PRAGMA foreign_keys=OFF;")
        .unwrap();
    let mut definition = record("definition", "toggle", "config_key");
    definition.edge_kind = "declares_config_key".to_owned();
    definition.metadata.bindings = vec!["a.Keys.KEY".to_owned()];
    definition.metadata.domain = Some("task".to_owned());
    definition.metadata.hot_reload = Some(true);
    let usage = record("usage", "a.Keys.KEY", "config_symbol");
    let mut template = record("template", "other", "config_key");
    template.edge_kind = "defines_config".to_owned();
    template.metadata.source_format = "ctmpl".to_owned();
    let mut historical = definition.clone();
    historical.source_scope = "old".to_owned();
    historical.source_key = "old_toggle".to_owned();
    let tx = connection.transaction().unwrap();
    super::super::insert_records(&tx, &[definition, usage, template, historical]).unwrap();
    tx.commit().unwrap();
    let query = request()
        .with_metadata_filters(
            Some("task".to_owned()),
            Some("java".to_owned()),
            Some(true),
            true,
        )
        .unwrap();
    let groups = search(&connection, &status(), &query).unwrap();
    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].source_key, "toggle");
    assert_eq!(groups[0].usages.len(), 2);
    assert!(
        groups[0]
            .consistency_diagnostics
            .iter()
            .any(|s| s.contains("ctmpl"))
    );
    connection.execute("DELETE FROM code_repository_feature_flags WHERE source_scope = 'scope' AND usage_id = 'definition'", []).unwrap();
    let groups = search(&connection, &status(), &request()).unwrap();
    assert!(groups.iter().any(|g| g.source_kind == "config_symbol"));
    assert!(!groups.iter().any(|g| g.source_key == "old_toggle"));
}

#[test]
fn degraded_scope_and_unknown_metadata_never_claim_proven_absence() {
    let mut group = CodeFeatureFlagGraph {
        feature_flag_id: "flag".to_owned(),
        name: "flag".to_owned(),
        source_kind: "config_key".to_owned(),
        source_key: "flag".to_owned(),
        score: 0.0,
        usages: Vec::new(),
        consistency_diagnostics: Vec::new(),
        analysis_complete: true,
    };
    let mut status = status();
    status.degraded_reason = Some("parser failure".to_owned());
    diagnose(&mut group, &BTreeSet::from(["ctmpl".to_owned()]), &status);
    assert!(!group.analysis_complete);
    assert_eq!(group.consistency_diagnostics.len(), 1);
    assert!(group.consistency_diagnostics[0].starts_with("unknown:"));
}

#[test]
fn bounded_scope_refuses_absence_claim_when_usage_budget_is_exhausted() {
    let store = crate::storage::SqliteGraphStore::open_in_memory().unwrap();
    let mut connection = store.connection.lock().unwrap();
    connection
        .execute_batch("PRAGMA foreign_keys=OFF;")
        .unwrap();
    let tx = connection.transaction().unwrap();
    super::super::insert_records(&tx, &[record("seed", "toggle", "config_key")]).unwrap();
    tx.commit().unwrap();
    connection.execute_batch("WITH RECURSIVE sequence(n) AS (SELECT 1 UNION ALL SELECT n+1 FROM sequence WHERE n < 10000)
      INSERT INTO code_repository_feature_flags SELECT repository_id, source_scope, feature_flag_id,
      'usage-' || n, file_id, path, language_id, name, source_kind, source_key, edge_kind,
      confidence_basis_points, confidence_tier, byte_start, byte_end, line_start, line_end, excerpt, metadata_json
      FROM code_repository_feature_flags, sequence WHERE usage_id = 'seed';").unwrap();
    let error = search(&connection, &status(), &request())
        .unwrap_err()
        .to_string();
    assert!(error.contains("incomplete"));
    assert!(error.contains("unknown"));
    let mut narrowed = request();
    narrowed.repository.path_filters = vec!["other".to_owned()];
    assert!(
        search(&connection, &status(), &narrowed)
            .unwrap()
            .is_empty()
    );
}

#[test]
fn direct_binding_never_hides_conflicting_two_hop_definition() {
    let mut direct = record("direct", "one", "config_key");
    direct.metadata.bindings = vec!["shared".to_owned()];
    let mut indirect = record("indirect", "via", "config_symbol");
    indirect.metadata.bindings = vec!["shared".to_owned()];
    let mut alternate = record("alternate", "two", "config_key");
    alternate.metadata.bindings = vec!["via".to_owned()];
    let mut rows = vec![
        record("read", "shared", "config_symbol"),
        direct,
        indirect,
        alternate,
    ];
    assert_eq!(resolve(&mut rows)[0], "ambiguous");
}

#[test]
fn pure_template_reads_count_as_format_presence_without_becoming_definitions() {
    let store = crate::storage::SqliteGraphStore::open_in_memory().unwrap();
    let mut connection = store.connection.lock().unwrap();
    connection
        .execute_batch("PRAGMA foreign_keys=OFF;")
        .unwrap();
    let mut definition = record("definition", "feature_x", "config_key");
    definition.edge_kind = "defines_config".to_owned();
    definition.metadata.source_format = "properties".to_owned();
    let mut template = record("template", "feature_x", "config_key");
    template.metadata.source_format = "ctmpl".to_owned();
    let mut missing = record("missing", "feature_y", "config_key");
    missing.edge_kind = "declares_config_key".to_owned();
    let tx = connection.transaction().unwrap();
    super::super::insert_records(&tx, &[definition, template, missing]).unwrap();
    tx.commit().unwrap();
    let mut query = request();
    query.consistency = true;
    let groups = search(&connection, &status(), &query).unwrap();
    let x = groups.iter().find(|g| g.source_key == "feature_x").unwrap();
    assert!(
        !x.consistency_diagnostics
            .iter()
            .any(|d| d.contains("ctmpl"))
    );
    let y = groups.iter().find(|g| g.source_key == "feature_y").unwrap();
    assert!(
        y.consistency_diagnostics
            .iter()
            .any(|d| d.contains("ctmpl"))
    );
}

#[test]
fn constant_candidates_surface_only_with_configuration_evidence_and_disappear_after_read_deletion()
{
    let store = crate::storage::SqliteGraphStore::open_in_memory().unwrap();
    let mut connection = store.connection.lock().unwrap();
    connection
        .execute_batch("PRAGMA foreign_keys=OFF;")
        .unwrap();
    let mut candidate = record("candidate", "feature_y", "config_key");
    candidate.edge_kind = "binds_config_symbol".to_owned();
    candidate.metadata.bindings = vec!["demo.Keys.KEY".to_owned()];
    let tx = connection.transaction().unwrap();
    super::super::insert_records(
        &tx,
        &[candidate, record("read", "demo.Keys.KEY", "config_symbol")],
    )
    .unwrap();
    tx.commit().unwrap();
    let mut query = request();
    query.query = Some("feature_y".to_owned());
    let groups = search(&connection, &status(), &query).unwrap();
    assert_eq!(groups.len(), 1);
    assert!(
        groups[0]
            .usages
            .iter()
            .any(|u| u.edge_kind == "declares_config_key")
    );
    connection
        .execute(
            "DELETE FROM code_repository_feature_flags WHERE usage_id = 'read'",
            [],
        )
        .unwrap();
    assert!(search(&connection, &status(), &query).unwrap().is_empty());
    query.consistency = true;
    assert!(search(&connection, &status(), &query).unwrap().is_empty());
}

#[test]
fn getter_candidates_require_proven_configuration_binding() {
    let store = crate::storage::SqliteGraphStore::open_in_memory().unwrap();
    let mut connection = store.connection.lock().unwrap();
    connection
        .execute_batch("PRAGMA foreign_keys=OFF;")
        .unwrap();
    let mut known = record("known", "feature_x", "config_key");
    known.metadata.bindings = vec!["demo.Config.getX".to_owned()];
    let tx = connection.transaction().unwrap();
    super::super::insert_records(
        &tx,
        &[
            known,
            record("getter", "demo.Config.getX", "config_getter"),
            record("dto", "demo.User.getName", "config_getter"),
        ],
    )
    .unwrap();
    tx.commit().unwrap();
    let groups = search(&connection, &status(), &request()).unwrap();
    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].usages.len(), 2);
    assert!(
        groups[0]
            .usages
            .iter()
            .any(|u| u.usage_id == "getter" && u.resolution_state == "resolved")
    );
    let mut query = request();
    query.consistency = true;
    assert_eq!(search(&connection, &status(), &query).unwrap().len(), 1);
}

#[test]
fn copied_and_new_occurrence_ids_for_same_key_are_not_ambiguous_bindings() {
    let mut old = record("old_occurrence", "feature", "config_key");
    old.metadata.bindings = vec!["demo.Keys.KEY".to_owned()];
    let mut new = old.clone();
    new.feature_flag_id = "new_occurrence".to_owned();
    new.usage_id = "new_usage".to_owned();
    let mut rows = vec![record("read", "demo.Keys.KEY", "config_symbol"), old, new];
    assert_eq!(resolve(&mut rows)[0], "resolved");
    assert_eq!(rows[0].source_key, "feature");
    assert_eq!(rows[0].feature_flag_id, "new_occurrence");
}

#[test]
fn typed_environment_read_projects_constant_identity_and_declaration() {
    let mut constant = record("constant", "SWITCH", "config_key");
    constant.edge_kind = "binds_config_symbol".to_owned();
    constant.metadata.bindings = vec!["demo.Keys.KEY".to_owned()];
    let mut read = record("read", "demo.Keys.KEY", "config_symbol");
    read.metadata.read_source_kind = Some(CodeConfigurationReadKind::EnvVar);
    let mut rows = vec![constant, read];
    let states = resolve(&mut rows);
    assert_eq!(states[1], "resolved");
    assert_eq!(rows[1].source_kind, "env_var");
    assert_eq!(
        rows[1].feature_flag_id,
        crate::identity::stable_id("feature_flag", ["repo", "scope", "env_var", "SWITCH"])
    );
    let projected = promote_bound_declarations(rows, states);
    assert_eq!(projected.len(), 2);
    assert!(
        projected
            .iter()
            .all(|(row, _)| row.source_kind == "env_var")
    );
    assert_eq!(
        projected[0].0.feature_flag_id,
        projected[1].0.feature_flag_id
    );
    assert_eq!(projected[0].0.edge_kind, "declares_config_key");
}

#[test]
fn declaration_projection_preserves_distinct_same_line_occurrences() {
    let mut first = record("first-occurrence", "SWITCH", "config_key");
    first.edge_kind = "binds_config_symbol".to_owned();
    first.metadata.bindings = vec!["demo.Keys.KEY".to_owned()];
    let mut second = first.clone();
    second.usage_id = "second-occurrence".to_owned();
    let mut read = record("read", "demo.Keys.KEY", "config_symbol");
    read.metadata.read_source_kind = Some(CodeConfigurationReadKind::EnvVar);
    let mut rows = vec![first, second, read];
    let states = resolve(&mut rows);
    let projected = promote_bound_declarations(rows, states);
    assert_eq!(projected.len(), 3);
    assert_ne!(projected[0].0.usage_id, projected[1].0.usage_id);
    assert!(
        projected
            .iter()
            .all(|(row, _)| row.source_kind == "env_var")
    );
}
