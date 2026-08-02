use rusqlite::Connection;

use crate::domain::GraphVersion;

use super::{collect, initialize_schema};
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
    let mut targets = Vec::new();

    collect(&document, GraphVersion::new(7), &mut targets).expect("targets should collect");

    assert_eq!(targets.len(), 2);
    assert!(
        targets
            .iter()
            .any(|target| target.name == "relay-web" && target.kind == "package")
    );
    assert!(targets.iter().any(|target| {
        target.name == "build"
            && target.kind == "script"
            && target.command.as_deref() == Some("vite build")
    }));
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
