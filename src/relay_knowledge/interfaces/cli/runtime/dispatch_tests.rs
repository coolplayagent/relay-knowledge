use super::*;
use crate::{
    application::KnowledgeMapService,
    interfaces::cli::{OutputFormat, map::MapCommand},
};
use std::time::{SystemTime, UNIX_EPOCH};

#[tokio::test]
async fn process_free_help_and_version_actions_skip_runtime_configuration() {
    let help = run_command(
        CliCommand {
            action: CliAction::Help {
                path: vec!["repo".to_owned(), "query".to_owned()],
            },
            format: OutputFormat::Json,
            remote_base_url: None,
            help: true,
        },
        None,
        ProcessRuntimeConfig::default(),
    )
    .await
    .expect("help should render");
    let version = run_command(
        CliCommand {
            action: CliAction::Version,
            format: OutputFormat::Text,
            remote_base_url: None,
            help: false,
        },
        None,
        ProcessRuntimeConfig::default(),
    )
    .await
    .expect("version should render");
    let help: serde_json::Value = serde_json::from_str(help.trim()).expect("help should be JSON");

    assert_eq!(help["path"], serde_json::json!(["repo", "query"]));
    assert!(version.starts_with("relay-knowledge "));
}

#[tokio::test]
async fn map_history_uses_repository_service_resolved_before_dispatch() {
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time should follow the epoch")
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "relay-knowledge-cli-map-dispatch-{}-{suffix}",
        std::process::id()
    ));
    std::fs::create_dir_all(&root).expect("temporary repository should be created");
    let service = KnowledgeMapService::new(root.clone());
    let context = RequestContext::for_interface(InterfaceKind::Cli);
    service
        .init(&context)
        .await
        .expect("knowledge map should initialize");

    let output = run_command(
        CliCommand {
            action: CliAction::Map(MapCommand::History {
                selection: crate::interfaces::cli::map::MapSelection::One(
                    crate::domain::RepositoryMapType::Knowledge,
                ),
                from_version: 1,
                limit: 64,
            }),
            format: OutputFormat::Json,
            remote_base_url: None,
            help: false,
        },
        Some(&service),
        ProcessRuntimeConfig::default(),
    )
    .await
    .expect("map history should use the resolved repository service");
    let output: serde_json::Value =
        serde_json::from_str(output.trim()).expect("history should render JSON");

    assert_eq!(output["map_version"], 1);
    assert_eq!(output["entries"][0]["version"], 1);
    std::fs::remove_dir_all(root).expect("temporary repository should be removed");
}

#[test]
fn unsupported_remote_actions_return_a_stable_capability_error() {
    assert_eq!(
        remote_unsupported_error(),
        CliError::ApiFailed(
            "remote CLI mode supports repo list, repo index, repo scope preview, repo status, repo query, repo graph, repo context, repo feature-flags, repo impact, repo report, repo software, and repo view"
                .to_owned()
        )
    );
}
