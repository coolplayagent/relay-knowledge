use rusqlite::Connection;

use crate::domain::GraphVersion;

use super::{collect, initialize_schema, new_elements};
use crate::storage::sqlite::software::lifecycle::document::{IndexedDocument, IndexedLine};

#[test]
fn collectors_extract_documented_architecture_and_manifest_module() {
    let mut elements = new_elements();
    collect(
        &document(
            "docs/architecture.md",
            "markdown",
            &["# Runtime Architecture", "", "Coordinates query workers."],
        ),
        GraphVersion::new(5),
        &mut elements,
    )
    .expect("markdown design should collect");
    collect(
        &document(
            "Cargo.toml",
            "toml",
            &["[package]", "name = \"relay-core\""],
        ),
        GraphVersion::new(5),
        &mut elements,
    )
    .expect("manifest design should collect");

    assert!(elements.as_slice().iter().any(|element| {
        element.element_kind == "architecture"
            && element.name == "Runtime Architecture"
            && element.summary.as_deref() == Some("Coordinates query workers.")
    }));
    assert!(elements.as_slice().iter().any(|element| {
        element.element_kind == "module"
            && element.name == "relay-core"
            && element.summary.as_deref() == Some("rust package/module boundary")
    }));
}

#[test]
fn readme_headings_stay_document_topics_unless_metadata_declares_an_entity() {
    let mut elements = new_elements();
    collect(
        &document(
            "README.md",
            "markdown",
            &["# Getting Started", "## Chapter Index"],
        ),
        GraphVersion::new(5),
        &mut elements,
    )
    .expect("generic README headings should be ignored by design promotion");
    assert!(elements.as_slice().is_empty());

    collect(
        &document(
            "docs/catalog.md",
            "markdown",
            &[
                "",
                "---",
                "software-system: relay-knowledge",
                "api: Graph API",
                "---",
                "# Guide",
            ],
        ),
        GraphVersion::new(5),
        &mut elements,
    )
    .expect("explicit metadata should collect");
    assert!(elements.as_slice().iter().any(|element| {
        element.element_kind == "software_system"
            && element.name == "relay-knowledge"
            && element.source_kind == "markdown-metadata"
    }));
    assert!(
        elements
            .as_slice()
            .iter()
            .any(|element| element.element_kind == "api" && element.name == "Graph API")
    );
}

#[test]
fn initialize_schema_creates_design_lookup_index() {
    let connection = Connection::open_in_memory().expect("sqlite should open");

    initialize_schema(&connection).expect("design schema should initialize");

    let index_count = connection
        .query_row(
            "
            SELECT COUNT(*)
            FROM sqlite_master
            WHERE type = 'index'
              AND name = 'software_design_elements_scope'
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
