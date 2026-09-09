use super::*;

#[test]
fn literal_pipeline_inputs_supply_only_proven_last_arguments() {
    for (action, key, default) in [
        ("\"piped_key\" | key", "piped_key", None),
        ("\"PIPED_ENV\" | env", "PIPED_ENV", None),
        (
            "\"true\" | keyOrDefault \"fallback\"",
            "fallback",
            Some("true"),
        ),
    ] {
        let found = reads(action);
        assert_eq!(found.len(), 1, "{action}");
        assert_eq!(found[0].key, key);
        assert_eq!(found[0].default.as_deref(), default);
    }
    for action in [
        "$dynamic | key",
        "\"not_proven\" | printf `%s` | key",
        "printf `%s` (\"nested\") | key",
    ] {
        assert!(reads(action).is_empty(), "{action}");
    }
}

#[test]
fn reads_follow_control_assignment_parentheses_and_pipeline_command_heads() {
    for (action, key) in [
        ("with key \"with_key\"", "with_key"),
        ("if env \"IF_ENV\"", "IF_ENV"),
        ("$v := key \"assigned\"", "assigned"),
        ("printf \"%s\" (key \"nested\")", "nested"),
        ("\"fallback\" | keyOrDefault \"piped\"", "piped"),
        ("template \"name\" key \"argument\"", "argument"),
        ("else if (env \"ELSE_ENV\")", "ELSE_ENV"),
    ] {
        let found = reads(action);
        assert_eq!(found.len(), 1, "{action}");
        assert_eq!(found[0].key, key);
    }
    let found = reads("printf \"%s/%s\" (key \"same\") (keyOrDefault \"same\" \"true\")");
    assert_eq!(found.len(), 2);
    assert_ne!(found[0].offset, found[1].offset);
    assert_eq!(found[1].default.as_deref(), Some("true"));
}

#[test]
fn quoted_raw_comment_field_and_dynamic_tokens_do_not_become_calls() {
    for action in [
        "/* key \"fake\" */",
        "printf \"key \\\"fake\\\"\"",
        "printf `%s` `key \"fake\"`",
        "key $dynamic",
        "$key \"fake\"",
        ".key \"fake\"",
        "printf key \"fake\"",
    ] {
        assert!(reads(action).is_empty(), "{action}");
    }
    let found = reads("keyOrDefault \"outer\" (env \"INNER\")");
    assert_eq!(found.len(), 2);
    assert!(found[0].default.is_none());
    assert_eq!(found[1].kind, "env_var");
}
