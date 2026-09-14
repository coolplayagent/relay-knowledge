use super::{detect, manual_parse_status, recoverable_parse_error, static_template_language};

#[test]
fn detects_named_and_extension_based_configuration_languages() {
    assert_eq!(
        detect("Dockerfile.dev").map(|spec| spec.id),
        Some("dockerfile")
    );
    assert_eq!(
        detect("deploy/config.yaml").map(|spec| spec.id),
        Some("yaml")
    );
    assert_eq!(detect("notes.unknown").map(|spec| spec.id), None);
    assert_eq!(static_template_language("values.toml"), Some("toml"));
}

#[test]
fn recovery_requires_a_balanced_language_specific_shape() {
    assert!(recoverable_parse_error("cmake", "set(NAME value)\n"));
    assert!(!recoverable_parse_error("cmake", "set(NAME value\n"));
    assert!(manual_parse_status("gotemplate", "{{ .Values.name }}"));
    assert!(!manual_parse_status("gotemplate", "{{ .Values.name"));
}

#[test]
fn gotemplate_feature_flag_delimiters_inside_literals_and_comments_are_balanced() {
    for source in [
        r#"{{ keyOrDefault "feature" `{"template":"{{value}}"}` }}"#,
        r#"{{ key "{{quoted}}" }}"#,
        "{{/* {{ ignored }} */}}",
        r#"{{ key "escaped\"}}" }}"#,
    ] {
        assert!(manual_parse_status("gotemplate", source), "{source}");
    }
    for source in [
        r#"{{ key "unterminated }}"#,
        "{{/* unterminated }}",
        "{{ key `value`",
    ] {
        assert!(!manual_parse_status("gotemplate", source), "{source}");
    }
}

#[test]
fn go_template_literal_delimiters_do_not_mask_unclosed_actions() {
    for source in [
        "}}",
        "before }} {{ .Name }} after }}",
        r#"{{with .Value}}{"outer":{"inner":true}}{{end}}"#,
    ] {
        assert!(manual_parse_status("gotemplate", source), "{source}");
    }
    for source in ["}} {{", "{{ key \"unterminated }}", "{{/* unclosed }}"] {
        assert!(!manual_parse_status("gotemplate", source), "{source}");
    }
}
