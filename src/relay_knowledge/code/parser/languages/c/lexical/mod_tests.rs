// Direct tests for shared C lexical predicates.

use super::{c_declaration_prefix_token, c_identifier_char, data_symbol_name};

#[test]
fn data_symbol_names_require_ascii_c_identifier_shape() {
    for accepted in ["status", "_hidden", "Status2"] {
        assert!(data_symbol_name(accepted), "{accepted} should be accepted");
    }
    for rejected in ["", "2status", "status-code", "状态"] {
        assert!(!data_symbol_name(rejected), "{rejected} should be rejected");
    }
}

#[test]
fn identifier_characters_are_ascii_alphanumeric_or_underscore() {
    for accepted in ['a', 'Z', '0', '9', '_'] {
        assert!(c_identifier_char(accepted), "{accepted} should be accepted");
    }
    for rejected in ['-', ' ', '状'] {
        assert!(
            !c_identifier_char(rejected),
            "{rejected} should be rejected"
        );
    }
}

#[test]
fn declaration_prefixes_match_only_supported_c_tokens() {
    for accepted in ["static", "const", "__attribute__", "__always_inline"] {
        assert!(
            c_declaration_prefix_token(accepted),
            "{accepted} should be accepted"
        );
    }
    for rejected in ["static_value", "Status", "attribute_like"] {
        assert!(
            !c_declaration_prefix_token(rejected),
            "{rejected} should be rejected"
        );
    }
}
