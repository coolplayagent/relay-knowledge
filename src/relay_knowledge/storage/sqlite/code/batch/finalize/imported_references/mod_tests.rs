//! Direct tests for imported-symbol uniqueness and target-path matching.

use super::{super::symbols::SymbolKey, symbols_by_name, unique_imported_symbol};
use crate::domain::RepositoryCodeRange;

#[test]
fn imported_symbol_resolution_requires_one_name_and_target_path_match() {
    let symbols = [
        symbol("protocol", "src/protocol.ts", "RuntimeClient"),
        symbol("other", "src/other.ts", "RuntimeClient"),
    ];
    let by_name = symbols_by_name(&symbols);

    let resolved = unique_imported_symbol(&by_name, "src/protocol.ts", "RuntimeClient")
        .expect("target path should disambiguate the imported name");
    assert_eq!(resolved.symbol_snapshot_id, "protocol");

    let ambiguous_symbols = [
        symbol("first", "src/protocol.ts", "RuntimeClient"),
        symbol("second", "src/protocol.ts", "RuntimeClient"),
    ];
    let ambiguous_by_name = symbols_by_name(&ambiguous_symbols);
    assert!(
        unique_imported_symbol(&ambiguous_by_name, "src/protocol.ts", "RuntimeClient").is_none()
    );
}

fn symbol(symbol_snapshot_id: &str, path: &str, name: &str) -> SymbolKey {
    SymbolKey {
        symbol_snapshot_id: symbol_snapshot_id.to_owned(),
        path: path.to_owned(),
        name: name.to_owned(),
        line_range: RepositoryCodeRange { start: 1, end: 2 },
    }
}
