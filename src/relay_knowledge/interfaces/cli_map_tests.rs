use super::*;

#[test]
fn parses_knowledge_map_source_commands() {
    let add = CliCommand::parse([
        "map",
        "source",
        "add",
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
    let route = CliCommand::parse(["map", "route", "build"]).expect("map route should parse");
    let validate = CliCommand::parse(["map", "validate"]).expect("map validate should parse");

    assert!(matches!(
        add.action,
        CliAction::Map(map_cli::MapCommand::SourceAdd { .. })
    ));
    assert_eq!(
        route.action,
        CliAction::Map(map_cli::MapCommand::Route {
            topic: "build".to_owned(),
        })
    );
    assert_eq!(
        validate.action,
        CliAction::Map(map_cli::MapCommand::Validate)
    );
    assert!(
        CliCommand::parse([
            "map",
            "source",
            "add",
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
