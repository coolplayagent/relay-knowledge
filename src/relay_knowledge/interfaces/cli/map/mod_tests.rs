//! Direct contracts for knowledge-map CLI ownership.

use super::*;

#[test]
fn agent_snippet_is_the_only_map_command_without_a_repository_root() {
    assert!(!MapCommand::AgentSnippet.needs_repository_root());
    for command in [
        MapCommand::Init,
        MapCommand::Show { topic: None },
        MapCommand::History {
            from_version: 1,
            limit: 64,
        },
        MapCommand::Route {
            topic: "build".to_owned(),
        },
        MapCommand::Validate,
    ] {
        assert!(command.needs_repository_root());
    }
}

#[test]
fn map_source_kind_parser_covers_supported_contract_values() {
    let values = [
        ("repo", KnowledgeMapSourceKind::Repo),
        ("file", KnowledgeMapSourceKind::File),
        ("doc", KnowledgeMapSourceKind::Doc),
        ("config", KnowledgeMapSourceKind::Config),
        ("db", KnowledgeMapSourceKind::Db),
        ("ci", KnowledgeMapSourceKind::Ci),
        ("runtime", KnowledgeMapSourceKind::Runtime),
        ("wiki", KnowledgeMapSourceKind::Wiki),
        ("monitoring", KnowledgeMapSourceKind::Monitoring),
    ];

    for (value, expected) in values {
        assert_eq!(source_kind(value), Ok(expected));
    }
    assert_eq!(
        source_kind("spreadsheet").expect_err("unknown source kind should fail"),
        CliError::InvalidMapSourceKind("spreadsheet".to_owned())
    );
}

#[test]
fn writer_lock_expiry_is_reported_as_a_timeout() {
    let error = map_error(
        "knowledge map mutation failed",
        KnowledgeMapServiceError::LockTimeout("map.lock".into()),
        OutputFormat::Json,
    );
    let rendered = error.render_stderr();

    assert!(rendered.contains("\"error_kind\":\"timeout\""));
}
