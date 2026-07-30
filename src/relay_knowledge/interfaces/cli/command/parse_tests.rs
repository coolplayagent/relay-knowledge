use super::*;

#[test]
fn parses_global_options_around_command_arguments() {
    let command = CliCommand::parse([
        "--remote=https://relay.example",
        "query",
        "durable tasks",
        "--format",
        "json",
    ])
    .expect("global options should parse");

    assert_eq!(command.format, OutputFormat::Json);
    assert_eq!(
        command.remote_base_url.as_deref(),
        Some("https://relay.example")
    );
    assert_eq!(
        command.action,
        CliAction::Query {
            query: "durable tasks".to_owned(),
            source_scope: None,
            limit: 10,
            freshness: crate::domain::FreshnessPolicy::AllowStale,
        }
    );
}

#[test]
fn preserves_dash_prefixed_values_after_value_options_and_delimiters() {
    let content = CliCommand::parse(["ingest", "--source", "docs", "--content", "--version"])
        .expect("value option should preserve dash-prefixed content");
    let query = CliCommand::parse(["query", "--", "--help"])
        .expect("delimiter should preserve dash-prefixed query");

    assert_eq!(
        content.action,
        CliAction::Ingest {
            source_scope: "docs".to_owned(),
            content: "--version".to_owned(),
            entity_labels: Vec::new(),
        }
    );
    assert_eq!(
        query.action,
        CliAction::Query {
            query: "--help".to_owned(),
            source_scope: None,
            limit: 10,
            freshness: crate::domain::FreshnessPolicy::AllowStale,
        }
    );
}

#[test]
fn help_and_version_global_flags_short_circuit_action_dispatch() {
    let help = CliCommand::parse(["repo", "query", "--help"]).expect("help should parse");
    let version = CliCommand::parse(["--version"]).expect("version should parse");

    assert_eq!(
        help.action,
        CliAction::Help {
            path: vec!["repo".to_owned(), "query".to_owned()],
        }
    );
    assert!(help.help);
    assert_eq!(version.action, CliAction::Version);
}
