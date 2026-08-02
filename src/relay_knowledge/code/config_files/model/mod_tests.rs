use super::{ConfigRange, ConfigValueKind};

#[test]
fn unknown_is_the_safe_default_for_untyped_configuration_values() {
    assert_eq!(ConfigValueKind::default(), ConfigValueKind::Unknown);
    assert_ne!(ConfigValueKind::Unknown, ConfigValueKind::Boolean);
}

#[test]
fn ranges_keep_byte_and_line_boundaries_independent() {
    let range = ConfigRange {
        byte_start: 4,
        byte_end: 17,
        line_start: 2,
        line_end: 3,
    };

    assert_eq!(range.byte_start, 4);
    assert_eq!(range.byte_end, 17);
    assert_eq!((range.line_start, range.line_end), (2, 3));
}
