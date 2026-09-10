//! Java identifier categories, pinned to Unicode 16.0 rather than XID rules.
use unicode_general_category::{GeneralCategory as Category, get_general_category};

pub(super) fn is_start(value: char) -> bool {
    if value.is_ascii() {
        return value.is_ascii_alphabetic() || matches!(value, '_' | '$');
    }
    matches!(
        get_general_category(value),
        Category::UppercaseLetter
            | Category::LowercaseLetter
            | Category::TitlecaseLetter
            | Category::ModifierLetter
            | Category::OtherLetter
            | Category::LetterNumber
            | Category::CurrencySymbol
            | Category::ConnectorPunctuation
    )
}

pub(super) fn is_ignorable(value: char) -> bool {
    matches!(value, '\u{0}'..='\u{8}' | '\u{e}'..='\u{1b}' | '\u{7f}'..='\u{9f}')
        || (!value.is_ascii() && get_general_category(value) == Category::Format)
}

pub(super) fn is_part(value: char) -> bool {
    is_start(value)
        || value.is_ascii_digit()
        || is_ignorable(value)
        || (!value.is_ascii()
            && matches!(
                get_general_category(value),
                Category::DecimalNumber | Category::NonspacingMark | Category::SpacingMark
            ))
}

pub(super) fn valid(value: &str) -> bool {
    let mut chars = value.chars();
    chars.next().is_some_and(is_start) && chars.all(is_part)
}

#[cfg(test)]
#[path = "java_identifiers_tests.rs"]
mod tests;
