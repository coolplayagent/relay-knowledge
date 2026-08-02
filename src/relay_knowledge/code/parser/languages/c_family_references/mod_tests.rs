use super::*;

#[test]
fn reference_nodes_and_names_accept_only_supported_c_identifier_surfaces() {
    for kind in [
        "identifier",
        "field_identifier",
        "namespace_identifier",
        "type_identifier",
    ] {
        assert!(c_family_reference_node(kind));
    }
    assert!(!c_family_reference_node("number_literal"));

    for name in ["handler", "_handler2", "SDK_TYPE"] {
        assert!(c_family_reference_name(name));
    }
    for name in ["", "2handler", "handler-name", "类型"] {
        assert!(!c_family_reference_name(name));
    }
}
