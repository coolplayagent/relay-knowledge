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
