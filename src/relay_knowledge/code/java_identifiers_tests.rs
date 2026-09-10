use super::*;

#[test]
fn java_unicode_categories_distinguish_start_part_and_ignorable_characters() {
    for name in [
        "$x",
        "_x",
        "€uro",
        "£ound",
        "‿name",
        "e\u{301}",
        "\u{10400}name",
        "Syst\u{200c}em",
    ] {
        assert!(valid(name), "{name:?}");
    }
    for name in [
        "",
        "3name",
        "\u{301}name",
        "😀name",
        "foo.bar",
        "bad-name",
        "bad\\u0061",
        "bad\nname",
    ] {
        assert!(!valid(name), "{name:?}");
    }
    for value in [
        '\0', '\u{8}', '\u{e}', '\u{1b}', '\u{7f}', '\u{9f}', '\u{200c}',
    ] {
        assert!(is_ignorable(value));
        assert!(is_part(value));
        assert!(!is_start(value));
    }
    for value in ['\t', '\n', '\r', '\u{1c}', '\u{a0}'] {
        assert!(!is_ignorable(value));
    }
}
