use super::{SymbolKey, caller_for_line};
use crate::domain::RepositoryCodeRange;

#[test]
fn caller_lookup_uses_sorted_prefix_and_prefers_innermost_symbol() {
    let symbols = [
        symbol("outer", 10, 100),
        symbol("same_start_outer", 20, 80),
        symbol("same_start_inner", 20, 40),
        symbol("after_call", 60, 70),
    ];
    let symbol_refs = symbols.iter().collect::<Vec<_>>();

    let caller = caller_for_line(Some(&symbol_refs), 30).expect("caller should match");

    assert_eq!(caller.name, "same_start_inner");
}

#[test]
fn caller_lookup_ignores_symbols_that_start_after_call_line() {
    let symbols = [symbol("before", 1, 5), symbol("after", 20, 30)];
    let symbol_refs = symbols.iter().collect::<Vec<_>>();

    assert!(caller_for_line(Some(&symbol_refs), 10).is_none());
}

fn symbol(name: &str, start: u32, end: u32) -> SymbolKey {
    SymbolKey {
        symbol_snapshot_id: format!("symbol:{name}"),
        path: "src/lib.rs".to_owned(),
        name: name.to_owned(),
        signature: format!("fn {name}()"),
        line_range: RepositoryCodeRange { start, end },
    }
}
