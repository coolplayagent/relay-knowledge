use super::*;

#[test]
fn enum_member_nodes_are_limited_to_supported_language_shapes() {
    assert!(enum_member_node("c", "enumerator"));
    assert!(enum_member_node("cpp", "enumerator"));
    assert!(enum_member_node("rust", "enum_variant"));
    assert!(!enum_member_node("rust", "enumerator"));
    assert!(!enum_member_node("java", "enum_constant"));
}

#[test]
fn enum_member_names_follow_language_identifier_rules() {
    for name in ["Ready", "r#match", "状态"] {
        assert!(enum_member_name("rust", name));
    }
    for name in ["READY", "_ready2"] {
        assert!(enum_member_name("c", name));
    }
    assert!(!enum_member_name("rust", "2Ready"));
    assert!(!enum_member_name("c", "状态"));
}
