//! Direct tests for imported-symbol target uniqueness.

use std::collections::BTreeMap;

use super::resolve_name_in_paths;
use crate::{
    domain::RepositoryCodeRange,
    storage::sqlite::code::batch::finalize::{imports::ImportResolution, symbols::SymbolKey},
};

#[test]
fn symbol_target_resolution_distinguishes_unique_and_ambiguous_matches() {
    let symbols = BTreeMap::from([(
        "Widget".to_owned(),
        vec![symbol("src/one.rs", "one"), symbol("src/two.rs", "two")],
    )]);

    assert_eq!(
        resolve_name_in_paths("Widget", &["src/one.rs".to_owned()], &symbols),
        ImportResolution::Resolved("Widget".to_owned())
    );
    assert_eq!(
        resolve_name_in_paths(
            "Widget",
            &["src/one.rs".to_owned(), "src/two.rs".to_owned()],
            &symbols,
        ),
        ImportResolution::Ambiguous
    );
}

fn symbol(path: &str, id: &str) -> SymbolKey {
    SymbolKey {
        byte_range: RepositoryCodeRange { start: 0, end: 1 },
        symbol_snapshot_id: format!("symbol:{id}"),
        path: path.to_owned(),
        name: "Widget".to_owned(),
    }
}
