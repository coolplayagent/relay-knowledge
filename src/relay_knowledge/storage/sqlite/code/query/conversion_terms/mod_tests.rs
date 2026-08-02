//! Regression tests for the conversion-action vocabulary.

use super::conversion_action_term;

#[test]
fn conversion_action_terms_accept_only_bounded_action_vocabulary() {
    for term in [
        "adapt",
        "adapts",
        "convert",
        "conversion",
        "format",
        "formats",
        "map",
        "maps",
        "normalize",
        "normalized",
        "transform",
        "translate",
    ] {
        assert!(conversion_action_term(term), "expected action term: {term}");
    }
    for term in ["", "adapter", "converter", "mapping", "normalizer"] {
        assert!(
            !conversion_action_term(term),
            "unexpected action term: {term}"
        );
    }
}
