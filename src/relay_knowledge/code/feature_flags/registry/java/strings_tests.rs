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
        assert_eq!(decode(raw).as_deref(), Some(expected), "{raw}");
    }
    for raw in [r"\u12", r"\uZZZZ", r"\uD800", r"\q", "\\", "\n", "\r", "\""] {
        assert!(decode(raw).is_none(), "{raw:?}");
    }
    assert!(decode(&"a".repeat(65_537)).is_none());
}
