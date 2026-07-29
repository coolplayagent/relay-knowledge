use super::javascript_regex_literal_can_start;

#[test]
fn regex_literal_can_start_after_expression_openers_and_keywords() {
    for prefix in ["", "value =", "return ", "case ", "await "] {
        assert!(
            javascript_regex_literal_can_start(prefix),
            "{prefix:?} should allow a regex literal"
        );
    }
}

#[test]
fn division_context_does_not_start_a_regex_literal() {
    for prefix in ["value", "value)", "items[index]"] {
        assert!(
            !javascript_regex_literal_can_start(prefix),
            "{prefix:?} should remain division context"
        );
    }
}
