use rusqlite::params;

use super::*;
use crate::storage::{CodeRepositorySetEdgeSelector, SqliteGraphStore};

use super::super::super::capacity::MAX_OVERLAY_EDGE_SELECTOR_KEYS;
use super::super::super::tests::support::{insert_repository_scope, member_seed, set_seed};

#[tokio::test]
async fn projection_returns_only_candidate_origin_and_target_edges() {
    let store = SqliteGraphStore::open_in_memory().expect("store should open");
    let edges = store
        .run(|connection| {
            insert_repository_scope(connection, "repo-a", "app", "scope-a", "tree-a", false)?;
            insert_repository_scope(connection, "repo-b", "sdk", "scope-b", "tree-b", false)?;
            let set = super::super::super::create_set(connection, set_seed("workspace", 10))?;
            super::super::super::add_member(
                connection,
                member_seed("workspace", "repo-a", "app", "scope-a", 10),
            )?;
            super::super::super::add_member(
                connection,
                member_seed("workspace", "repo-b", "sdk", "scope-b", 0),
            )?;
            insert_edge(
                connection,
                &set.set_id,
                "edge-origin",
                "import-origin",
                "file-scope-b",
                r#"{"from_path":"src/app.rs","from_line_start":1,"from_line_end":1}"#,
            )?;
            insert_edge(
                connection,
                &set.set_id,
                "edge-target",
                "import-target",
                "file-scope-b",
                r#"{"from_path":"src/other.rs","from_line_start":2,"from_line_end":2}"#,
            )?;
            insert_edge(
                connection,
                &set.set_id,
                "edge-noise",
                "import-noise",
                "noise-file",
                r#"{"from_path":"src/noise.rs","from_line_start":3,"from_line_end":3}"#,
            )?;
            insert_edge(
                connection,
                &set.set_id,
                "edge-invalid-json",
                "import-invalid",
                "invalid-file",
                "not-json",
            )?;

            cross_edges_for_selector(
                connection,
                &set.set_id,
                &CodeRepositorySetEdgeSelector {
                    origin_files: vec![("scope-a".to_owned(), "src/app.rs".to_owned())],
                    target_records: vec![(
                        "scope-b".to_owned(),
                        "code_file".to_owned(),
                        "file-scope-b".to_owned(),
                    )],
                },
            )
        })
        .await
        .expect("candidate edges should query");

    assert_eq!(
        edges
            .iter()
            .map(|edge| edge.edge_id.as_str())
            .collect::<Vec<_>>(),
        vec!["edge-origin", "edge-target"]
    );
}

#[test]
fn selector_values_are_bounded_rows() {
    assert_eq!(selector_values_sql(2, 3), "(?, ?, ?), (?, ?, ?)");
}

#[tokio::test]
async fn projection_rejects_selector_cap_plus_one_before_querying_edges() {
    let store = SqliteGraphStore::open_in_memory().expect("store should open");
    let selector = CodeRepositorySetEdgeSelector {
        origin_files: (0..=MAX_OVERLAY_EDGE_SELECTOR_KEYS)
            .map(|index| (format!("scope-{index}"), format!("path-{index}")))
            .collect(),
        target_records: Vec::new(),
    };
    let error = store
        .run(move |connection| cross_edges_for_selector(connection, "set", &selector))
        .await
        .expect_err("selector cap plus one must be rejected");

    assert!(matches!(
        error,
        crate::storage::StorageError::CapacityExceeded(_)
    ));
}

fn insert_edge(
    connection: &rusqlite::Connection,
    set_id: &str,
    edge_id: &str,
    from_record_id: &str,
    to_record_id: &str,
    evidence_json: &str,
) -> Result<(), crate::storage::StorageError> {
    connection.execute(
        "
        INSERT INTO code_repository_cross_edges (
            edge_id, set_id, from_source_scope, from_repository_id, from_record_kind,
            from_record_id, to_source_scope, to_repository_id, to_record_kind, to_record_id,
            edge_kind, resolution_state, confidence_basis_points, confidence_tier,
            evidence_json, created_at_ms
        )
        VALUES (?1, ?2, 'scope-a', 'repo-a', 'module_reference',
                ?3, 'scope-b', 'repo-b', 'code_file', ?4,
                'imports', 'resolved', 10000, 'explicit', ?5, 20)
        ",
        params![edge_id, set_id, from_record_id, to_record_id, evidence_json],
    )?;
    Ok(())
}
