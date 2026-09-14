use rusqlite::Connection;

use crate::domain::GraphVersion;

use super::{
    BuildTargets, MAX_BUILD_TARGETS_PER_SCOPE, collect, existing_maven_build_targets,
    initialize_schema,
};
use crate::storage::StorageError;
use crate::storage::sqlite::software::lifecycle::document::{IndexedDocument, IndexedLine};

#[test]
fn package_manifest_collection_keeps_nonempty_scripts() {
    let document = document(
        "package.json",
        "json",
        &[
            r#""name": "relay-web","#,
            r#""scripts": {"#,
            r#"  "build": "vite build","#,
            r#"  "empty": "","#,
            "}",
        ],
    );
    let mut targets = BuildTargets::new(MAX_BUILD_TARGETS_PER_SCOPE, "build targets");

    collect(&document, GraphVersion::new(7), &mut targets).expect("targets should collect");

    assert_eq!(targets.as_slice().len(), 2);
    assert!(
        targets
            .as_slice()
            .iter()
            .any(|target| target.name == "relay-web" && target.kind == "package")
    );
    assert!(targets.as_slice().iter().any(|target| {
        target.name == "build"
            && target.kind == "script"
            && target.command.as_deref() == Some("vite build")
    }));
}

#[test]
fn dockerfile_is_a_build_definition_instead_of_an_iac_resource() {
    let document = document(
        "Dockerfile",
        "dockerfile",
        &["FROM rust:1.88 AS builder", "RUN cargo build --release"],
    );
    let mut targets = BuildTargets::new(MAX_BUILD_TARGETS_PER_SCOPE, "build targets");

    collect(&document, GraphVersion::new(7), &mut targets).expect("target should collect");

    assert_eq!(targets.as_slice().len(), 1);
    assert_eq!(targets.as_slice()[0].ecosystem, "container");
    assert_eq!(targets.as_slice()[0].kind, "definition");
    assert_eq!(
        targets.as_slice()[0].command.as_deref(),
        Some("FROM rust:1.88 AS builder")
    );
}

#[test]
fn initialize_schema_creates_build_target_lookup_index() {
    let connection = Connection::open_in_memory().expect("sqlite should open");

    initialize_schema(&connection).expect("build schema should initialize");

    let index_count = connection
        .query_row(
            "
            SELECT COUNT(*)
            FROM sqlite_master
            WHERE type = 'index'
              AND name = 'software_build_targets_scope'
            ",
            [],
            |row| row.get::<_, usize>(0),
        )
        .expect("index count should load");
    assert_eq!(index_count, 1);
}

#[test]
fn existing_maven_targets_reject_cap_plus_one_at_sql_boundary() {
    let connection = Connection::open_in_memory().expect("sqlite should open");
    initialize_schema(&connection).expect("build schema should initialize");
    for index in 0..2 {
        connection
            .execute(
                "INSERT INTO software_build_targets (
                    target_id, repository_id, source_scope, ecosystem, language_id, name, kind,
                    command, output_hint, source_kind, evidence_path, evidence_line_start,
                    evidence_line_end, confidence_basis_points, created_graph_version
                 ) VALUES (?1, 'repo', 'scope', 'maven', 'java', ?2, 'project', NULL, NULL,
                    'pom.xml', 'pom.xml', 1, 1, 9000, 1)",
                rusqlite::params![format!("target-{index}"), format!("name-{index}")],
            )
            .expect("Maven target should insert");
    }

    let error = existing_maven_build_targets(&connection, "scope", 1)
        .expect_err("two persisted targets should exceed a one-row cap");

    assert!(matches!(error, StorageError::CapacityExceeded(message)
        if message.contains("existing Maven build targets")));
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
