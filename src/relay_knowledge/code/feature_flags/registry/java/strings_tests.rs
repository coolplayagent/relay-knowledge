use super::decode;
#[test]
fn java_escape_boundaries_and_invalid_literals() {
    for (raw, expected) in [
        (r"\uu0062", "b"),
        (r"\uD83D\uDE00", "😀"),
        (r"\\u0061", r"\u0061"),
        (r"\u005cn", "\n"),
        (r"\477", "'7"),
        (r#"\"\'\\"#, "\"'\\"),
        ("plain", "plain"),
    ] {
        assert_eq!(decode(raw, false).as_deref(), Some(expected), "{raw}");
    }
    for raw in [r"\u12", r"\uZZZZ", r"\uD800", r"\q", "\\", "\n", "\r", "\""] {
        assert!(decode(raw, false).is_none(), "{raw:?}");
    }
    assert!(decode(&"a".repeat(65_537), false).is_none());
}

#[test]
fn text_blocks_normalize_indentation_before_escapes() {
    for (raw, expected) in [
        ("\n    a\n      b\n    ", "a\n  b\n"),
        ("\r\n\ta\r\n\t", "a\n"),
        ("\n  a  ", "a"),
        ("\n  a\n", "  a\n"),
        ("\n  a\\\n  b\\s  \n  ", "ab \n"),
        ("\n  \"quoted\"\n  ", "\"quoted\"\n"),
        (r"\u000a  value\u000a  ", "value\n"),
    ] {
        assert_eq!(decode(raw, true).as_deref(), Some(expected), "{raw:?}");
    }
    for raw in ["missing newline", "bad\nvalue", "\n\\q", "\n\\uD800"] {
        assert!(decode(raw, true).is_none(), "{raw:?}");
    }
    assert!(decode(&"a".repeat(65_537), true).is_none());
}
