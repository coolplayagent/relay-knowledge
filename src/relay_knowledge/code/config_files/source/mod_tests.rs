use super::{
    push_boolean_definition, push_definition, source_lines, strip_inline_hash_comment, unquote,
};
use crate::code::config_files::model::{ConfigRange, ConfigValueKind};

#[test]
fn source_lines_preserve_crlf_byte_offsets_and_line_numbers() {
    let lines = source_lines("alpha\r\nbeta");

    assert_eq!(lines.len(), 2);
    assert_eq!(
        (lines[0].number, lines[0].byte_start, lines[0].byte_end),
        (1, 0, 5)
    );
    assert_eq!(
        (lines[1].number, lines[1].byte_start, lines[1].byte_end),
        (2, 7, 11)
    );
    assert_eq!(lines[1].text, "beta");
}

#[test]
fn duplicate_definitions_upgrade_unknown_values_to_boolean() {
    let range = ConfigRange {
        byte_start: 0,
        byte_end: 13,
        line_start: 1,
        line_end: 1,
    };
    let mut definitions = Vec::new();

    push_definition(&mut definitions, "enabled", "config_key", range);
    push_boolean_definition(&mut definitions, "enabled", "config_key", range);

    assert_eq!(definitions.len(), 1);
    assert_eq!(definitions[0].value_kind, ConfigValueKind::Boolean);
}

#[test]
fn text_normalization_respects_quoted_comment_markers() {
    assert_eq!(unquote("  'feature.name'  "), "feature.name");
    assert_eq!(
        strip_inline_hash_comment("value = \"#literal\" # comment"),
        "value = \"#literal\" "
    );
}
