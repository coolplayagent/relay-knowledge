use super::*;
use crate::project::PROJECT_NAME;

#[test]
fn cli_spec_uses_canonical_binary_name_and_kebab_case_command_paths() {
    let spec = cli_spec::cli_spec();
    let value = serde_json::to_value(spec).expect("CLI spec should serialize");
    let commands = value["commands"].as_array().expect("commands should exist");

    assert_eq!(value["binary"], PROJECT_NAME);
    assert!(commands.len() > 10);

    for command in commands {
        let path = command_path(command);
        let usage = command["usage"].as_str().expect("usage should be a string");

        for segment in &path {
            assert_cli_path_segment(segment);
        }
        assert_usage_invokes_path(usage, &path);

        for example in command["examples"]
            .as_array()
            .expect("examples should be an array")
        {
            assert_invocation_uses_project_name(
                example.as_str().expect("example should be a string"),
            );
        }
    }
}

#[test]
fn text_help_uses_canonical_cli_name_for_root_namespaces_and_commands() {
    let root = cli_spec::render_help(&[], OutputFormat::Text).expect("root help should render");
    let repo = cli_spec::render_help(&["repo".to_owned()], OutputFormat::Text)
        .expect("repo help should render");
    let query = cli_spec::render_help(&["repo".to_owned(), "query".to_owned()], OutputFormat::Text)
        .expect("repo query help should render");

    assert!(root.contains(&format!("Usage: {PROJECT_NAME} <command>")));
    assert!(root.contains(&format!(
        "Use `{PROJECT_NAME} help <command> --format json`"
    )));
    assert!(repo.contains(&format!("Usage: {PROJECT_NAME} repo <subcommand>")));
    assert!(query.contains(&format!("Usage: {PROJECT_NAME} repo query")));
}

#[test]
fn parse_diagnostics_use_canonical_cli_name_in_suggestions_and_usage() {
    let flag_style = CliCommand::parse(["--ingest", "--source", "docs", "--content", "x"])
        .expect_err("flag-style command should fail")
        .render_stderr();
    let typo = CliCommand::parse(["repo", "qurey", "core", "--query", "rust"])
        .expect_err("misspelled command should fail")
        .render_stderr();
    let json_error = CliCommand::parse(["--format", "json", "query", "--query", "SQLite"])
        .expect_err("removed query flag should fail")
        .render_stderr();
    let json: serde_json::Value =
        serde_json::from_str(&json_error).expect("diagnostic should be JSON");

    assert!(flag_style.contains(&format!("Try: {PROJECT_NAME} ingest")));
    assert!(typo.contains("did you mean 'repo query'"));
    assert!(typo.contains(&format!("Try: {PROJECT_NAME} repo query")));
    assert!(typo.contains(&format!("Usage: {PROJECT_NAME} repo <subcommand>")));
    assert_eq!(json["suggestion"], format!("{PROJECT_NAME} query SQLite"));
    assert_eq!(
        json["usage"],
        format!(
            "{PROJECT_NAME} query <text> [--source <scope>] [--limit <n>] [--freshness <policy>]"
        )
    );
}

fn command_path(command: &serde_json::Value) -> Vec<String> {
    command["path"]
        .as_array()
        .expect("path should be an array")
        .iter()
        .map(|segment| {
            segment
                .as_str()
                .expect("path segment should be a string")
                .to_owned()
        })
        .collect()
}

fn assert_cli_path_segment(segment: &str) {
    assert!(!segment.is_empty());
    assert!(!segment.starts_with('-'));
    assert!(!segment.ends_with('-'));
    assert!(!segment.contains("--"));
    assert!(
        segment
            .chars()
            .all(|character| character.is_ascii_lowercase()
                || character.is_ascii_digit()
                || character == '-')
    );
}

fn assert_usage_invokes_path(usage: &str, path: &[String]) {
    assert_invocation_uses_project_name(usage);

    let tokens = usage.split_whitespace().collect::<Vec<_>>();
    assert_eq!(tokens.first().copied(), Some(PROJECT_NAME));

    for (index, segment) in path.iter().enumerate() {
        let usage_segment = tokens
            .get(index + 1)
            .unwrap_or_else(|| panic!("usage is missing path segment '{segment}': {usage}"));
        assert!(
            usage_segment
                .split('|')
                .any(|candidate| candidate == segment.as_str()),
            "usage segment '{usage_segment}' should include path segment '{segment}' in {usage}"
        );
    }
}

fn assert_invocation_uses_project_name(invocation: &str) {
    assert!(
        invocation
            .split_whitespace()
            .any(|token| token == PROJECT_NAME),
        "invocation should include canonical CLI name '{PROJECT_NAME}': {invocation}"
    );
    assert!(!invocation.contains("relay_knowledge"));
    assert!(!invocation.contains("RelayKnowledge"));
}
