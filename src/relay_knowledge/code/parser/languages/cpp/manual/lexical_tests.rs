use super::*;

#[test]
fn head_tokens_preserve_identifier_text_and_byte_spans() {
    let head = "API class outer::Widget";
    let tokens = cpp_head_tokens(head);

    assert_eq!(
        tokens
            .iter()
            .map(|token| (token.text, token.start, token.end))
            .collect::<Vec<_>>(),
        [
            ("API", 0, 3),
            ("class", 4, 9),
            ("outer", 10, 15),
            ("Widget", 17, 23)
        ]
    );
    assert!(cpp_tokens_joined_by_qualifier(head, tokens[2], tokens[3]));
    assert!(!cpp_tokens_joined_by_qualifier(head, tokens[0], tokens[1]));
}

#[test]
fn type_name_candidates_reject_keywords_builtins_and_invalid_identifiers() {
    assert!(cpp_type_name_candidate("Widget_2"));
    assert!(cpp_type_name_candidate("_Widget"));
    assert!(!cpp_type_name_candidate("class"));
    assert!(!cpp_type_name_candidate("unsigned"));
    assert!(!cpp_type_name_candidate("2Widget"));
    assert!(!cpp_type_name_candidate("Widget-name"));
}

#[test]
fn decorator_and_declaration_prefix_rules_cover_supported_shapes() {
    assert!(cpp_decorator_token("__attribute__"));
    assert!(cpp_decorator_token("WIDGET_API"));
    assert!(cpp_decorator_token("UPPER_CASE_2"));
    assert!(!cpp_decorator_token("mixed_case"));
    assert!(cpp_declaration_prefix_token("constexpr"));
    assert!(cpp_declaration_prefix_token("always_inline"));
    assert!(!cpp_declaration_prefix_token("Widget"));
    assert!(cpp_type_name_decorator_prefix("WIDGET_EXPORT"));
    assert!(cpp_decorator_payload_token("visibility"));
}

#[test]
fn type_intro_and_builtin_rules_are_explicit() {
    assert!(cpp_type_intro_keyword("class"));
    assert!(cpp_type_intro_keyword("union"));
    assert!(!cpp_type_intro_keyword("namespace"));
    assert!(cpp_builtin_type_token("char16_t"));
    assert!(cpp_builtin_type_token("void"));
    assert!(!cpp_builtin_type_token("Widget"));
}
