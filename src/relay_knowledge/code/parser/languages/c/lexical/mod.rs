//! Shared lexical predicates for C-language adapter and recovery modules.

//! Shared C lexical predicates.

pub(super) fn data_symbol_name(name: &str) -> bool {
    let mut characters = name.chars();
    characters
        .next()
        .is_some_and(|character| character == '_' || character.is_ascii_alphabetic())
        && characters.all(|character| character == '_' || character.is_ascii_alphanumeric())
}

pub(super) fn c_identifier_char(character: char) -> bool {
    character == '_' || character.is_ascii_alphanumeric()
}

pub(super) fn c_declaration_prefix_token(token: &str) -> bool {
    matches!(
        token,
        "__always_inline"
            | "__attribute__"
            | "__attribute"
            | "__declspec"
            | "__declspec__"
            | "__inline"
            | "__inline__"
            | "always_inline"
            | "attribute"
            | "const"
            | "extern"
            | "inline"
            | "register"
            | "restrict"
            | "static"
            | "volatile"
    )
}

#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;
