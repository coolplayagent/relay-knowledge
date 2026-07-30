use super::*;

#[test]
fn accepts_external_and_qualified_initializer_declarations() {
    assert!(c_family_typedef_like_initializer_declaration(
        "ExternalType value = { 0 };"
    ));
    assert!(c_family_typedef_like_initializer_declaration(
        "sdk::ExternalType value = { 0 };"
    ));
}

#[test]
fn rejects_missing_declarator_or_non_aggregate_initializer() {
    assert!(!c_family_typedef_like_initializer_declaration(
        "int = { 0 };"
    ));
    assert!(!c_family_typedef_like_initializer_declaration(
        "ExternalType value = make_value();"
    ));
}
