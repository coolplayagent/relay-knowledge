use super::*;

#[test]
fn parses_knowledge_map_source_commands() {
    let add = CliCommand::parse([
        "map",
        "source",
        "add",
        "--type",
        "knowledge",
        "--id",
        "build-cargo",
        "--topic",
        "build",
        "--kind",
        "config",
        "--uri",
        "Cargo.toml",
        "--scope",
        "repo",
    ])
    .expect("map source add should parse");
    let route = CliCommand::parse(["map", "route", "build", "--type", "knowledge"])
        .expect("map route should parse");
    let history = CliCommand::parse(["map", "history", "--from", "17", "--limit", "32"])
        .expect("map history should parse");
    let validate = CliCommand::parse(["map", "validate"]).expect("map validate should parse");

    assert!(matches!(
        add.action,
        CliAction::Map(map::MapCommand::SourceAdd { .. })
    ));
    assert_eq!(
        route.action,
        CliAction::Map(map::MapCommand::Route {
            topic: "build".to_owned(),
        })
    );
    assert_eq!(
        validate.action,
        CliAction::Map(map::MapCommand::Validate {
            selection: map::MapSelection::All,
        })
    );
    assert_eq!(
        history.action,
        CliAction::Map(map::MapCommand::History {
            selection: map::MapSelection::All,
            from_version: 17,
            limit: 32,
        })
    );
    assert!(
        CliCommand::parse([
            "map",
            "source",
            "add",
            "--type",
            "knowledge",
            "--id",
            "bad",
            "--kind",
            "spreadsheet"
        ])
        .expect_err("invalid source kind should fail")
        .to_string()
        .contains("invalid --kind value 'spreadsheet'")
    );
}

#[test]
fn map_source_kind_diagnostics_are_machine_readable() {
    let error = CliCommand::parse([
        "map",
        "source",
        "add",
        "--type",
        "knowledge",
        "--id",
        "bad",
        "--topic",
        "build",
        "--kind",
        "spreadsheet",
        "--uri",
        "Cargo.toml",
        "--format",
        "json",
    ])
    .expect_err("invalid source kind should fail");

    let rendered = error.render_stderr();

    assert!(rendered.contains("\"unexpected_token\":\"spreadsheet\""));
    assert!(rendered.contains("repo"));
    assert!(rendered.contains("monitoring"));
}

#[test]
fn parses_typed_directory_crud_and_rejects_implicit_mutation_type() {
    let add = CliCommand::parse([
        "map",
        "directory",
        "add",
        "--type",
        "knowledge",
        "--directory",
        "integrations",
        "--purpose",
        "Integration knowledge",
        "--content-scope",
        "knowledge/integrations/**",
        "--key-file",
        "knowledge/integrations/README.md",
        "--load-hint",
        "on_demand",
        "--relation",
        "documents=codespec:api",
        "--update-rule",
        "reviewed",
    ])
    .expect("typed directory add should parse");
    assert!(matches!(
        add.action,
        CliAction::Map(map::MapCommand::DirectoryAdd {
            map_type: crate::domain::RepositoryMapType::Knowledge,
            ..
        })
    ));
    assert!(
        CliCommand::parse(["map", "directory", "remove", "--directory", "integrations"]).is_err()
    );
}
