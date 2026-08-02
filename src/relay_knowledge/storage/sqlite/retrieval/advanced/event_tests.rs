use super::*;

#[test]
fn occurred_label_only_adds_present_timestamps() {
    assert_eq!(occurred_label(Some("2026-07-31")), " at 2026-07-31");
    assert_eq!(occurred_label(None), "");
}

#[test]
fn load_events_filters_by_graph_version_and_status() {
    let connection = Connection::open_in_memory().expect("database should open");
    connection
        .execute_batch(
            "
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
            CREATE TABLE entities (
                id TEXT PRIMARY KEY,
                label TEXT NOT NULL
            );
            INSERT INTO entities (id, label) VALUES ('entity-1', 'Relay');
            INSERT INTO graph_events VALUES
                ('visible', 'release', '2026-07-31', '[]', 9000, 'accepted', 1, 1, NULL),
                ('future', 'release', '2027-01-01', '[]', 9000, 'accepted', 2, 2, NULL),
                ('rejected', 'release', '2026-01-01', '[]', 9000, 'rejected', 1, 1, NULL);
            INSERT INTO graph_event_entities VALUES
                ('visible', 'entity-1'),
                ('future', 'entity-1'),
                ('rejected', 'entity-1');
            ",
        )
        .expect("event schema should initialize");
    let request = GraphSearchRequest {
        query: "timeline".to_owned(),
        source_scope: None,
        graph_version: crate::domain::GraphVersion::new(1),
        limit: 10,
        disabled_retriever_sources: Vec::new(),
    };

    let events = load_events(&connection, &request).expect("events should load");

    assert_eq!(events.len(), 1);
    assert_eq!(events[0].id, "visible");
    assert_eq!(events[0].labels, "Relay");
}
