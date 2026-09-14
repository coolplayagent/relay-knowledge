use super::decode;
#[test]
fn go_literal_byte_unicode_and_invalid_escape_boundaries() {
    for (raw, expected) in [
        (r"\xC3\xA9", "é"),
        (r"\u0061\U0001F600", "a😀"),
        (r"\141", "a"),
        (r#"\a\b\f\n\r\t\v\\\""#, "\u{7}\u{8}\u{c}\n\r\t\u{b}\\\""),
    ] {
        assert_eq!(decode(raw).as_deref(), Some(expected));
    }
    for raw in [
        r"\xFF",
        r"\uD800",
        r"\UFFFFFFFF",
        r"\400",
        r"\1",
        r"\xG0",
        r"\q",
        "\\",
        "\n",
        "\r",
        "\"",
    ] {
        assert!(decode(raw).is_none(), "{raw:?}");
    }
    assert!(decode(&"x".repeat(65_537)).is_none());
}
