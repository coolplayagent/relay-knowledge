use std::collections::HashMap;

use super::{ActiveMacroDefinition, evaluate_if_condition};

fn object_macro(replacement: &str) -> ActiveMacroDefinition {
    ActiveMacroDefinition {
        replacement: replacement.to_owned(),
        function_like: false,
    }
}

#[test]
fn condition_evaluation_respects_precedence_defined_and_integer_radices() {
    let macros = HashMap::from([
        ("FEATURE".to_owned(), object_macro("1")),
        ("VERSION".to_owned(), object_macro("0x3U")),
    ]);

    assert!(evaluate_if_condition(
        "defined(FEATURE) && VERSION >= 03 && !0",
        &macros
    ));
    assert!(!evaluate_if_condition(
        "defined MISSING || VERSION < 0b10",
        &macros
    ));
}

#[test]
fn condition_evaluation_resolves_object_macros_and_rejects_cycles() {
    let resolved = HashMap::from([
        ("FIRST".to_owned(), object_macro("SECOND")),
        ("SECOND".to_owned(), object_macro("2")),
    ]);
    assert!(evaluate_if_condition("FIRST == 2", &resolved));

    let cyclic = HashMap::from([
        ("FIRST".to_owned(), object_macro("SECOND")),
        ("SECOND".to_owned(), object_macro("FIRST")),
    ]);
    assert!(!evaluate_if_condition("FIRST", &cyclic));
}

#[test]
fn condition_evaluation_rejects_malformed_or_unclosed_input() {
    let macros = HashMap::new();

    assert!(!evaluate_if_condition("(1 && 1", &macros));
    assert!(!evaluate_if_condition("1 /* unclosed", &macros));
    assert!(!evaluate_if_condition("1 + 1", &macros));
}
