//! Regression tests for cross-style identifier equivalence.

use super::identifier_terms_equivalent;

#[test]
fn identifier_equivalence_normalizes_case_and_regular_plural_forms() {
    assert!(identifier_terms_equivalent("Policies", "policy"));
    assert!(identifier_terms_equivalent("worker", "Workers"));
    assert!(identifier_terms_equivalent("Routes", "routes"));
}

#[test]
fn identifier_equivalence_rejects_unsafe_singularization_shapes() {
    for (candidate, token) in [
        ("series", "serie"),
        ("species", "specie"),
        ("status", "statu"),
        ("a_b", "a_b_s"),
        ("索引", "索引s"),
    ] {
        assert!(
            !identifier_terms_equivalent(candidate, token),
            "unexpected equivalence: {candidate} ~ {token}"
        );
    }
}
