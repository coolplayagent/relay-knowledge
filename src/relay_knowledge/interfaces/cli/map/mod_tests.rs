//! Direct contracts for knowledge-map CLI ownership.

use super::*;
use crate::{
    api::{InterfaceKind, RequestContext},
    domain::{KnowledgeMap, RepositoryMapType},
    project::{
        KNOWLEDGE_MAP_RELATIVE_PATH, LEGACY_AGENT_CONTRACT_DIR_NAME,
        LEGACY_KNOWLEDGE_MAP_RELATIVE_PATH,
    },
};
use std::time::{SystemTime, UNIX_EPOCH};

#[test]
fn agent_snippet_is_the_only_map_command_without_a_repository_root() {
    assert!(!MapCommand::AgentSnippet.needs_repository_root());
    for command in [
        MapCommand::Init {
            selection: MapSelection::All,
        },
        MapCommand::Show {
            selection: MapSelection::All,
            topic: None,
            directory: None,
        },
        MapCommand::History {
            selection: MapSelection::All,
            from_version: 1,
            limit: 64,
        },
        MapCommand::Route {
            topic: "build".to_owned(),
        },
        MapCommand::Validate {
            selection: MapSelection::All,
        },
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
fn map_mutation_namespaces_require_an_operation_before_map_type() {
    assert_eq!(
        parse_map(&["source".to_owned()]).expect_err("source operation should be required"),
        CliError::UnexpectedArgument("source".to_owned())
    );
    assert_eq!(
        parse_map(&["directory".to_owned()]).expect_err("directory operation should be required"),
        CliError::UnexpectedArgument("directory".to_owned())
    );
    assert_eq!(
        parse_map(&["source".to_owned(), "add".to_owned()])
            .expect_err("concrete source mutation should still require a map type"),
        CliError::MissingValue("--type")
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

#[tokio::test]
async fn map_execution_keeps_cli_and_repository_governance_in_sync() {
    let root = temp_root("cli-governance");
    tokio::fs::create_dir_all(root.join("codespec/integrations"))
        .await
        .expect("governed directory should create");
    tokio::fs::write(
        root.join("AGENTS.md"),
        "CodeSpec map: codespec/codespec-map.yaml\nKnowledge map: knowledge/knowledge-map.yaml\n",
    )
    .await
    .expect("repository map pointers should write");
    tokio::fs::write(
        root.join("codespec/integrations/README.md"),
        "# Integrations\n",
    )
    .await
    .expect("governed key file should write");
    let service = KnowledgeMapService::new(root.clone());
    let context = RequestContext::with_ids(InterfaceKind::Cli, "req-map-cli", "trace-map-cli");

    let initialized = run_map(
        MapCommand::Init {
            selection: MapSelection::All,
        },
        Some(&service),
        context.clone(),
        OutputFormat::Json,
    )
    .await
    .expect("both repository maps should initialize");
    let initialized: serde_json::Value =
        serde_json::from_str(initialized.trim()).expect("init should render JSON");
    assert_eq!(initialized["results"].as_array().map(Vec::len), Some(2));

    let added = run_map(
        MapCommand::SourceAdd {
            request: KnowledgeMapSourceAddRequest {
                id: "architecture-catalog".to_owned(),
                topic: "architecture".to_owned(),
                kind: KnowledgeMapSourceKind::Doc,
                uri: "docs/architecture.md".to_owned(),
                source_scope: Some("repository".to_owned()),
                description: None,
            },
        },
        Some(&service),
        context.clone(),
        OutputFormat::Json,
    )
    .await
    .expect("knowledge source should add");
    let added: serde_json::Value =
        serde_json::from_str(added.trim()).expect("source add should render JSON");
    assert_eq!(added["map_type"], "knowledge");
    assert_eq!(added["map_version"], 2);

    let updated = run_map(
        MapCommand::SourceUpdate {
            change: KnowledgeMapChange {
                id: "architecture-catalog".to_owned(),
                topic: None,
                kind: None,
                uri: None,
                source_scope: None,
                description: Some("Architecture decisions and constraints".to_owned()),
            },
        },
        Some(&service),
        context.clone(),
        OutputFormat::Json,
    )
    .await
    .expect("knowledge source should update");
    let updated: serde_json::Value =
        serde_json::from_str(updated.trim()).expect("source update should render JSON");
    assert_eq!(updated["map_version"], 3);

    let routed = run_map(
        MapCommand::Route {
            topic: "architecture".to_owned(),
        },
        Some(&service),
        context.clone(),
        OutputFormat::Json,
    )
    .await
    .expect("topic should route through the CLI adapter");
    let routed: serde_json::Value =
        serde_json::from_str(routed.trim()).expect("route should render JSON");
    assert_eq!(routed["sources"][0]["id"], "architecture-catalog");
    assert_eq!(
        routed["sources"][0]["description"],
        "Architecture decisions and constraints"
    );

    let shown = run_map(
        MapCommand::Show {
            selection: MapSelection::One(RepositoryMapType::Knowledge),
            topic: Some("architecture".to_owned()),
            directory: None,
        },
        Some(&service),
        context.clone(),
        OutputFormat::Json,
    )
    .await
    .expect("filtered knowledge map should show");
    let shown: serde_json::Value =
        serde_json::from_str(shown.trim()).expect("show should render JSON");
    assert_eq!(shown["map"]["topics"].as_array().map(Vec::len), Some(1));
    assert_eq!(shown["map"]["sources"][0]["id"], "architecture-catalog");

    let directory = RepositoryMapDirectory {
        directory: "integrations".to_owned(),
        purpose: "Cross-system integration specifications.".to_owned(),
        content_scope: vec!["codespec/integrations/**".to_owned()],
        key_files: vec!["codespec/integrations/README.md".to_owned()],
        load_hint: DirectoryLoadHint::OnDemand,
        relations: Vec::new(),
        update_rule: DirectoryUpdateRule::Reviewed,
    };
    let directory_added = run_map(
        MapCommand::DirectoryAdd {
            map_type: RepositoryMapType::Codespec,
            directory,
        },
        Some(&service),
        context.clone(),
        OutputFormat::Json,
    )
    .await
    .expect("CodeSpec directory should add");
    let directory_added: serde_json::Value =
        serde_json::from_str(directory_added.trim()).expect("directory add should render JSON");
    assert_eq!(directory_added["map_type"], "codespec");

    let directory_updated = run_map(
        MapCommand::DirectoryUpdate {
            map_type: RepositoryMapType::Codespec,
            change: RepositoryMapDirectoryChange {
                directory: "integrations".to_owned(),
                purpose: None,
                content_scope: None,
                key_files: None,
                load_hint: Some(DirectoryLoadHint::TaskMatch),
                relations: None,
                update_rule: Some(DirectoryUpdateRule::Generated),
            },
        },
        Some(&service),
        context.clone(),
        OutputFormat::Json,
    )
    .await
    .expect("CodeSpec directory should update");
    let directory_updated: serde_json::Value = serde_json::from_str(directory_updated.trim())
        .expect("directory update should render JSON");
    assert!(
        directory_updated["summary"]
            .as_str()
            .is_some_and(|summary| summary.contains("updated directory integrations"))
    );

    let history = run_map(
        MapCommand::History {
            selection: MapSelection::All,
            from_version: 1,
            limit: 64,
        },
        Some(&service),
        context.clone(),
        OutputFormat::Json,
    )
    .await
    .expect("both map histories should render");
    let history: serde_json::Value =
        serde_json::from_str(history.trim()).expect("history should render JSON");
    assert_eq!(history["results"].as_array().map(Vec::len), Some(2));

    let validation = run_map(
        MapCommand::Validate {
            selection: MapSelection::All,
        },
        Some(&service),
        context.clone(),
        OutputFormat::Json,
    )
    .await
    .expect("both repository maps should validate");
    let validation: serde_json::Value =
        serde_json::from_str(validation.trim()).expect("validation should render JSON");
    assert!(
        validation["results"]
            .as_array()
            .expect("batch results should exist")
            .iter()
            .all(|result| result["valid"] == true)
    );

    let directory_removed = run_map(
        MapCommand::DirectoryRemove {
            map_type: RepositoryMapType::Codespec,
            directory: "integrations".to_owned(),
        },
        Some(&service),
        context.clone(),
        OutputFormat::Json,
    )
    .await
    .expect("custom CodeSpec directory should remove");
    let directory_removed: serde_json::Value = serde_json::from_str(directory_removed.trim())
        .expect("directory remove should render JSON");
    assert!(
        directory_removed["summary"]
            .as_str()
            .is_some_and(|summary| summary.contains("removed directory integrations"))
    );

    let source_removed = run_map(
        MapCommand::SourceRemove {
            id: "architecture-catalog".to_owned(),
        },
        Some(&service),
        context.clone(),
        OutputFormat::Json,
    )
    .await
    .expect("custom knowledge source should remove");
    let source_removed: serde_json::Value =
        serde_json::from_str(source_removed.trim()).expect("source remove should render JSON");
    assert!(
        source_removed["summary"]
            .as_str()
            .is_some_and(|summary| summary.contains("removed source architecture-catalog"))
    );

    let snippet = run_map(
        MapCommand::AgentSnippet,
        None,
        context.clone(),
        OutputFormat::Json,
    )
    .await
    .expect("agent snippet should not require a repository root");
    let snippet: serde_json::Value =
        serde_json::from_str(snippet.trim()).expect("snippet should render JSON");
    assert!(
        snippet["snippet"]
            .as_str()
            .is_some_and(|value| value.contains(KNOWLEDGE_MAP_RELATIVE_PATH))
    );

    let missing_root = run_map(
        MapCommand::Route {
            topic: "architecture".to_owned(),
        },
        None,
        context,
        OutputFormat::Json,
    )
    .await
    .expect_err("repository-backed commands should require a resolved root");
    assert!(
        missing_root
            .render_stderr()
            .contains("knowledge map repository root was not resolved")
    );

    tokio::fs::remove_dir_all(root)
        .await
        .expect("temporary repository should remove");
}

#[tokio::test]
async fn map_migration_commands_preserve_a_recoverable_legacy_root() {
    let root = temp_root("cli-migration");
    tokio::fs::create_dir_all(root.join(LEGACY_AGENT_CONTRACT_DIR_NAME))
        .await
        .expect("legacy contract directory should create");
    tokio::fs::write(
        root.join("AGENTS.md"),
        "CodeSpec map: codespec/codespec-map.yaml\nKnowledge map: knowledge/knowledge-map.yaml\n",
    )
    .await
    .expect("repository map pointers should write");
    tokio::fs::write(
        root.join(LEGACY_KNOWLEDGE_MAP_RELATIVE_PATH),
        serde_norway::to_string(&KnowledgeMap::initial("unix:1".to_owned()))
            .expect("legacy map should serialize"),
    )
    .await
    .expect("legacy map should write");
    let service = KnowledgeMapService::new(root.clone());
    let context = RequestContext::with_ids(
        InterfaceKind::Cli,
        "req-map-migration",
        "trace-map-migration",
    );

    let migrated = run_map(
        MapCommand::MigrateToV3,
        Some(&service),
        context.clone(),
        OutputFormat::Json,
    )
    .await
    .expect("legacy map should migrate through the CLI adapter");
    let migrated: serde_json::Value =
        serde_json::from_str(migrated.trim()).expect("migration should render JSON");
    assert_eq!(migrated["map_type"], "knowledge");
    assert!(
        tokio::fs::read_to_string(root.join(KNOWLEDGE_MAP_RELATIVE_PATH))
            .await
            .expect("v3 root should read")
            .contains("schema_version: 3")
    );

    let rolled_back = run_map(
        MapCommand::MigrateRollback,
        Some(&service),
        context,
        OutputFormat::Json,
    )
    .await
    .expect("migration should roll back through the CLI adapter");
    let rolled_back: serde_json::Value =
        serde_json::from_str(rolled_back.trim()).expect("rollback should render JSON");
    assert!(
        rolled_back["summary"]
            .as_str()
            .is_some_and(|summary| summary.contains("restored Knowledge Map v2 root"))
    );
    assert!(
        !tokio::fs::try_exists(root.join(KNOWLEDGE_MAP_RELATIVE_PATH))
            .await
            .expect("v3 root should be probed after rollback")
    );

    tokio::fs::remove_dir_all(root)
        .await
        .expect("temporary repository should remove");
}

#[test]
fn map_parser_covers_the_complete_governance_command_surface() {
    let args = |values: &[&str]| {
        values
            .iter()
            .map(|value| (*value).to_owned())
            .collect::<Vec<_>>()
    };

    assert_eq!(
        parse_map(&args(&["init", "--type", "codespec"])).expect("typed init should parse"),
        CliAction::Map(MapCommand::Init {
            selection: MapSelection::One(RepositoryMapType::Codespec),
        })
    );
    assert!(matches!(
        parse_map(&args(&[
            "show",
            "--type",
            "knowledge",
            "--topic",
            "architecture",
            "--directory",
            "best-practices",
        ]))
        .expect("filtered show should parse"),
        CliAction::Map(MapCommand::Show {
            selection: MapSelection::One(RepositoryMapType::Knowledge),
            topic: Some(_),
            directory: Some(_),
        })
    ));
    assert_eq!(
        parse_map(&args(&["history"])).expect("default history window should parse"),
        CliAction::Map(MapCommand::History {
            selection: MapSelection::All,
            from_version: 1,
            limit: 64,
        })
    );
    assert_eq!(
        parse_map(&args(&[
            "source",
            "update",
            "--type",
            "knowledge",
            "--id",
            "architecture-catalog",
            "--topic",
            "design",
            "--kind",
            "doc",
            "--uri",
            "docs/design.md",
            "--scope",
            "repository",
            "--description",
            "Design contracts",
        ]))
        .expect("full source update should parse"),
        CliAction::Map(MapCommand::SourceUpdate {
            change: KnowledgeMapChange {
                id: "architecture-catalog".to_owned(),
                topic: Some("design".to_owned()),
                kind: Some(KnowledgeMapSourceKind::Doc),
                uri: Some("docs/design.md".to_owned()),
                source_scope: Some("repository".to_owned()),
                description: Some("Design contracts".to_owned()),
            },
        })
    );
    assert_eq!(
        parse_map(&args(&[
            "source",
            "remove",
            "--type",
            "knowledge",
            "--id",
            "architecture-catalog",
        ]))
        .expect("source removal should parse"),
        CliAction::Map(MapCommand::SourceRemove {
            id: "architecture-catalog".to_owned(),
        })
    );

    for load_hint in ["always", "task_match", "on_demand"] {
        for update_rule in ["reviewed", "generated", "external_sync"] {
            let command = parse_map(&args(&[
                "directory",
                "add",
                "--type",
                "codespec",
                "--directory",
                "integrations",
                "--purpose",
                "Integration contracts",
                "--content-scope",
                "codespec/integrations/**",
                "--key-file",
                "codespec/integrations/README.md",
                "--load-hint",
                load_hint,
                "--relation",
                "documents=knowledge:architecture",
                "--update-rule",
                update_rule,
            ]))
            .expect("directory add variants should parse");
            assert!(matches!(
                command,
                CliAction::Map(MapCommand::DirectoryAdd {
                    map_type: RepositoryMapType::Codespec,
                    ..
                })
            ));
        }
    }

    for relation in [
        "depends_on=knowledge:architecture",
        "implements=knowledge:architecture",
        "documents=knowledge:architecture",
        "tests=knowledge:architecture",
        "operates=knowledge:architecture",
        "related_to=knowledge:architecture",
    ] {
        assert!(matches!(
            parse_map(&args(&[
                "directory",
                "update",
                "--type",
                "codespec",
                "--directory",
                "integrations",
                "--purpose",
                "Updated contracts",
                "--content-scope",
                "codespec/integrations/**",
                "--key-file",
                "codespec/integrations/README.md",
                "--load-hint",
                "task_match",
                "--relation",
                relation,
                "--update-rule",
                "generated",
            ]))
            .expect("directory update variants should parse"),
            CliAction::Map(MapCommand::DirectoryUpdate { .. })
        ));
    }

    assert_eq!(
        parse_map(&args(&[
            "directory",
            "remove",
            "--type",
            "codespec",
            "--directory",
            "integrations",
        ]))
        .expect("directory removal should parse"),
        CliAction::Map(MapCommand::DirectoryRemove {
            map_type: RepositoryMapType::Codespec,
            directory: "integrations".to_owned(),
        })
    );
    assert_eq!(
        parse_map(&args(&["migrate", "--type", "knowledge", "--to-v3",]))
            .expect("forward migration should parse"),
        CliAction::Map(MapCommand::MigrateToV3)
    );
    assert_eq!(
        parse_map(&args(&["migrate", "--type", "knowledge", "--rollback",]))
            .expect("migration rollback should parse"),
        CliAction::Map(MapCommand::MigrateRollback)
    );
    assert_eq!(
        parse_map(&args(&["agent-snippet"])).expect("agent snippet should parse"),
        CliAction::Map(MapCommand::AgentSnippet)
    );

    for invalid in [
        vec!["init", "extra"],
        vec!["validate", "extra"],
        vec!["route", "topic", "--type", "codespec"],
        vec!["source", "remove", "--type", "knowledge"],
        vec![
            "directory",
            "update",
            "--type",
            "codespec",
            "--directory",
            "x",
        ],
        vec!["directory", "remove", "--type", "codespec"],
        vec!["migrate", "--type", "knowledge", "--unknown"],
        vec!["show", "--unknown"],
        vec!["history", "--limit", "many"],
        vec!["history", "--from", "old"],
        vec!["agent-snippet", "extra"],
    ] {
        assert!(
            parse_map(&args(&invalid)).is_err(),
            "invalid map command should fail: {invalid:?}"
        );
    }
}

fn temp_root(name: &str) -> std::path::PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock should follow the epoch")
        .as_nanos();
    std::env::temp_dir().join(format!(
        "relay-knowledge-map-{name}-{}-{nonce}",
        std::process::id()
    ))
}
