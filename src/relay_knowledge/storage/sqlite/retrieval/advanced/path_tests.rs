use super::*;

#[test]
fn path_candidates_compose_empty_relation_claim_and_event_scans() {
    let connection = Connection::open_in_memory().expect("database should open");
    connection
        .execute_batch(
            "
            CREATE TABLE entities (id TEXT PRIMARY KEY, label TEXT NOT NULL);
            CREATE TABLE graph_relations (
                id TEXT PRIMARY KEY,
                source_entity_id TEXT NOT NULL,
                relation_type TEXT NOT NULL,
                target_entity_id TEXT NOT NULL,
                evidence_ids_json TEXT NOT NULL,
                confidence_basis_points INTEGER NOT NULL,
                status TEXT NOT NULL,
                created_graph_version INTEGER NOT NULL,
                valid_from_graph_version INTEGER NOT NULL,
                valid_until_graph_version INTEGER
            );
            CREATE TABLE graph_claims (
                id TEXT PRIMARY KEY,
                subject_entity_id TEXT NOT NULL,
                predicate TEXT NOT NULL,
                object TEXT NOT NULL,
                evidence_ids_json TEXT NOT NULL,
                confidence_basis_points INTEGER NOT NULL,
                status TEXT NOT NULL,
                created_graph_version INTEGER NOT NULL,
                valid_from_graph_version INTEGER NOT NULL,
                valid_until_graph_version INTEGER
            );
            CREATE TABLE graph_events (
                id TEXT PRIMARY KEY,
                event_type TEXT NOT NULL,
                occurred_at TEXT,
                evidence_ids_json TEXT NOT NULL,
                confidence_basis_points INTEGER NOT NULL,
                status TEXT NOT NULL,
                created_graph_version INTEGER NOT NULL,
                valid_from_graph_version INTEGER NOT NULL,
                valid_until_graph_version INTEGER
            );
            CREATE TABLE graph_event_entities (
                event_id TEXT NOT NULL,
                entity_id TEXT NOT NULL
            );
            ",
        )
        .expect("path schema should initialize");
    let request = GraphSearchRequest {
        query: "relay".to_owned(),
        source_scope: None,
        graph_version: crate::domain::GraphVersion::new(1),
        limit: 10,
        disabled_retriever_sources: Vec::new(),
    };

    let hits = path_candidates(&connection, &request).expect("path scan should succeed");

    assert!(hits.is_empty());
}
