use super::*;

#[test]
fn optional_text_is_trimmed_and_empty_values_are_rejected() {
    assert_eq!(
        normalize_optional("hint", Some("  value  ".to_owned()))
            .expect("text should validate")
            .as_deref(),
        Some("value")
    );
    assert_eq!(
        normalize_optional("hint", Some(" ".to_owned()))
            .expect_err("empty text should fail")
            .field,
        "hint"
    );
}

#[test]
fn software_identity_is_stable_and_part_order_is_significant() {
    let first = stable_software_id("topic", ["scope", "name"]);
    let repeated = stable_software_id("topic", ["scope", "name"]);
    let reordered = stable_software_id("topic", ["name", "scope"]);

    assert_eq!(first, repeated);
    assert_ne!(first, reordered);
}

#[test]
fn confidence_accepts_basis_point_bounds_only() {
    assert_eq!(validate_confidence(10_000), Ok(10_000));
    assert_eq!(
        validate_confidence(10_001)
            .expect_err("confidence above the bound should fail")
            .field,
        "confidence"
    );
}
