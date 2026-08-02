use std::collections::BTreeMap;

use super::resolve;
use crate::storage::sqlite::code::batch::finalize::imports::ImportResolution;

#[test]
fn go_import_resolves_a_unique_package_directory() {
    let paths = BTreeMap::from([(
        "example.org/client/api.go".to_owned(),
        vec!["vendor/example.org/client/api.go".to_owned()],
    )]);

    assert_eq!(
        resolve("import \"example.org/client\"", &paths),
        ImportResolution::Resolved("vendor/example.org/client".to_owned())
    );
}

#[test]
fn go_import_rejects_an_empty_specifier() {
    assert_eq!(
        resolve("import \"\"", &BTreeMap::new()),
        ImportResolution::Unresolved
    );
}
