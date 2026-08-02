//! Direct tests for quoted import-specifier extraction.

use super::quoted;

#[test]
fn quoted_specifier_accepts_both_quote_styles() {
    assert_eq!(quoted("import \"./widget\""), Some("./widget"));
    assert_eq!(quoted("import('./dynamic')"), Some("./dynamic"));
}

#[test]
fn quoted_specifier_rejects_unterminated_values() {
    assert_eq!(quoted("import \"./widget"), None);
    assert_eq!(quoted("import widget"), None);
}
