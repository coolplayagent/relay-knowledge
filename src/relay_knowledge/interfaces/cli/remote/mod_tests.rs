//! Direct contracts for the remote CLI transport owner.

use super::*;
use crate::{
    api::{InterfaceKind, RequestContext},
    domain::{
        BusinessKnowledgeQueryKind, CodeQueryKind, CodebaseViewKind, FreshnessPolicy,
        SoftwareGlobalKind,
    },
    interfaces::cli::repo::view::RepoViewCommand,
};

#[test]
fn status_error_maps_http_429_to_qos_rejected() {
    let error = status_error(
        StatusCode::TOO_MANY_REQUESTS,
        std::borrow::Cow::Borrowed("request budget exhausted"),
    );

    assert_eq!(error.error_kind, ErrorKind::QosRejected);
    assert!(error.message.contains("request budget exhausted"));
}

#[test]
fn repository_update_is_remote_and_never_falls_back_to_local_state() {
    let action = CliAction::Repo(RepoCommand::Update {
        alias: "core".to_owned(),
        base_ref: None,
        head_ref: None,
    });

    assert!(supports(&action));
    assert!(blocks_local_fallback(&action));
}

#[test]
fn repository_index_forwards_historical_reuse_to_remote_request() {
    let request = remote_index_request("core", "HEAD", false, true, OutputFormat::Json)
        .expect("remote index request should map");

    assert!(request.reuse_historical);
    assert_eq!(request.mode, CodeIndexMode::Full);
}

#[tokio::test]
async fn every_repository_remote_command_rejects_an_empty_alias_before_transport() {
    let actions = vec![
        RepoCommand::Index {
            alias: String::new(),
            ref_selector: "HEAD".to_owned(),
            dry_run: false,
            reuse_historical: false,
        },
        RepoCommand::ScopePreview {
            alias: String::new(),
            ref_selector: "HEAD".to_owned(),
        },
        RepoCommand::Update {
            alias: String::new(),
            base_ref: Some("HEAD~1".to_owned()),
            head_ref: Some("HEAD".to_owned()),
        },
        RepoCommand::Query {
            alias: String::new(),
            query: "catalog schema".to_owned(),
            kind: CodeQueryKind::Hybrid,
            limit: 10,
            ref_selector: "HEAD".to_owned(),
            path_filters: Vec::new(),
            language_filters: Vec::new(),
            freshness: FreshnessPolicy::AllowStale,
            exclude_generated: true,
        },
        RepoCommand::Graph {
            alias: String::new(),
            focus_path: "src/lib.rs".to_owned(),
            depth: 2,
            ref_selector: "HEAD".to_owned(),
            path_filters: Vec::new(),
            node_limit: 50,
            edge_limit: 100,
        },
        RepoCommand::Context {
            alias: String::new(),
            query: "catalog schema".to_owned(),
            limit: 10,
            ref_selector: "HEAD".to_owned(),
            path_filters: Vec::new(),
            language_filters: Vec::new(),
            freshness: FreshnessPolicy::AllowStale,
            max_context_bytes: 16_384,
            include_code: true,
            exclude_generated: true,
        },
        RepoCommand::FeatureFlags {
            alias: String::new(),
            query: Some("catalog".to_owned()),
            limit: 10,
            ref_selector: "HEAD".to_owned(),
            path_filters: Vec::new(),
            language_filters: Vec::new(),
            freshness: FreshnessPolicy::AllowStale,
        },
        RepoCommand::FrameworkGraph {
            alias: String::new(),
            query: Some("router".to_owned()),
            frameworks: Vec::new(),
            kinds: Vec::new(),
            limit: 10,
            ref_selector: "HEAD".to_owned(),
            path_filters: Vec::new(),
            freshness: FreshnessPolicy::AllowStale,
        },
        RepoCommand::Impact {
            alias: String::new(),
            base_ref: "HEAD~1".to_owned(),
            head_ref: "HEAD".to_owned(),
            limit: 10,
        },
        RepoCommand::Report {
            alias: String::new(),
        },
        RepoCommand::Software {
            alias: String::new(),
            ref_selector: "HEAD".to_owned(),
            kind: SoftwareGlobalKind::All,
            freshness: FreshnessPolicy::AllowStale,
            limit: 10,
        },
        RepoCommand::Business {
            alias: String::new(),
            ref_selector: "HEAD".to_owned(),
            domain: Some("architecture".to_owned()),
            query: Some("catalog".to_owned()),
            kind: BusinessKnowledgeQueryKind::All,
            freshness: FreshnessPolicy::AllowStale,
            limit: 10,
        },
        RepoCommand::View(RepoViewCommand {
            alias: String::new(),
            kind: CodebaseViewKind::ArchitectureLayers,
            limit: 10,
            ref_selector: "HEAD".to_owned(),
            path_filters: Vec::new(),
            language_filters: Vec::new(),
            freshness: FreshnessPolicy::AllowStale,
            changed_paths: Vec::new(),
        }),
        RepoCommand::Status {
            alias: String::new(),
        },
    ];
    let context = RequestContext::with_ids(
        InterfaceKind::Cli,
        "req-empty-remote-alias",
        "trace-empty-remote-alias",
    );

    for command in actions {
        let error = run_remote(
            &NetworkEnvOverrides::default(),
            "http://127.0.0.1:9",
            &CliAction::Repo(command.clone()),
            context.clone(),
            OutputFormat::Json,
        )
        .await
        .expect_err("an empty repository alias must fail before transport");
        let rendered = error.render_stderr();
        assert!(
            rendered.contains("\"error_kind\":\"invalid_argument\""),
            "unexpected error for {command:?}: {rendered}"
        );
        assert!(
            rendered.contains("repository"),
            "repository identity should be named for {command:?}: {rendered}"
        );
    }
}

#[tokio::test]
async fn remote_command_families_map_connection_failures_without_local_fallback() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("ephemeral listener should bind");
    let unavailable_address = listener
        .local_addr()
        .expect("ephemeral listener address should resolve");
    drop(listener);
    let base_url = format!("http://{unavailable_address}");
    let actions = vec![
        RepoCommand::Index {
            alias: "fixture".to_owned(),
            ref_selector: "HEAD".to_owned(),
            dry_run: true,
            reuse_historical: true,
        },
        RepoCommand::ScopePreview {
            alias: "fixture".to_owned(),
            ref_selector: "HEAD".to_owned(),
        },
        RepoCommand::Update {
            alias: "fixture".to_owned(),
            base_ref: Some("HEAD~1".to_owned()),
            head_ref: Some("HEAD".to_owned()),
        },
        RepoCommand::Graph {
            alias: "fixture".to_owned(),
            focus_path: "src/lib.rs".to_owned(),
            depth: 2,
            ref_selector: "HEAD".to_owned(),
            path_filters: vec!["src".to_owned()],
            node_limit: 50,
            edge_limit: 100,
        },
        RepoCommand::Context {
            alias: "fixture".to_owned(),
            query: "catalog schema".to_owned(),
            limit: 10,
            ref_selector: "HEAD".to_owned(),
            path_filters: Vec::new(),
            language_filters: Vec::new(),
            freshness: FreshnessPolicy::AllowStale,
            max_context_bytes: 16_384,
            include_code: true,
            exclude_generated: true,
        },
        RepoCommand::FeatureFlags {
            alias: "fixture".to_owned(),
            query: Some("catalog".to_owned()),
            limit: 10,
            ref_selector: "HEAD".to_owned(),
            path_filters: Vec::new(),
            language_filters: Vec::new(),
            freshness: FreshnessPolicy::AllowStale,
        },
        RepoCommand::FrameworkGraph {
            alias: "fixture".to_owned(),
            query: Some("router".to_owned()),
            frameworks: Vec::new(),
            kinds: Vec::new(),
            limit: 10,
            ref_selector: "HEAD".to_owned(),
            path_filters: Vec::new(),
            freshness: FreshnessPolicy::AllowStale,
        },
        RepoCommand::Impact {
            alias: "fixture".to_owned(),
            base_ref: "HEAD~1".to_owned(),
            head_ref: "HEAD".to_owned(),
            limit: 10,
        },
        RepoCommand::Report {
            alias: "fixture".to_owned(),
        },
        RepoCommand::Business {
            alias: "fixture".to_owned(),
            ref_selector: "HEAD".to_owned(),
            domain: Some("architecture".to_owned()),
            query: Some("catalog".to_owned()),
            kind: BusinessKnowledgeQueryKind::All,
            freshness: FreshnessPolicy::AllowStale,
            limit: 10,
        },
        RepoCommand::View(RepoViewCommand {
            alias: "fixture".to_owned(),
            kind: CodebaseViewKind::ArchitectureLayers,
            limit: 10,
            ref_selector: "HEAD".to_owned(),
            path_filters: Vec::new(),
            language_filters: Vec::new(),
            freshness: FreshnessPolicy::AllowStale,
            changed_paths: Vec::new(),
        }),
        RepoCommand::Status {
            alias: "fixture".to_owned(),
        },
    ];
    let context = RequestContext::with_ids(
        InterfaceKind::Cli,
        "req-remote-unavailable",
        "trace-remote-unavailable",
    );

    for command in actions {
        let error = run_remote(
            &NetworkEnvOverrides::default(),
            &base_url,
            &CliAction::Repo(command.clone()),
            context.clone(),
            OutputFormat::Json,
        )
        .await
        .expect_err("an unavailable service must not fall back to local repository state");
        let rendered = error.render_stderr();
        assert!(
            rendered.contains("\"error_kind\":\"storage_unavailable\""),
            "unexpected transport mapping for {command:?}: {rendered}"
        );
        assert!(
            rendered.contains("remote service request failed"),
            "transport context should be retained for {command:?}: {rendered}"
        );
    }
}

#[tokio::test]
async fn non_remote_actions_do_not_consume_network_capacity() {
    let context =
        RequestContext::with_ids(InterfaceKind::Cli, "req-local-action", "trace-local-action");
    let process_action = run_remote(
        &NetworkEnvOverrides::default(),
        "not a URL",
        &CliAction::Version,
        context.clone(),
        OutputFormat::Json,
    )
    .await
    .expect("a process-only action should bypass remote configuration");
    assert_eq!(process_action, None);

    let unsupported_repo_action = run_remote(
        &NetworkEnvOverrides::default(),
        "http://127.0.0.1:9",
        &CliAction::Repo(RepoCommand::Register {
            root_path: "/tmp/repository".to_owned(),
            alias: "fixture".to_owned(),
            path_filters: Vec::new(),
            language_filters: Vec::new(),
        }),
        context,
        OutputFormat::Json,
    )
    .await
    .expect("an unsupported remote repository action should not send a request");
    assert_eq!(unsupported_repo_action, None);
}

#[test]
fn remote_urls_are_normalized_and_repository_segments_are_encoded() {
    assert_eq!(
        normalize_base_url("  https://example.test/control///  ", OutputFormat::Json)
            .expect("HTTP base URL should normalize"),
        "https://example.test/control"
    );
    let repository = repository_url(
        "https://example.test/control",
        "org/repository",
        "scope/preview",
        OutputFormat::Json,
    )
    .expect("repository URL should build");
    assert_eq!(
        repository.as_str(),
        "https://example.test/control/api/v1/code/repositories/org%2Frepository/scope/preview"
    );
    let repositories = repositories_url("https://example.test/control", OutputFormat::Json)
        .expect("repository collection URL should build");
    assert_eq!(
        repositories.as_str(),
        "https://example.test/control/api/v1/code/repositories"
    );

    for invalid in [
        "relative/path",
        "ftp://example.test",
        "https://example.test?tenant=core",
        "https://example.test#fragment",
    ] {
        let error = normalize_base_url(invalid, OutputFormat::Json)
            .expect_err("invalid remote base URL should fail");
        assert!(
            error
                .render_stderr()
                .contains("\"error_kind\":\"invalid_argument\"")
        );
    }
    assert!(
        repository_url("https://example.test", "   ", "status", OutputFormat::Json,)
            .expect_err("blank aliases should fail")
            .render_stderr()
            .contains("remote repository alias must not be empty")
    );
}
