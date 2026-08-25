//! Direct tests for quoted import-specifier extraction.

use super::{CIncludeDelimiter, c_include_specifier, quoted};

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

#[test]
fn c_include_specifier_separates_target_and_delimiter_from_source_syntax() {
    let angle = c_include_specifier("#include <vendor/runtime.h> // \"documentation\"")
        .expect("angle include should parse");
    assert_eq!(angle.target, "vendor/runtime.h");
    assert_eq!(angle.delimiter, CIncludeDelimiter::Angle);

    let quoted =
        c_include_specifier("#include \"platform/driver.h\"").expect("quoted include should parse");
    assert_eq!(quoted.target, "platform/driver.h");
    assert_eq!(quoted.delimiter, CIncludeDelimiter::Quoted);

    assert_eq!(c_include_specifier("include vendor/runtime.h"), None);
}
