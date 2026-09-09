use super::*;

#[test]
fn go_string_escapes_decode_bytes_controls_unicode_and_literal_backslashes() {
    for (literal, value) in [
        (r#""\x74rue""#, "true"),
        (r#""\164rue""#, "true"),
        (r#""\xc3\xa9""#, "é"),
        (r#""\u4e2d\U0001f600""#, "中😀"),
        (r#""\a\b\f\n\r\t\v\\\"""#, "\x07\x08\x0c\n\r\t\x0b\\\""),
        ("`first\r\nsecond\rthird`", "first\nsecondthird"),
    ] {
        assert_eq!(
            string(&format!("{literal} rest")),
            Some((value.to_owned(), " rest")),
            "{literal}"
        );
    }
}

#[test]
fn invalid_go_escapes_and_non_utf8_bytes_remain_unknown_without_lossy_replacement() {
    for literal in [
        r#""\xff""#,
        r#""\377""#,
        r#""\400""#,
        r#""\x1""#,
        r#""\12""#,
        r#""\uD800""#,
        r#""\U00110000""#,
        r#""\q""#,
        r#""\'""#,
        "\"line\nbreak\"",
        "\"unterminated",
        "`unterminated",
        "dynamic",
    ] {
        assert!(string(literal).is_none(), "{literal}");
    }
}
