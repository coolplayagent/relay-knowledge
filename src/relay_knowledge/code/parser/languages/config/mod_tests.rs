use super::*;

#[test]
fn definition_kinds_are_limited_to_supported_configuration_nodes() {
    assert_eq!(definition_kind("json", "pair"), Some("config"));
    assert_eq!(definition_kind("ini", "section"), Some("section"));
    assert_eq!(definition_kind("markdown", "atx_heading"), Some("heading"));
    assert_eq!(
        definition_kind("toml", "table_array_element"),
        Some("section")
    );
    assert_eq!(definition_kind("yaml", "flow_pair"), Some("config"));
    assert_eq!(definition_kind("json", "array"), None);
    assert_eq!(definition_kind("rust", "pair"), None);
}

#[test]
fn markdown_heading_normalization_requires_visible_content() {
    assert_eq!(
        nonempty_markdown_heading("  First line  \n\n second line "),
        Some("First line second line".to_owned())
    );
    assert_eq!(nonempty_markdown_heading(" \n\t"), None);
}
