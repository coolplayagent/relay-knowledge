use super::*;
use crate::storage::sqlite::code::feature_flags::{
    insert_records, knowledge,
    test_support::{record, request, status},
};

#[test]
fn declaration_provenance_follows_resolved_aliases_in_the_actual_namespace() {
    let mut read = record("read", "checkout", "env_var");
    read.metadata.referenced_symbol = Some("demo.Config.get".to_owned());
    let mut getter = record("getter", "checkout", "env_var");
    getter.metadata.bindings = vec!["demo.Config.get".to_owned()];
    getter.metadata.referenced_symbol = Some("demo.Keys.FLAG".to_owned());
    let literal = record("literal", "checkout", "config_key");
    let mut unknown = record("unknown", "checkout", "env_var");
    unknown.metadata.referenced_symbol = Some("demo.Labels.TEXT".to_owned());
    let used = used_symbols(
        &[read, getter, literal, unknown],
        &[
            "resolved".into(),
            "resolved".into(),
            "literal".into(),
            "ambiguous".into(),
        ],
    );
    assert_eq!(used.len(), 1);
    let symbols = &used[&("env_var".to_owned(), "checkout".to_owned())];
    assert!(symbols.contains("demo.Config.get"));
    assert!(symbols.contains("demo.Keys.FLAG"));
    assert!(!symbols.contains("demo.Labels.TEXT"));
}

#[test]
fn same_value_constants_require_actual_reference_and_literal_reads_do_not_promote_them() {
    let store = crate::storage::SqliteGraphStore::open_in_memory().unwrap();
    let mut connection = store.connection.lock().unwrap();
    connection.execute_batch("PRAGMA foreign_keys=OFF").unwrap();
    let mut used = record("used", "checkout", "config_key");
    used.edge_kind = "binds_config_symbol".to_owned();
    used.metadata.bindings = vec!["demo.Keys.FLAG".to_owned()];
    let mut unused = used.clone();
    unused.usage_id = "unused".to_owned();
    unused.path = "Labels.java".to_owned();
    unused.metadata.bindings = vec!["demo.Labels.TEXT".to_owned()];
    let mut getter = record("getter", "demo.Keys.FLAG", "config_symbol");
    getter.metadata.bindings = vec!["demo.Config.get".to_owned()];
    getter.metadata.read_source_kind = Some(crate::domain::CodeConfigurationReadKind::EnvVar);
    let tx = connection.transaction().unwrap();
    insert_records(
        &tx,
        &[
            used,
            unused,
            getter,
            record("read", "demo.Config.get", "config_getter"),
        ],
    )
    .unwrap();
    tx.commit().unwrap();
    let mut query = request();
    query.consistency = true;
    let flags = knowledge::search(&connection, &status(), &query).unwrap();
    assert_eq!(flags.len(), 1);
    assert_eq!(flags[0].source_kind, "env_var");
    let declarations = flags[0]
        .usages
        .iter()
        .filter(|u| u.edge_kind == "declares_config_key")
        .collect::<Vec<_>>();
    assert_eq!(declarations.len(), 1);
    assert_eq!(declarations[0].metadata.bindings, ["demo.Keys.FLAG"]);
    connection
        .execute(
            "DELETE FROM code_repository_feature_flags WHERE usage_id IN ('getter', 'read')",
            [],
        )
        .unwrap();
    let tx = connection.transaction().unwrap();
    insert_records(&tx, &[record("literal", "checkout", "config_key")]).unwrap();
    tx.commit().unwrap();
    let flags = knowledge::search(&connection, &status(), &query).unwrap();
    assert_eq!(flags.len(), 1);
    assert_eq!(flags[0].usages.len(), 1);
    assert_eq!(flags[0].usages[0].edge_kind, "reads_config");
}
