use std::collections::BTreeMap;

use super::resolve;
use crate::storage::sqlite::code::batch::finalize::imports::ImportResolution;

#[test]
fn quoted_include_prefers_the_importer_directory() {
    let paths = BTreeMap::from([(
        "src/api/widget.h".to_owned(),
        vec!["src/api/widget.h".to_owned()],
    )]);

    assert_eq!(
        resolve("src/api/client.c", "#include \"widget.h\"", &paths),
        ImportResolution::Resolved("src/api/widget.h".to_owned())
    );
}

#[test]
fn include_resolution_rejects_non_include_statements() {
    assert_eq!(
        resolve("src/api/client.c", "define WIDGET", &BTreeMap::new()),
        ImportResolution::Unresolved
    );
}
