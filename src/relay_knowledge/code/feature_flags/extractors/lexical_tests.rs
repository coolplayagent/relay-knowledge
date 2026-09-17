use super::{find_pattern_with_quotes, push_unique, valid_source_key};

#[test]
fn configuration_scan_work_suite_skips_quote_work_for_absent_patterns() {
    use std::cell::Cell;

    let line = "fn unrelated(input: u64) { let text = \"没有配置\"; } ".repeat(4096);
    let visited = Cell::new(0usize);
    for pattern in [
        ".variation(",
        ".getBooleanValue(",
        "std::env::var(",
        "config.get(",
    ] {
        assert_eq!(
            find_pattern_with_quotes(&line, pattern, 0, |character| {
                visited.set(visited.get() + 1);
                matches!(character, '\'' | '"' | '`')
            }),
            None
        );
    }
    assert_eq!(
        visited.get(),
        0,
        "absent APIs must not decode the line per pattern"
    );
}

#[test]
fn pattern_scan_preserves_unicode_offsets_quotes_and_empty_patterns() {
    let samples = [
        "",
        "ααcall(\"key\")",
        "'call(x)' call(y)",
        "`call(x)` call(y)",
        "\"escaped\\\" call(x)\" call(y)",
        "\"unterminated call(x)",
        "call(call(x))",
    ];
    for line in samples {
        for pattern in ["call(", "α", "'", "", "missing"] {
            for start in 0..=line.len() + 1 {
                for backticks in [false, true] {
                    let quote = |c| matches!(c, '\'' | '"') || (backticks && c == '`');
                    let mut quoted = None;
                    let mut escaped = false;
                    let expected = line.char_indices().find_map(|(index, c)| {
                        if let Some(delimiter) = quoted {
                            if escaped {
                                escaped = false;
                            } else if c == '\\' {
                                escaped = true;
                            } else if c == delimiter {
                                quoted = None;
                            }
                        } else {
                            if index >= start && line[index..].starts_with(pattern) {
                                return Some(index);
                            }
                            if quote(c) {
                                quoted = Some(c);
                            }
                        }
                        None
                    });
                    assert_eq!(
                        find_pattern_with_quotes(line, pattern, start, quote),
                        expected,
                        "{line:?} {pattern:?} {start} {backticks}"
                    );
                }
            }
        }
    }
}

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
