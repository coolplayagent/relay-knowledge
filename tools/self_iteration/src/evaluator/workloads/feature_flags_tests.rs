use super::*;

#[test]
fn command_preserves_filters_and_false_boolean() {
    let args = query_command(
        Path::new("relay"),
        "demo",
        "HEAD",
        &serde_json::json!({
            "query":"toggle", "source":"java", "domain":"system", "hot_reload":false,
            "path_filters":["src"], "language_filters":["java"], "consistency":true
        }),
    );
    for pair in [
        ["--query", "toggle"],
        ["--hot-reload", "false"],
        ["--source", "java"],
        ["--path", "src"],
        ["--language", "java"],
        ["--domain", "system"],
    ] {
        assert!(args.windows(2).any(|values| values == pair));
    }
    assert!(args.contains(&"--consistency".into()));
}

#[test]
fn scoring_requires_real_flag_usage_and_preserves_failure() {
    let case = serde_json::json!({"expected":[{"name":"toggle", "path":"config.properties", "kind":"defines_config"}], "degraded_reason":false});
    let mut result = CommandResult { name:"flag".into(), command:vec![], exit_code:0, duration_ms:1,
        stdout:serde_json::json!({"flags":[{"source_key":"toggle", "usages":[{"path":"config.properties", "edge_kind":"defines_config"}]}]}).to_string(), stderr:String::new() };
    assert!(score("repo", &case, &result).passed);
    result.exit_code = 1;
    assert!(!score("repo", &case, &result).passed);
    result.exit_code = 0;
    for invalid in [
        serde_json::json!({"flags":[]}),
        serde_json::json!({"flags":[{"source_key":"toggle", "usages":[]}]}),
        serde_json::json!({"flags":[{"source_key":"toggle","usages":[null]}]}),
        serde_json::json!({"results":[]}),
    ] {
        result.stdout = invalid.to_string();
        assert!(!score("repo", &case, &result).passed);
    }
    assert!(!score("repo", &serde_json::json!({"expect_empty":true}), &result).passed);
}
