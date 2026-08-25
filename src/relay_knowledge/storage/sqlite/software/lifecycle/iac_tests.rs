use rusqlite::Connection;

use crate::domain::GraphVersion;

use super::{collect, initialize_schema, new_resources};
use crate::storage::sqlite::software::lifecycle::document::{IndexedDocument, IndexedLine};

#[test]
fn collectors_extract_container_and_kubernetes_resources() {
    let mut resources = new_resources();
    collect(
        &document(
            "Dockerfile",
            "dockerfile",
            &["FROM rust:1.88 AS builder", "EXPOSE 8080"],
        ),
        GraphVersion::new(3),
        &mut resources,
    )
    .expect("container resources should collect");
    collect(
        &document(
            "deploy/app.yaml",
            "yaml",
            &["kind: Deployment", "metadata:", "  name: relay-api"],
        ),
        GraphVersion::new(3),
        &mut resources,
    )
    .expect("kubernetes resources should collect");

    assert!(resources.as_slice().iter().any(|resource| {
        resource.provider == "container"
            && resource.resource_kind == "base_image"
            && resource.target_hint.as_deref() == Some("rust:1.88")
    }));
    assert!(resources.as_slice().iter().any(|resource| {
        resource.provider == "kubernetes"
            && resource.resource_kind == "Deployment"
            && resource.name == "relay-api"
    }));
}

#[test]
fn initialize_schema_creates_iac_lookup_index() {
    let connection = Connection::open_in_memory().expect("sqlite should open");

    initialize_schema(&connection).expect("iac schema should initialize");

    let index_count = connection
        .query_row(
            "
            SELECT COUNT(*)
            FROM sqlite_master
            WHERE type = 'index'
              AND name = 'software_iac_resources_scope'
            ",
            [],
            |row| row.get::<_, usize>(0),
        )
        .expect("index count should load");
    assert_eq!(index_count, 1);
}

fn document(path: &str, language_id: &str, lines: &[&str]) -> IndexedDocument {
    IndexedDocument {
        repository_id: "repo".to_owned(),
        source_scope: "scope".to_owned(),
        path: path.to_owned(),
        language_id: language_id.to_owned(),
        lines: lines
            .iter()
            .enumerate()
            .map(|(index, text)| IndexedLine {
                number: index as u32 + 1,
                text: (*text).to_owned(),
            })
            .collect(),
    }
}
