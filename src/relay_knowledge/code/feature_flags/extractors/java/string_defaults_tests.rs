use super::*;

#[test]
fn decodes_java_unicode_octal_and_standard_escapes_in_runtime_order() {
    for (source, expected) in [
        (r#""\u0074rue""#, "true"),
        (r#""\uuuu0074rue""#, "true"),
        (r#""\164rue""#, "true"),
        (r#""\400""#, " 0"),
        (r#""\u005cn""#, "\n"),
        (r#""\u005c\u006e""#, "\n"),
        (r#""\b\t\n\f\r\s\"\'\\""#, "\u{8}\t\n\u{c}\r \"'\\"),
        (r#""\uD83D\uDE00""#, "😀"),
        ("\"日本語😀\"", "日本語😀"),
        ("\"\"", ""),
    ] {
        assert_eq!(decode(source).as_deref(), Some(expected), "{source}");
    }
}

#[test]
fn ineligible_backslashes_preserve_literal_unicode_spelling() {
    assert_eq!(decode(r#""\\u0074rue""#).as_deref(), Some(r"\u0074rue"));
    assert_eq!(decode(r#""\\\u0074rue""#).as_deref(), Some(r"\true"));
}

#[test]
fn unrepresentable_invalid_and_oversized_defaults_remain_unknown() {
    for source in [
        r#""\uD800""#,
        r#""\u007""#,
        r#""\uZZZZ""#,
        r#""\q""#,
        r#""\u000a""#,
        r#""\u005cu0074""#,
        "\"\"\"text\"\"\"",
        "unquoted",
        "\"trailing\\\"",
        "\"raw\"quote\"",
    ] {
        assert!(decode(source).is_none(), "{source}");
    }
    assert!(decode(&format!("\"{}\"", "x".repeat(MAX_LITERAL_SOURCE_BYTES))).is_none());
}
