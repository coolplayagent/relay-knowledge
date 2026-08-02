//! Direct contracts for CLI grammar matching and diagnostic boundaries.

use super::*;

#[test]
fn grammar_matches_command_paths_while_skipping_option_values() {
    let grammar = CliGrammar::new();
    let tokens = [
        "repo".to_owned(),
        "query".to_owned(),
        "core".to_owned(),
        "--limit".to_owned(),
        "5".to_owned(),
    ];

    let invocation = grammar.parse_context(&tokens);

    assert_eq!(invocation.matched_path, ["repo", "query"]);
    assert!(invocation.usage.is_some());
}

#[test]
fn grammar_distance_handles_ascii_and_unicode_command_terms() {
    assert_eq!(edit_distance("query", "qurey"), 2);
    assert_eq!(edit_distance("repo", "repos"), 1);
    assert_eq!(edit_distance("查询", "查找"), 1);
}

#[test]
fn runtime_failures_bypass_parse_diagnostic_rewriting() {
    let error = CliError::RuntimeConfigFailed("missing runtime".to_owned());

    assert_eq!(
        diagnose(&["status".to_owned()], error.clone(), OutputFormat::Json),
        error
    );
}
