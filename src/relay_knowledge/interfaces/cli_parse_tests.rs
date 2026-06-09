use super::*;
use crate::{api::ServicePlanRequest, domain::ServiceManagerAction};

#[test]
fn parses_index_and_service_actions() {
    let index = CliCommand::parse(["index", "refresh", "--kind", "bm25"])
        .expect("index command should parse");
    let service = CliCommand::parse(["service", "doctor"]).expect("service command should parse");
    let run = CliCommand::parse(["service", "run", "--mcp", "streamable-http"])
        .expect("service run should parse");

    assert_eq!(
        index.action,
        CliAction::IndexRefresh {
            kinds: vec![IndexKind::Bm25],
        }
    );
    assert_eq!(service.action, CliAction::ServiceStatus);
    assert_eq!(
        run.action,
        CliAction::ServiceRun {
            mcp: ServiceMcpTransport::StreamableHttp,
            web: false,
        }
    );
}

#[test]
fn parses_service_run_with_web_and_mcp() {
    let web = CliCommand::parse(["service", "run", "--web"]).expect("web service should parse");
    let combined = CliCommand::parse(["service", "run", "--web", "--mcp", "streamable-http"])
        .expect("combined service should parse");

    assert_eq!(
        web.action,
        CliAction::ServiceRun {
            mcp: ServiceMcpTransport::Configured,
            web: true,
        }
    );
    assert_eq!(
        combined.action,
        CliAction::ServiceRun {
            mcp: ServiceMcpTransport::StreamableHttp,
            web: true,
        }
    );
}

#[test]
fn parses_service_lifecycle_execute_options() {
    let lifecycle = CliCommand::parse([
        "service",
        "lifecycle",
        "upgrade",
        "--execute",
        "--target-version",
        "1.2.3",
        "--install-dir",
        "/opt/relay",
    ])
    .expect("service lifecycle should parse");

    assert_eq!(
        lifecycle.action,
        CliAction::ServicePlan(ServicePlanRequest {
            action: ServiceManagerAction::Upgrade,
            dry_run: false,
            execute: true,
            target_version: Some("1.2.3".to_owned()),
            install_dir: Some("/opt/relay".to_owned()),
        })
    );
}

#[test]
fn rejects_mixed_service_lifecycle_dry_run_and_execute_flags() {
    let dry_run_then_execute =
        CliCommand::parse(["service", "lifecycle", "install", "--dry-run", "--execute"])
            .expect_err("mixed lifecycle execution flags should fail");
    let execute_then_dry_run =
        CliCommand::parse(["service", "lifecycle", "install", "--execute", "--dry-run"])
            .expect_err("mixed lifecycle execution flags should fail");

    assert!(
        dry_run_then_execute
            .to_string()
            .contains("unexpected option '--execute' for 'service lifecycle'")
    );
    assert!(
        execute_then_dry_run
            .to_string()
            .contains("unexpected option '--dry-run' for 'service lifecycle'")
    );
}

#[test]
fn environment_remote_base_url_only_selects_supported_repo_commands() {
    let env_remote = Some("http://127.0.0.1:8791".to_owned());
    let status = CliCommand::parse(["status"]).expect("status should parse");
    let repo_status =
        CliCommand::parse(["repo", "status", "org/repo"]).expect("repo status should parse");
    let repo_software =
        CliCommand::parse(["repo", "software", "org/repo", "--kind", "relationships"])
            .expect("repo software should parse");
    let repo_reset = CliCommand::parse(["repo", "index", "--reset", "org/repo"])
        .expect("repo reset should parse");
    let repo_worker =
        CliCommand::parse(["repo", "index-worker"]).expect("repo worker should parse");
    let explicit_status = CliCommand::parse(["--remote", "http://127.0.0.1:8791", "status"])
        .expect("explicit remote status should parse");

    assert!(!remote_environment_needed(&status));
    assert!(remote_environment_needed(&repo_status));
    assert!(remote_environment_needed(&repo_software));
    assert!(remote_environment_needed(&repo_reset));
    assert!(remote_environment_needed(&repo_worker));
    assert!(remote_environment_needed(&explicit_status));
    assert_eq!(remote_selection(&status, env_remote.clone()), None);
    assert_eq!(
        remote_selection(&repo_status, env_remote),
        Some(RemoteSelection {
            base_url: "http://127.0.0.1:8791".to_owned(),
            explicit: false,
        })
    );
    assert_eq!(
        remote_selection(&repo_software, Some("http://127.0.0.1:8791".to_owned())),
        Some(RemoteSelection {
            base_url: "http://127.0.0.1:8791".to_owned(),
            explicit: false,
        })
    );
    assert_eq!(
        remote_selection(&repo_reset, Some("http://127.0.0.1:8791".to_owned())),
        Some(RemoteSelection {
            base_url: "http://127.0.0.1:8791".to_owned(),
            explicit: false,
        })
    );
    assert_eq!(
        remote_selection(&repo_worker, Some("http://127.0.0.1:8791".to_owned())),
        Some(RemoteSelection {
            base_url: "http://127.0.0.1:8791".to_owned(),
            explicit: false,
        })
    );
    assert_eq!(
        remote_selection(&explicit_status, None),
        Some(RemoteSelection {
            base_url: "http://127.0.0.1:8791".to_owned(),
            explicit: true,
        })
    );
}
