//! Direct alias-generation invariants.

use super::*;

#[test]
fn generates_searchable_identifier_aliases() {
    let aliases = lexical_aliases(&["GraphRAGContextPack", "relay-knowledge"]);

    assert!(aliases.contains("graph rag context pack"));
    assert!(aliases.contains("grcp"));
    assert!(aliases.contains("relay knowledge"));
    assert!(aliases.contains("rk"));
}
