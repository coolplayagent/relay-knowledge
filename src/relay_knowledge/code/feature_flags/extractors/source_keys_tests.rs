use super::{config_read_keys, env_keys, preprocessor_flag_keys, usage_edge_kind};

#[test]
fn environment_extractors_cover_call_member_and_bracket_forms() {
    let line = r#"let values = [env::var("RUST_FLAG"), process.env.NODE_FLAG, ENV["RUBY_FLAG"]];"#;

    assert_eq!(
        env_keys(line),
        ["RUST_FLAG", "NODE_FLAG", "RUBY_FLAG"]
            .map(str::to_owned)
            .to_vec()
    );
}

#[test]
fn config_extractors_ignore_calls_inside_string_literals() {
    let line = r#"log("config.get(\"ignored\")"); settings.get_bool("active")"#;

    assert_eq!(config_read_keys(line), ["active"]);
}

#[test]
fn preprocessor_extractors_filter_language_keywords() {
    let keys = preprocessor_flag_keys("#if defined(ENABLE_ALPHA) && !defined(ENABLE_BETA)", "cpp");

    assert_eq!(keys, ["ENABLE_ALPHA", "ENABLE_BETA"]);
    assert!(preprocessor_flag_keys("#if ENABLE_ALPHA", "rust").is_empty());
}

#[test]
fn usage_kind_distinguishes_guards_from_plain_reads() {
    assert_eq!(
        usage_edge_kind("if flags.enabled(\"checkout\")"),
        "guards_code"
    );
    assert_eq!(
        usage_edge_kind("let enabled = flags.enabled(\"checkout\")"),
        "reads_config"
    );
}
