use super::*;
#[test]
fn signed_literals_preserve_java_integer_boundaries_and_float_values() {
    for (kind, raw, sign, expected) in [
        ("decimal_integer_literal", "1", "-", "-1"),
        ("decimal_integer_literal", "10L", "+", "10"),
        ("decimal_integer_literal", "2147483648", "-", "-2147483648"),
        (
            "decimal_integer_literal",
            "9223372036854775808L",
            "-",
            "-9223372036854775808",
        ),
        ("hex_integer_literal", "0xffffffff", "-", "1"),
        ("hex_integer_literal", "0x80000000", "-", "-2147483648"),
        ("decimal_floating_point_literal", "1.25f", "-", "-1.25"),
        ("decimal_integer_literal", "1L", "-", "-1"),
    ] {
        assert_eq!(signed_literal(kind, raw, sign).as_deref(), Some(expected));
    }
    assert!(signed_literal("decimal_integer_literal", "1", "!").is_none());
    assert!(signed_literal("decimal_integer_literal", "bad", "-").is_none());
    assert!(signed_literal("decimal_integer_literal", &"1".repeat(257), "-").is_none());
}
#[test]
fn numeric_literals_preserve_values_across_java_spellings_and_bounds() {
    for (kind, raw, expected) in [
        ("decimal_integer_literal", "1_000", "1000"),
        ("decimal_integer_literal", "10L", "10"),
        ("hex_integer_literal", "0xff", "255"),
        ("hex_integer_literal", "0xffff_ffff", "-1"),
        ("binary_integer_literal", "0b1010L", "10"),
        ("octal_integer_literal", "012", "10"),
        ("decimal_floating_point_literal", "1_0.0D", "10"),
        ("decimal_floating_point_literal", "1e3f", "1000"),
    ] {
        assert_eq!(literal(kind, raw).as_deref(), Some(expected));
    }
    for (kind, raw) in [
        ("decimal_integer_literal", "2147483648"),
        ("decimal_integer_literal", "9223372036854775808L"),
        ("hex_integer_literal", "0x100000000"),
        ("decimal_floating_point_literal", "1e999"),
        ("decimal_floating_point_literal", "1e999f"),
        ("decimal_integer_literal", "bad"),
        ("unknown", "1"),
    ] {
        assert!(literal(kind, raw).is_none());
    }
    assert!(literal("decimal_integer_literal", &"1".repeat(257)).is_none());
}
