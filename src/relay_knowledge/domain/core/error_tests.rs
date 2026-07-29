use super::*;

#[test]
fn displays_field_and_message() {
    let error = DomainError::invalid("field", "failed");

    assert_eq!(error.to_string(), "field: failed");
}
