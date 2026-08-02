//! Direct tests for bounded call-target disambiguation.

use super::{CallTargetSymbol, unique_preferred_callable};

#[test]
fn preferred_callable_selects_the_only_definition_over_declarations() {
    let symbols = [
        symbol("declaration", "function", "int connect();"),
        symbol("definition", "function", "int connect() {"),
    ];

    let preferred = unique_preferred_callable(&symbols).expect("definition should be unique");

    assert_eq!(preferred.symbol_snapshot_id, "definition");
}

fn symbol(symbol_snapshot_id: &str, kind: &str, signature: &str) -> CallTargetSymbol {
    CallTargetSymbol {
        symbol_snapshot_id: symbol_snapshot_id.to_owned(),
        path: "src/client.c".to_owned(),
        kind: kind.to_owned(),
        signature: signature.to_owned(),
    }
}
