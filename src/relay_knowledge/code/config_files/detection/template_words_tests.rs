use super::*;
#[test]
fn go_words_accept_identifiers_fields_and_complete_numeric_literals() {
    for word in [
        "18446744073709551616i",
        "0x1p2i",
        "18446744073709551615",
        "0xffffffffffffffff",
        "+9223372036854775807",
        "-9223372036854775808",
        "key",
        "true",
        "nil",
        "_value",
        "配置",
        ".",
        ".Values.enabled",
        "0",
        "123",
        "1_234",
        "0xff",
        "0x_FF",
        "0o77",
        "0b101",
        "0755",
        "1.25",
        ".5",
        "1.",
        "1e-3",
        "0x1.fp2",
        "3i",
        "08i",
    ] {
        assert!(valid(word), "{word}");
    }
}
#[test]
fn malformed_numeric_and_field_words_cannot_prove_valid_recovery() {
    for word in [
        "0x1i",
        "0b1i",
        "18446744073709551616",
        "999999999999999999999999",
        "0x10000000000000000",
        "+9223372036854775808",
        "-9223372036854775809",
        "123abc",
        "0x_.1p1",
        "1e309",
        "0x1p9999",
        "1e",
        "1__0",
        "1._0",
        "0x",
        "0x_",
        "0b2",
        "08",
        "1.2.3",
        "0x1.2",
        ".Values..flag",
        ".1flag",
        "abc-def",
        "١value",
        "",
    ] {
        assert!(!valid(word), "{word}");
    }
}
