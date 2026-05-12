use super::*;

#[test]
fn fallback_evidence_id_disambiguates_embedded_newlines() {
    let first = generated_evidence_id("a", None, None, "b\nc");
    let second = generated_evidence_id("a\nb", None, None, "c");

    assert_ne!(first, second);
}

#[test]
fn fallback_evidence_id_is_stable_for_same_scope_and_content() {
    let first = generated_evidence_id("docs", None, None, "Rust graph idempotency");
    let second = generated_evidence_id("docs", None, None, "Rust graph idempotency");

    assert_eq!(first, second);
}
