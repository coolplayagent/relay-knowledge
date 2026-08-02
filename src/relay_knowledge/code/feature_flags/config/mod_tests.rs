//! Unit contract for boolean feature-flag configuration scanning.

use super::*;

#[test]
fn boolean_config_keys_accept_direct_and_inline_values() {
    assert_eq!(
        boolean_config_keys("checkout.enabled = true"),
        ["checkout.enabled"]
    );
    assert_eq!(
        boolean_config_keys("{ checkout_v2: true, 'payments-v2': false }"),
        ["checkout_v2", "payments-v2"]
    );
}

#[test]
fn boolean_config_keys_reject_strings_arrays_and_invalid_keys() {
    assert!(boolean_config_keys("url = 'https://example.test/true'").is_empty());
    assert!(boolean_config_keys("flags = [true, false]").is_empty());
    assert!(boolean_config_keys("bad/key = enabled").is_empty());
}

#[test]
fn config_file_detection_requires_an_exact_supported_suffix() {
    assert!(looks_like_config_file("config/flags.toml"));
    assert!(looks_like_config_file(".env"));
    assert!(!looks_like_config_file("config/flags.toml.bak"));
    assert!(!looks_like_config_file("src/config.rs"));
}
