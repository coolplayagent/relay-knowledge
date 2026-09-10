use super::feature_flag_name;

#[test]
fn display_names_preserve_unicode_and_punctuation_without_changing_ascii_names() {
    for key in ["功能/结账", "/", "::", "🚀"] {
        assert_eq!(feature_flag_name(key), key);
    }
    for (key, expected) in [
        ("FEATURE-X", "feature_x"),
        ("功能.flag", "功能_flag"),
        ("flag.功能", "flag_功能"),
        ("９.flag", "９_flag"),
        ("service:ready", "service_ready"),
        ("feature/checkout", "feature/checkout"),
        (" 功能.Flag-X ", "功能_flag_x"),
    ] {
        assert_eq!(feature_flag_name(key), expected);
    }
}
