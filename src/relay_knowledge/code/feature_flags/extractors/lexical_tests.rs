use super::{find_pattern_with_quotes, push_unique, valid_source_key};

#[test]
fn pattern_scan_skips_quoted_source_text() {
    let line = r#"log("config.get(\"ignored\")"); config.get("active")"#;

    let found =
        find_pattern_with_quotes(line, "config.get(", 0, |value| matches!(value, '"' | '\''));

    assert_eq!(found, line.rfind("config.get("));
}

#[test]
fn source_keys_enforce_length_and_character_boundaries() {
    assert!(valid_source_key("checkout.beta:enabled"));
    assert!(!valid_source_key(""));
    assert!(!valid_source_key("checkout flag"));
    assert!(!valid_source_key(&"x".repeat(161)));
}

#[test]
fn unique_insertion_preserves_first_seen_order() {
    let mut values = vec!["first".to_owned()];

    push_unique(&mut values, "second".to_owned());
    push_unique(&mut values, "first".to_owned());

    assert_eq!(values, ["first", "second"]);
}
