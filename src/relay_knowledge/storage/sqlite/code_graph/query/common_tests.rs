use super::*;

#[test]
fn query_filters_trim_values_and_reject_empty_or_nul_input() {
    assert_eq!(
        normalize_filter("symbol_name", Some("  main  ".to_owned())).expect("filter should parse"),
        Some("main".to_owned())
    );
    assert_eq!(
        normalize_filter("symbol_name", Some("  ".to_owned()))
            .expect_err("empty filter should fail")
            .to_string(),
        "invalid storage input: symbol_name filter must not be empty"
    );
    assert_eq!(
        normalize_filter("symbol_name", Some("ma\0in".to_owned()))
            .expect_err("NUL filter should fail")
            .to_string(),
        "invalid storage input: symbol_name filter must not contain NUL bytes"
    );
}

#[test]
fn query_limits_must_be_positive() {
    assert_eq!(
        validate_limit("code symbol search limit", 0)
            .expect_err("zero limit should fail")
            .to_string(),
        "invalid storage input: code symbol search limit must be greater than zero"
    );
    validate_limit("code symbol search limit", 1).expect("positive limit should pass");
}
