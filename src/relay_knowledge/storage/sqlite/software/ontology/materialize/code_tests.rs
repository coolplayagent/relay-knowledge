use super::*;
use crate::domain::GraphVersion;

#[test]
fn source_io_configuration_without_display_name_keeps_entity_and_evidence() {
    let connection = Connection::open_in_memory().unwrap();
    connection
        .execute_batch(
            "CREATE TABLE code_repository_feature_flags (
        source_scope TEXT, feature_flag_id TEXT, path TEXT, language_id TEXT, name TEXT,
        source_kind TEXT, source_key TEXT, edge_kind TEXT, confidence_basis_points INTEGER,
        line_start INTEGER, line_end INTEGER);
        INSERT INTO code_repository_feature_flags VALUES
        ('scope', 'flag', 'config/pattern.properties', 'properties', '', 'config_key', '*',
         'defines_config', 7500, 2, 2);",
        )
        .unwrap();
    let mut builder =
        OntologyBuilder::new("repo".into(), "scope".into(), GraphVersion::ZERO).unwrap();
    builder
        .add_file(
            "file",
            "config/pattern.properties",
            "properties",
            "configuration",
            "parsed",
        )
        .unwrap();
    collect_configurations(&connection, &mut builder).unwrap();
    let entity = builder
        .entities
        .iter()
        .find(|entity| entity.entity_kind == SoftwareEntityKind::Configuration)
        .unwrap();
    assert_eq!(entity.name, "*");
    assert_eq!(entity.attributes["source_key"], "*");
    assert!(!entity.attributes.contains_key("display_name"));
    assert_eq!(entity.evidence_refs.len(), 1);
    assert!(
        builder
            .statements
            .iter()
            .any(|statement| statement.predicate == SoftwarePredicate::Configures)
    );
}
